//! Local performance telemetry captured from `MangoHud` CSV logs.

use std::cmp::Ordering;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};
use crate::profile::EnabledMod;

pub const DEFAULT_WARMUP_SECONDS: f64 = 30.0;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PerformanceModSnapshot {
    pub mod_id: String,
    pub version: Option<String>,
    pub enabled: bool,
}

impl From<&EnabledMod> for PerformanceModSnapshot {
    fn from(value: &EnabledMod) -> Self {
        Self {
            mod_id: value.mod_id.clone(),
            version: value.version.clone(),
            enabled: value.enabled,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerformanceSample {
    pub elapsed_seconds: Option<f64>,
    pub fps: f64,
    pub frame_time_ms: Option<f64>,
    pub cpu_load: Option<f64>,
    pub gpu_load: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PerformanceSummary {
    pub sample_count: usize,
    pub duration_seconds: Option<f64>,
    pub median_fps: Option<f64>,
    pub average_fps: Option<f64>,
    pub one_percent_low_fps: Option<f64>,
    pub point_one_percent_low_fps: Option<f64>,
    pub median_frame_time_ms: Option<f64>,
    pub p95_frame_time_ms: Option<f64>,
    pub p99_frame_time_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MangoHudParseResult {
    pub samples: Vec<PerformanceSample>,
    pub summary: PerformanceSummary,
}

#[must_use]
pub fn mod_snapshot(mods: &[EnabledMod]) -> Vec<PerformanceModSnapshot> {
    mods.iter()
        .filter(|m| m.enabled)
        .map(PerformanceModSnapshot::from)
        .collect()
}

#[must_use]
pub fn mod_set_hash(snapshot: &[PerformanceModSnapshot]) -> String {
    let mut entries: Vec<String> = snapshot
        .iter()
        .map(|m| format!("{}={}", m.mod_id, m.version.as_deref().unwrap_or("")))
        .collect();
    entries.sort();
    let mut hasher = xxhash_rust::xxh64::Xxh64::new(0);
    for entry in entries {
        hasher.update(entry.as_bytes());
        hasher.update(b"\0");
    }
    format!("{:016x}", hasher.digest())
}

pub fn parse_mangohud_csv_file(path: &Path) -> Result<MangoHudParseResult> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        CoreError::Other(
            format!(
                "failed to read MangoHud CSV {}: {e}. Expected a CSV produced by `modde perf run`; validate with `test -s {}`",
                path.display(),
                path.display()
            )
            .into(),
        )
    })?;
    parse_mangohud_csv(&content).map_err(|e| {
        CoreError::Other(
            format!(
                "{e}. Expected a MangoHud CSV with an fps column and frametime or timing data; validate with `head -n 20 {}`",
                path.display()
            )
            .into(),
        )
    })
}

pub fn parse_mangohud_csv_file_with_warmup(
    path: &Path,
    warmup_seconds: f64,
) -> Result<MangoHudParseResult> {
    let mut parsed = parse_mangohud_csv_file(path)?;
    parsed.summary = summarize_samples_with_warmup(&parsed.samples, warmup_seconds);
    Ok(parsed)
}

pub fn parse_mangohud_csv(content: &str) -> Result<MangoHudParseResult> {
    parse_csv(content, true)
}

/// Capture and grading must retain missing measurements. FPS-derived estimates
/// remain available through `parse_mangohud_csv` for displaying legacy logs, but
/// do not count as measured frame times in benchmarks or performance bisects.
pub fn parse_mangohud_csv_measured(content: &str) -> Result<MangoHudParseResult> {
    parse_csv(content, false)
}

fn parse_csv(content: &str, estimate_missing_column: bool) -> Result<MangoHudParseResult> {
    let lines: Vec<&str> = content.lines().collect();
    let Some((header_idx, columns)) = lines.iter().enumerate().find_map(|(idx, line)| {
        let columns = split_csv_line(line);
        let map = ColumnMap::from_columns(&columns)?;
        (map.fps.is_some() && (map.frame_time_ms.is_some() || map.elapsed_seconds.is_some()))
            .then_some((idx, columns))
    }) else {
        return Err(CoreError::Other(
            "malformed MangoHud CSV: no data header with fps and timing columns found".into(),
        ));
    };

    let map = ColumnMap::from_columns(&columns).ok_or_else(|| {
        CoreError::Other("malformed MangoHud CSV: failed to map data columns".into())
    })?;
    let mut samples = Vec::new();
    for line in lines.iter().skip(header_idx + 1) {
        if line.trim().is_empty() {
            continue;
        }
        let fields = split_csv_line(line);
        let Some(fps) = map.get(&fields, map.fps).and_then(parse_number) else {
            continue;
        };
        if !fps.is_finite() || fps <= 0.0 {
            continue;
        }
        let frame_time_ms = match map.frame_time_ms {
            Some(column) => map.get(&fields, Some(column)).and_then(parse_number),
            None if estimate_missing_column => Some(1000.0 / fps),
            None => None,
        };
        samples.push(PerformanceSample {
            elapsed_seconds: map
                .get(&fields, map.elapsed_seconds)
                .and_then(parse_number)
                .map(|value| {
                    if map.elapsed_nanoseconds {
                        value / 1_000_000_000.0
                    } else {
                        value
                    }
                }),
            fps,
            frame_time_ms,
            cpu_load: map.get(&fields, map.cpu_load).and_then(parse_number),
            gpu_load: map.get(&fields, map.gpu_load).and_then(parse_number),
        });
    }

    if samples.is_empty() {
        return Err(CoreError::Other(
            "malformed MangoHud CSV: no numeric fps samples found".into(),
        ));
    }

    let summary = summarize_samples(&samples);
    Ok(MangoHudParseResult { samples, summary })
}

#[must_use]
pub fn summarize_samples(samples: &[PerformanceSample]) -> PerformanceSummary {
    summarize_samples_with_warmup(samples, DEFAULT_WARMUP_SECONDS)
}

#[must_use]
pub fn summarize_samples_with_warmup(
    samples: &[PerformanceSample],
    warmup_seconds: f64,
) -> PerformanceSummary {
    let filtered = samples_after_warmup(samples, warmup_seconds);
    let summary_samples = if filtered.is_empty() {
        samples
    } else {
        filtered.as_slice()
    };

    let mut fps: Vec<f64> = summary_samples
        .iter()
        .map(|s| s.fps)
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    fps.sort_by(total_cmp);

    let mut frame_times: Vec<f64> = summary_samples
        .iter()
        .filter_map(|s| s.frame_time_ms)
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    frame_times.sort_by(total_cmp);

    let elapsed: Vec<f64> = summary_samples
        .iter()
        .filter_map(|s| s.elapsed_seconds)
        .filter(|v| v.is_finite())
        .collect();
    let duration_seconds = elapsed
        .iter()
        .min_by(|a, b| total_cmp(a, b))
        .zip(elapsed.iter().max_by(|a, b| total_cmp(a, b)))
        .map(|(min, max)| (max - min).max(0.0));

    PerformanceSummary {
        sample_count: fps.len(),
        duration_seconds,
        median_fps: percentile_sorted(&fps, 0.50),
        average_fps: (!fps.is_empty()).then(|| fps.iter().sum::<f64>() / fps.len() as f64),
        one_percent_low_fps: low_percent_average_sorted(&fps, 0.01),
        point_one_percent_low_fps: low_percent_average_sorted(&fps, 0.001),
        median_frame_time_ms: percentile_sorted(&frame_times, 0.50),
        p95_frame_time_ms: percentile_sorted(&frame_times, 0.95),
        p99_frame_time_ms: percentile_sorted(&frame_times, 0.99),
    }
}

fn samples_after_warmup(
    samples: &[PerformanceSample],
    warmup_seconds: f64,
) -> Vec<PerformanceSample> {
    if !warmup_seconds.is_finite() || warmup_seconds <= 0.0 {
        return samples.to_vec();
    }
    let Some(start) = samples
        .iter()
        .filter_map(|sample| sample.elapsed_seconds)
        .filter(|value| value.is_finite())
        .min_by(total_cmp)
    else {
        return samples.to_vec();
    };
    samples
        .iter()
        .filter(|sample| {
            sample
                .elapsed_seconds
                .is_none_or(|elapsed| elapsed - start >= warmup_seconds)
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, Copy, Default)]
struct ColumnMap {
    elapsed_seconds: Option<usize>,
    elapsed_nanoseconds: bool,
    fps: Option<usize>,
    frame_time_ms: Option<usize>,
    cpu_load: Option<usize>,
    gpu_load: Option<usize>,
}

impl ColumnMap {
    fn from_columns(columns: &[String]) -> Option<Self> {
        let mut map = Self::default();
        for (idx, column) in columns.iter().enumerate() {
            let normalized = normalize_column(column);
            if map.fps.is_none() && (normalized == "fps" || normalized.ends_with("_fps")) {
                map.fps = Some(idx);
            } else if map.frame_time_ms.is_none()
                && (normalized.contains("frametime")
                    || normalized.contains("frame_time")
                    || normalized == "ms")
            {
                map.frame_time_ms = Some(idx);
            } else if map.elapsed_seconds.is_none()
                && (normalized == "time"
                    || normalized == "elapsed"
                    || normalized == "elapsed_ns"
                    || normalized == "elapsed_time"
                    || normalized == "elapsed_seconds"
                    || normalized == "time_s")
            {
                map.elapsed_seconds = Some(idx);
                // MangoHud's elapsed column is a nanosecond duration, including
                // when log_versioning=0. Generic time/time_s fields use seconds.
                // Source: https://github.com/flightlessmango/MangoHud/blob/73931de948402caf3ea80f26d5bbb6b3f1d2c1d9/src/logging.cpp#L200-L230
                map.elapsed_nanoseconds = matches!(normalized.as_str(), "elapsed" | "elapsed_ns");
            } else if map.cpu_load.is_none()
                && normalized.contains("cpu")
                && normalized.contains("load")
            {
                map.cpu_load = Some(idx);
            } else if map.gpu_load.is_none()
                && normalized.contains("gpu")
                && normalized.contains("load")
            {
                map.gpu_load = Some(idx);
            }
        }
        (map.fps.is_some()).then_some(map)
    }

    fn get<'a>(&self, fields: &'a [String], idx: Option<usize>) -> Option<&'a str> {
        fields.get(idx?).map(String::as_str)
    }
}

fn split_csv_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    while let Some(ch) = chars.next() {
        match ch {
            '"' if in_quotes && chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                fields.push(field.trim().trim_matches('"').to_string());
                field.clear();
            }
            _ => field.push(ch),
        }
    }
    fields.push(field.trim().trim_matches('"').to_string());
    fields
}

fn normalize_column(column: &str) -> String {
    column
        .trim()
        .trim_start_matches('#')
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '/', '(', ')', '[', ']'], "_")
        .trim_matches('_')
        .to_string()
}

fn parse_number(raw: &str) -> Option<f64> {
    let trimmed = raw.trim().trim_end_matches('%');
    if trimmed.is_empty() {
        return None;
    }
    trimmed.parse::<f64>().ok().filter(|v| v.is_finite())
}

fn percentile_sorted(values: &[f64], percentile: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let idx = ((values.len() - 1) as f64 * percentile).round() as usize;
    values.get(idx).copied()
}

fn low_percent_average_sorted(values: &[f64], fraction: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let count = ((values.len() as f64) * fraction).ceil().max(1.0) as usize;
    let count = count.min(values.len());
    Some(values.iter().take(count).sum::<f64>() / count as f64)
}

fn total_cmp(a: &f64, b: &f64) -> Ordering {
    a.total_cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mangohud_csv_with_metadata_prelude() {
        let csv = "\
MangoHud v0.7
Some metadata,row
time,fps,frametime,cpu_load,gpu_load
0.0,100,10.0,20,90
1.0,50,20.0,30,80
31.0,25,40.0,40,70
";
        let parsed = parse_mangohud_csv(csv).unwrap();
        assert_eq!(parsed.samples.len(), 3);
        assert_eq!(parsed.summary.sample_count, 1);
        assert_eq!(parsed.summary.median_fps, Some(25.0));
        assert_eq!(parsed.summary.duration_seconds, Some(0.0));
        assert_eq!(parsed.summary.p99_frame_time_ms, Some(40.0));
    }

    #[test]
    fn warmup_can_be_disabled_for_summary() {
        let samples = vec![
            PerformanceSample {
                elapsed_seconds: Some(0.0),
                fps: 10.0,
                frame_time_ms: Some(100.0),
                cpu_load: None,
                gpu_load: None,
            },
            PerformanceSample {
                elapsed_seconds: Some(31.0),
                fps: 60.0,
                frame_time_ms: Some(16.0),
                cpu_load: None,
                gpu_load: None,
            },
        ];
        let summary = summarize_samples_with_warmup(&samples, 0.0);
        assert_eq!(summary.sample_count, 2);
        assert_eq!(summary.median_fps, Some(60.0));
    }

    #[test]
    fn parses_different_column_order_and_quoted_headers() {
        let csv = "\
\"GPU Load\",\"Frame Time (ms)\",\"FPS\",\"Elapsed\"
10,8.0,125,0
20,16.0,62.5,1000000000
";
        let parsed = parse_mangohud_csv(csv).unwrap();
        assert!((parsed.samples[0].fps - 125.0).abs() < f64::EPSILON);
        assert_eq!(parsed.samples[1].frame_time_ms, Some(16.0));
        assert_eq!(parsed.samples[1].elapsed_seconds, Some(1.0));
    }

    #[test]
    fn mangohud_nanosecond_timestamps_apply_warmup_in_seconds() {
        let csv = "\
os,cpu,gpu,ram,kernel,driver,cpuscheduler
Linux,fixture,fixture,16,fixture,fixture,fixture
fps,frametime,cpu_load,elapsed
100,10,20,0
50,20,30,1000000000
25,40,40,31000000000
";
        let parsed = parse_mangohud_csv_measured(csv).unwrap();
        assert_eq!(
            parsed
                .samples
                .iter()
                .map(|sample| sample.elapsed_seconds)
                .collect::<Vec<_>>(),
            [Some(0.0), Some(1.0), Some(31.0)]
        );
        assert_eq!(parsed.summary.sample_count, 1);
        assert_eq!(parsed.summary.median_fps, Some(25.0));
        assert_eq!(parsed.summary.p99_frame_time_ms, Some(40.0));
        for column in ["time", "time_s", "elapsed_seconds"] {
            let parsed =
                parse_mangohud_csv_measured(&format!("{column},fps\n0,100\n1,50\n31,25\n"))
                    .unwrap();
            assert_eq!(parsed.summary.sample_count, 1);
            assert_eq!(parsed.samples[2].elapsed_seconds, Some(31.0));
        }
    }

    #[test]
    fn falls_back_to_fps_for_frame_time() {
        let csv = "time,fps\n0,100\n1,50\n";
        let parsed = parse_mangohud_csv(csv).unwrap();
        assert_eq!(parsed.samples[0].frame_time_ms, Some(10.0));
        assert_eq!(parsed.summary.p95_frame_time_ms, Some(20.0));
    }

    #[test]
    fn invalid_measured_frame_times_are_not_replaced_with_fps_estimates() {
        let csv = "time,fps,frametime\n0,60,\n1,60,NaN\n2,60,inf\n3,60,invalid\n4,60,0\n";
        let parsed = parse_mangohud_csv(csv).unwrap();
        assert!(
            parsed.samples[..4]
                .iter()
                .all(|sample| sample.frame_time_ms.is_none())
        );
        assert_eq!(parsed.summary.p99_frame_time_ms, None);
        let measured = parse_mangohud_csv_measured("time,fps\n0,60\n1,60\n").unwrap();
        assert!(
            measured
                .samples
                .iter()
                .all(|sample| sample.frame_time_ms.is_none())
        );
        assert_eq!(measured.summary.median_fps, Some(60.0));
        assert_eq!(measured.summary.p99_frame_time_ms, None);
    }

    #[test]
    fn rejects_missing_fps_header() {
        let err = parse_mangohud_csv("time,cpu\n0,1\n").unwrap_err();
        assert!(err.to_string().contains("no data header"));
    }
}
