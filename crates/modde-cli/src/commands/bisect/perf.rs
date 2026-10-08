//! Performance oracle and statistical regression helpers.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use modde_core::performance::DEFAULT_WARMUP_SECONDS;
use modde_core::profile::{Profile, ProfileManager};
use modde_core::{BisectSession, PerformanceSample, PerformanceSummary};

pub(super) async fn run_perf_candidate(
    pm: &ProfileManager,
    profile: &Profile,
    session: &BisectSession,
    step_id: i64,
) -> Result<Option<String>> {
    let context = super::candidate::installation(pm, session).await?;
    if let modde_core::BisectOracle::Perf { baseline_run, .. } = &session.oracle {
        let baseline = pm.db().load_performance_run(baseline_run).await?;
        anyhow::ensure!(
            baseline.status == "complete" && baseline.exit_status == Some(0),
            "baseline must retain a completed, successful observed exit; record a new baseline"
        );
        let source = pm
            .load(&session.source_profile_name, Some(&session.game_id))
            .await?;
        require_baseline_profile(&baseline, &source)?;
        crate::commands::perf::require_baseline_configuration(baseline_run, &context)?;
        crate::commands::perf::measured_run_samples(pm.db(), &baseline, DEFAULT_WARMUP_SECONDS)
            .await?;
    }
    let (run, complete) = crate::commands::perf::run_installation(
        pm,
        &context,
        &profile.name,
        300,
        Some(format!("bisect step {step_id}")),
        DEFAULT_WARMUP_SECONDS,
        false,
        None,
        Some(super::Completion {
            session: session.session_id.clone(),
            step: step_id,
            started_unix_ms: None,
        }),
    )
    .await?;
    Ok(complete.then_some(run))
}

pub(super) fn require_baseline_profile(
    baseline: &modde_core::db::PerformanceRunRow,
    profile: &Profile,
) -> Result<()> {
    anyhow::ensure!(
        profile.id.is_some()
            && baseline.profile_id == profile.id
            && baseline.game_id == profile.game_id,
        "baseline belongs to another source profile; record a new baseline for this profile"
    );
    let recorded: Vec<modde_core::performance::PerformanceModSnapshot> =
        serde_json::from_str(&baseline.mod_snapshot_json)?;
    anyhow::ensure!(
        recorded == modde_core::performance::mod_snapshot(&profile.mods),
        "source profile's enabled mod order or versions differ from the baseline; record a new baseline"
    );
    Ok(())
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PerfRegressionConfig {
    pub(super) p99_frame_time_ratio: f64,
    pub(super) one_percent_low_fps_ratio: f64,
    pub(super) alpha: f64,
    pub(super) min_samples: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PerfRegressionVerdict {
    pub(super) regressed: bool,
    pub(super) signal: String,
}

pub(super) fn perf_regression_verdict(
    baseline: &PerformanceSummary,
    candidate: &PerformanceSummary,
    baseline_samples: &[PerformanceSample],
    candidate_samples: &[PerformanceSample],
    config: PerfRegressionConfig,
) -> Result<PerfRegressionVerdict> {
    let baseline_filtered = crate::commands::perf::capture_samples_after_warmup(
        baseline_samples,
        DEFAULT_WARMUP_SECONDS,
    )?;
    let candidate_filtered = crate::commands::perf::capture_samples_after_warmup(
        candidate_samples,
        DEFAULT_WARMUP_SECONDS,
    )?;
    let baseline_fps = fps_values(&baseline_filtered);
    let candidate_fps = fps_values(&candidate_filtered);
    let baseline_frame_times = frame_time_values(&baseline_filtered);
    let candidate_frame_times = frame_time_values(&candidate_filtered);

    // A missing timestamp/metric is not evidence of an improvement. Check the
    // actual post-warmup series as well as the stored summary before grading.
    let minimum = config.min_samples.max(2);
    anyhow::ensure!(
        [
            baseline_fps.len(),
            candidate_fps.len(),
            baseline_frame_times.len(),
            candidate_frame_times.len()
        ]
        .into_iter()
        .all(|count| count >= minimum),
        "insufficient usable post-warmup samples; retry or mark this candidate manually"
    );
    anyhow::ensure!(
        [
            baseline.p99_frame_time_ms,
            candidate.p99_frame_time_ms,
            baseline.one_percent_low_fps,
            candidate.one_percent_low_fps
        ]
        .into_iter()
        .all(|metric| metric.is_some_and(|value| value.is_finite() && value > 0.0)),
        "performance summary has missing or invalid metrics; retry or mark this candidate manually"
    );

    let p99_regressed = baseline
        .p99_frame_time_ms
        .zip(candidate.p99_frame_time_ms)
        .is_some_and(|(base, cand)| {
            cand >= base * config.p99_frame_time_ratio
                && variance_gate_regressed(
                    &baseline_frame_times,
                    &candidate_frame_times,
                    Direction::HigherIsWorse,
                    config,
                )
        });
    let low_regressed = baseline
        .one_percent_low_fps
        .zip(candidate.one_percent_low_fps)
        .is_some_and(|(base, cand)| {
            cand <= base * config.one_percent_low_fps_ratio
                && variance_gate_regressed(
                    &baseline_fps,
                    &candidate_fps,
                    Direction::LowerIsWorse,
                    config,
                )
        });

    let signal = format!(
        "p99={} -> {}; 1% low={} -> {}; samples={} -> {}",
        format_metric(baseline.p99_frame_time_ms, "ms"),
        format_metric(candidate.p99_frame_time_ms, "ms"),
        format_metric(baseline.one_percent_low_fps, "fps"),
        format_metric(candidate.one_percent_low_fps, "fps"),
        baseline_fps.len(),
        candidate_fps.len()
    );
    Ok(PerfRegressionVerdict {
        regressed: p99_regressed || low_regressed,
        signal,
    })
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    HigherIsWorse,
    LowerIsWorse,
}

fn fps_values(samples: &[PerformanceSample]) -> Vec<f64> {
    samples
        .iter()
        .map(|sample| sample.fps)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect()
}

fn frame_time_values(samples: &[PerformanceSample]) -> Vec<f64> {
    samples
        .iter()
        .filter_map(|sample| sample.frame_time_ms)
        .filter(|value| value.is_finite() && *value > 0.0)
        .collect()
}

fn variance_gate_regressed(
    baseline: &[f64],
    candidate: &[f64],
    direction: Direction,
    config: PerfRegressionConfig,
) -> bool {
    if baseline.len() < config.min_samples.max(2) || candidate.len() < config.min_samples.max(2) {
        return false;
    }
    welch_significant(baseline, candidate, direction, config.alpha)
}

fn welch_significant(
    baseline: &[f64],
    candidate: &[f64],
    direction: Direction,
    alpha: f64,
) -> bool {
    let Some(base_mean) = mean(baseline) else {
        return false;
    };
    let Some(candidate_mean) = mean(candidate) else {
        return false;
    };
    let base_var = sample_variance(baseline).unwrap_or(0.0);
    let candidate_var = sample_variance(candidate).unwrap_or(0.0);
    let standard_error =
        ((base_var / baseline.len() as f64) + (candidate_var / candidate.len() as f64)).sqrt();
    if !standard_error.is_finite() || standard_error <= f64::EPSILON {
        return match direction {
            Direction::HigherIsWorse => candidate_mean > base_mean,
            Direction::LowerIsWorse => candidate_mean < base_mean,
        };
    }
    let t = match direction {
        Direction::HigherIsWorse => (candidate_mean - base_mean) / standard_error,
        Direction::LowerIsWorse => (base_mean - candidate_mean) / standard_error,
    };
    t.is_finite() && t >= normal_critical_one_sided(alpha)
}

fn normal_critical_one_sided(alpha: f64) -> f64 {
    if alpha <= 0.001 {
        3.09
    } else if alpha <= 0.01 {
        2.33
    } else if alpha <= 0.025 {
        1.96
    } else if alpha <= 0.05 {
        1.645
    } else if alpha <= 0.10 {
        1.282
    } else {
        1.0
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

fn sample_variance(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let avg = mean(values)?;
    Some(
        values
            .iter()
            .map(|value| (value - avg).powi(2))
            .sum::<f64>()
            / (values.len() - 1) as f64,
    )
}

fn format_metric(value: Option<f64>, unit: &str) -> String {
    value.map_or_else(|| "n/a".to_string(), |value| format!("{value:.2} {unit}"))
}

pub(super) fn time_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{:09}", now.as_secs(), now.subsec_nanos())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warmup_excludes_missing_nonfinite_and_early_timestamps() {
        let samples: Vec<_> = [
            Some(0.0),
            None,
            Some(f64::NAN),
            Some(29.0),
            Some(30.0),
            Some(31.0),
        ]
        .into_iter()
        .map(|elapsed_seconds| PerformanceSample {
            elapsed_seconds,
            fps: 60.0,
            frame_time_ms: Some(16.67),
            cpu_load: None,
            gpu_load: None,
        })
        .collect();
        let filtered =
            crate::commands::perf::capture_samples_after_warmup(&samples, DEFAULT_WARMUP_SECONDS)
                .unwrap();
        assert_eq!(
            filtered
                .iter()
                .map(|sample| sample.elapsed_seconds)
                .collect::<Vec<_>>(),
            [Some(30.0), Some(31.0)]
        );
        assert!(
            crate::commands::perf::capture_samples_after_warmup(
                &samples[1..3],
                DEFAULT_WARMUP_SECONDS
            )
            .is_err()
        );
    }
}
