#![allow(clippy::wildcard_imports)]
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use modde_core::db::NewPerformanceRun;
use modde_core::performance::{PerformanceModSnapshot, mod_snapshot};
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;
use modde_games::library::context::InstallationContext;

pub async fn handle_run(
    profile_name: Option<String>,
    game_id: String,
    duration: u64,
    label: Option<String>,
    warmup_seconds: f64,
    no_deploy: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let context = modde_games::library::context::for_game(
        &modde_core::settings::AppSettings::load(),
        &game_id,
        pm.db(),
    )
    .await?;
    let target = match profile_name.or(context.launch.profile.clone()) {
        Some(name) => name,
        None => {
            pm.active(&context.saves.scope)
                .await?
                .context("no active profile for this installation")?
                .profile
                .name
        }
    };
    run_installation(
        &pm,
        &context,
        &target,
        duration,
        label,
        warmup_seconds,
        no_deploy,
        None,
        None,
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Capture {
    pub run_id: String,
    pub directory: PathBuf,
    pub warmup_seconds: f64,
}

pub(super) fn require_baseline_configuration(
    run: &str,
    context: &InstallationContext,
) -> Result<()> {
    let path = modde_core::paths::modde_data_dir()
        .join("performance")
        .join(context.saves.game_id.as_str())
        .join(run)
        .join("configuration.json");
    let recorded: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).context(
        "baseline has no installation configuration; record a new baseline with modde perf run",
    )?)?;
    anyhow::ensure!(
        recorded["installation"]["id"].as_str() == Some(context.game.id.as_str())
            && recorded["save_scope"].as_str() == Some(context.saves.scope.as_str()),
        "baseline belongs to a different installation or save destination"
    );
    anyhow::ensure!(
        recorded["warmup_seconds"].as_f64()
            == Some(modde_core::performance::DEFAULT_WARMUP_SECONDS),
        "bisect baseline must use the default warmup so summaries and sample comparisons match"
    );
    let mut settings: modde_core::library::LaunchSettings =
        serde_json::from_value(recorded["settings"].clone())?;
    if let Some(enabled) = recorded["sandbox_override"].as_bool() {
        settings.sandbox.enabled = enabled;
    }
    settings.profile.clone_from(&context.launch.profile);
    settings.use_active_profile = context.launch.use_active_profile;
    anyhow::ensure!(
        settings == context.launch,
        "launch settings differ from the baseline; restore them or record a new baseline"
    );
    Ok(())
}

pub(super) async fn run_installation(
    pm: &ProfileManager,
    context: &InstallationContext,
    target: &str,
    duration: u64,
    label: Option<String>,
    warmup_seconds: f64,
    no_deploy: bool,
    sandbox: Option<bool>,
    bisect: Option<super::bisect::Completion>,
) -> Result<(String, bool)> {
    anyhow::ensure!(
        warmup_seconds.is_finite() && warmup_seconds >= 0.0 && duration as f64 > warmup_seconds,
        "duration must be positive and longer than the finite, nonnegative warmup"
    );
    let game = &context.saves.game_id;
    let profile = pm.load(target, Some(game)).await?;
    let run_id = new_run_id(game.as_str());
    pm.db()
        .create_performance_run(&NewPerformanceRun {
            run_id: run_id.clone(),
            game_id: game.clone(),
            profile_id: profile.id,
            profile_name: profile.name.clone(),
            mod_snapshot: mod_snapshot(&profile.mods),
            experiment_depth: pm.db().experiment_depth(&context.saves.scope).await?,
            label,
        })
        .await?;

    let perf_dir = modde_core::paths::modde_data_dir()
        .join("performance")
        .join(game.as_str())
        .join(&run_id);
    std::fs::create_dir_all(&perf_dir)
        .with_context(|| format!("failed to create {}", perf_dir.display()))?;
    let config_path = perf_dir.join("MangoHud.conf");
    write_mangohud_config(&config_path, &perf_dir, &run_id, duration)?;
    let capture = Capture {
        run_id: run_id.clone(),
        directory: perf_dir.clone(),
        warmup_seconds,
    };
    modde_core::library::atomic_json(
        &perf_dir.join("configuration.json"),
        &serde_json::json!({
            "installation": context.game, "save_scope": context.saves.scope, "settings": context.launch,
            "sandbox_override": sandbox, "duration": duration, "warmup_seconds": warmup_seconds,
            "modde_version": env!("CARGO_PKG_VERSION"), "revision": option_env!("MODDE_GIT_SHA"),
            "platform": std::env::consts::OS, "architecture": std::env::consts::ARCH,
            "kernel": std::fs::read_to_string("/proc/sys/kernel/osrelease").ok(),
            "nvidia_driver": std::fs::read_to_string("/proc/driver/nvidia/version").ok(),
        }),
    )?;
    println!("Performance run: {run_id}");
    pm.db()
        .mark_performance_run_pending(&run_id, Some(&perf_dir.join(format!("{run_id}.csv"))))
        .await?;
    let outcome = super::library::play(
        &context.game.id,
        super::library::PlayOptions {
            profile: Some(target.into()),
            no_deploy,
            sandbox,
            require_observed: true,
            expected_scope: Some(context.saves.scope.clone()),
            bisect,
            environment: BTreeMap::from([
                ("MANGOHUD".into(), "1".into()),
                (
                    "MANGOHUD_CONFIGFILE".into(),
                    config_path.to_string_lossy().into_owned(),
                ),
            ]),
            writable: vec![perf_dir],
            performance: Some(capture),
            ..Default::default()
        },
    )
    .await?;
    Ok((
        run_id,
        matches!(
            outcome,
            modde_games::library::launch::LaunchOutcome::Exited(_)
        ),
    ))
}

pub(super) async fn complete_capture(
    pm: &ProfileManager,
    capture: &Capture,
    status: std::process::ExitStatus,
) -> Result<()> {
    let saved = pm.db().load_performance_run(&capture.run_id).await?;
    if saved.status == "complete" {
        anyhow::ensure!(
            saved.exit_status == status.code().map(i64::from),
            "ingested performance exit status does not match the observed game"
        );
        measured_run_samples(pm.db(), &saved, capture.warmup_seconds).await?;
        return Ok(());
    }
    let csv = find_mangohud_csv(&capture.directory, &capture.run_id)
        .context("MangoHud did not produce a CSV; run remains pending for ingest")?;
    let parsed = parse_capture(&csv, capture.warmup_seconds)?;
    pm.db()
        .complete_performance_run(
            &capture.run_id,
            &csv,
            status.code().map(i64::from),
            &parsed.summary,
            &parsed.samples,
        )
        .await?;
    print_summary(&parsed.summary);
    Ok(())
}

pub async fn sandbox_pairs(
    id: &str,
    profile: &str,
    pairs: usize,
    duration: u64,
    warmup: f64,
) -> Result<()> {
    anyhow::ensure!(
        (2..=30).contains(&pairs),
        "use 2–30 pairs to measure between-run variation"
    );
    let pm = ProfileManager::open().await?;
    let games = modde_games::library::catalogue(&modde_core::settings::AppSettings::load())?.games;
    let game = games
        .iter()
        .find(|game| game.id == id)
        .context("installation is unavailable")?;
    let context = modde_games::library::context::for_installation(game, &games, pm.db()).await?;
    anyhow::ensure!(
        context.launch.executable.is_some(),
        "paired automation requires a direct executable; store runs can be captured and compared individually"
    );
    let report_path = modde_core::paths::modde_data_dir()
        .join("performance")
        .join(format!("sandbox-{}.json", new_run_id(id)));
    let mut results = Vec::new();
    let mut deltas = Vec::new();
    let mut startup_deltas = Vec::new();
    for pair in 0..pairs {
        let mut metrics = [None, None];
        let mut startup = [None, None];
        // Alternate AB/BA order to reduce monotonic temperature/cache bias.
        for enabled in if pair % 2 == 0 {
            [false, true]
        } else {
            [true, false]
        } {
            println!(
                "Pair {}/{pairs}: sandbox {}. Replay the same workload and exit the game.",
                pair + 1,
                if enabled { "on" } else { "off" }
            );
            let (run, complete) = run_installation(
                &pm,
                &context,
                profile,
                duration,
                Some(format!(
                    "sandbox pair {} {}",
                    pair + 1,
                    if enabled { "on" } else { "off" }
                )),
                warmup,
                false,
                Some(enabled),
                None,
            )
            .await?;
            anyhow::ensure!(complete, "paired run is still pending");
            let row = pm.db().load_performance_run(&run).await?;
            anyhow::ensure!(
                row.exit_status == Some(0),
                "benchmark run {run} failed; refusing to treat it as a performance sample"
            );
            let samples = pm.db().list_performance_samples(&run).await?;
            let p99 = benchmark_p99(&row.summary, &samples, warmup)
                .with_context(|| format!("run {run} cannot be used in a paired benchmark"))?;
            metrics[usize::from(enabled)] = Some(p99);
            let recorded: serde_json::Value = serde_json::from_slice(&std::fs::read(
                modde_core::paths::modde_data_dir()
                    .join("performance")
                    .join(context.saves.game_id.as_str())
                    .join(&run)
                    .join("session.json"),
            )?)?;
            let first_sample = recorded["process"]["first_sample_ms"].as_f64();
            startup[usize::from(enabled)] = first_sample;
            results.push(
                serde_json::json!({"pair": pair + 1, "sandbox": enabled, "run": run,
                "first_csv_sample_ms": first_sample,
                "p99_frame_time_ms": p99, "one_percent_low_fps": row.summary.one_percent_low_fps,
                "median_fps": row.summary.median_fps, "sample_count": row.summary.sample_count}),
            );
            modde_core::library::atomic_json(
                &report_path,
                &serde_json::json!({"complete": false, "runs": results}),
            )?;
        }
        let off = metrics[0].context("missing unsandboxed run")?;
        let on = metrics[1].context("missing sandboxed run")?;
        anyhow::ensure!(off > 0.0, "invalid zero baseline frame time");
        deltas.push((on / off - 1.0) * 100.0);
        if let (Some(off), Some(on)) = (startup[0], startup[1]) {
            startup_deltas.push(on - off);
        }
    }
    let mean = deltas.iter().sum::<f64>() / deltas.len() as f64;
    let sd = (deltas
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / (deltas.len() - 1) as f64)
        .sqrt();
    modde_core::library::atomic_json(
        &report_path,
        &serde_json::json!({
            "complete": true, "installation": id, "pairs": pairs, "runs": results,
            "paired_p99_percent_changes": deltas, "mean_percent_change": mean, "sample_stddev_percent_points": sd,
            "paired_first_sample_latency_changes_ms": startup_deltas,
            "startup_measure": "Launch to first parseable CSV sample, 100ms polling; not first displayed frame. Missing samples remain unavailable.",
            "interpretation": "Positive frame-time change is worse. Compare variation and reproduce the same workload; no universal overhead guarantee.",
        }),
    )?;
    println!(
        "Paired p99 change: {mean:+.2}% (SD {sd:.2} percentage points). Report: {}",
        report_path.display()
    );
    Ok(())
}

pub async fn handle_ingest(run_id: String, csv: PathBuf, warmup_seconds: f64) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let run = db.load_performance_run(&run_id).await?;
    let configuration = modde_core::paths::modde_data_dir()
        .join("performance")
        .join(run.game_id.as_str())
        .join(&run_id)
        .join("configuration.json");
    require_ingest_warmup(&configuration, warmup_seconds)?;
    let parsed = parse_capture(&csv, warmup_seconds)?;
    let exit_status = if let Some(session) = modde_core::library::PendingSession::load_blocking()? {
        session.require_owner()?;
        let options: super::library::PlayOptions = serde_json::from_value(
            session
                .launch_request
                .clone()
                .context("session has no performance capture")?,
        )?;
        let capture = options
            .performance
            .context("session has no performance capture")?;
        anyhow::ensure!(
            session.phase == modde_core::library::SessionPhase::Captured
                && capture.run_id == run_id,
            "finish the current game session before ingesting another run"
        );
        anyhow::ensure!(
            capture.warmup_seconds == warmup_seconds,
            "use the recorded warmup ({}) for this session's analysis",
            capture.warmup_seconds
        );
        super::library::observed_status(&session)?
            .and_then(|status| status.code())
            .map(i64::from)
    } else if run.exit_status.is_some() {
        run.exit_status
    } else {
        super::library::recorded_performance_status(
            configuration
                .parent()
                .context("capture directory missing")?,
            &run_id,
        )?
    };
    let csv = std::path::absolute(csv)?;
    db.complete_performance_run(&run_id, &csv, exit_status, &parsed.summary, &parsed.samples)
        .await?;
    println!("Ingested performance run: {run_id}");
    print_summary(&parsed.summary);
    Ok(())
}

/// An imported CSV cannot silently re-grade the capture's pre-warmup frames
/// while its provenance still advertises the original benchmark configuration.
fn require_ingest_warmup(configuration: &Path, warmup: f64) -> Result<()> {
    let bytes = match std::fs::read(configuration) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("reading performance capture configuration"),
    };
    let recorded: serde_json::Value =
        serde_json::from_slice(&bytes).context("invalid performance capture configuration")?;
    let expected = recorded["warmup_seconds"]
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0)
        .context("performance capture configuration has no valid recorded warmup")?;
    anyhow::ensure!(
        warmup == expected,
        "use the recorded warmup ({expected}) when ingesting this captured run"
    );
    Ok(())
}

fn parse_capture(path: &Path, warmup: f64) -> Result<modde_core::performance::MangoHudParseResult> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("reading performance capture {}", path.display()))?;
    let mut parsed = modde_core::performance::parse_mangohud_csv_measured(&content)
        .with_context(|| format!("parsing performance capture {}", path.display()))?;
    let remaining = capture_samples_after_warmup(&parsed.samples, warmup)?;
    parsed.summary = modde_core::performance::summarize_samples_with_warmup(&remaining, 0.0);
    Ok(parsed)
}

/// Stored values alone cannot distinguish a measurement from a legacy FPS
/// estimate. Reparse its retained CSV and require the exact ingested series and
/// summary before using a run for automatic grading.
pub(super) async fn measured_run_samples(
    db: &modde_core::ModdeDb,
    run: &modde_core::db::PerformanceRunRow,
    warmup: f64,
) -> Result<Vec<modde_core::PerformanceSample>> {
    anyhow::ensure!(
        run.status == "complete",
        "performance run {} is not complete",
        run.run_id
    );
    let csv = run
        .mangohud_csv_path
        .as_deref()
        .context("performance run has no retained CSV; record or ingest a new measured trace")?;
    let parsed = parse_capture(csv, warmup)?;
    let stored = db.list_performance_samples(&run.run_id).await?;
    anyhow::ensure!(
        parsed.summary == run.summary && parsed.samples == stored,
        "performance run {} differs from its measured CSV; ingest it again with the recorded warmup before grading",
        run.run_id
    );
    Ok(parsed.samples)
}

/// Capture summaries, paired benchmarks and bisects must use the same strict
/// timestamp boundary rather than the display summary's sparse-trace fallback.
pub(super) fn capture_samples_after_warmup(
    samples: &[modde_core::PerformanceSample],
    warmup: f64,
) -> Result<Vec<modde_core::PerformanceSample>> {
    anyhow::ensure!(
        warmup.is_finite() && warmup >= 0.0,
        "warmup must be finite and nonnegative"
    );
    if warmup == 0.0 {
        return Ok(samples.to_vec());
    }
    let start = samples.iter().filter_map(|sample| sample.elapsed_seconds)
        .filter(|time| time.is_finite()).min_by(f64::total_cmp)
        .context("CSV has no elapsed times; cannot apply warmup (use --warmup-seconds 0 for a pre-trimmed trace)")?;
    let remaining: Vec<_> = samples
        .iter()
        .filter(|sample| {
            sample
                .elapsed_seconds
                .is_some_and(|time| time.is_finite() && time - start >= warmup)
        })
        .cloned()
        .collect();
    anyhow::ensure!(
        !remaining.is_empty(),
        "CSV ended during warmup; no benchmark samples remain"
    );
    Ok(remaining)
}

fn benchmark_p99(
    summary: &modde_core::PerformanceSummary,
    samples: &[modde_core::PerformanceSample],
    warmup: f64,
) -> Result<f64> {
    let remaining = capture_samples_after_warmup(samples, warmup)?;
    let fps_count = remaining
        .iter()
        .filter(|sample| sample.fps.is_finite() && sample.fps > 0.0)
        .count();
    let frame_time_count = remaining
        .iter()
        .filter(|sample| {
            sample
                .frame_time_ms
                .is_some_and(|value| value.is_finite() && value > 0.0)
        })
        .count();
    anyhow::ensure!(
        summary.sample_count >= 100 && fps_count >= 100 && frame_time_count >= 100,
        "fewer than 100 usable post-warmup FPS or frame-time samples; collect a longer valid trace for tail percentiles"
    );
    summary
        .p99_frame_time_ms
        .filter(|value| value.is_finite() && *value > 0.0)
        .context("benchmark has no finite positive p99 frame time")
}

pub async fn handle_list(game_id: String, profile: Option<String>, limit: usize) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let rows = db
        .list_performance_runs(&GameId::from(game_id.as_str()), profile.as_deref(), limit)
        .await?;
    if rows.is_empty() {
        println!("No performance runs found.");
        return Ok(());
    }
    for row in rows {
        let one_low = format_opt(row.summary.one_percent_low_fps, " fps");
        println!(
            "{}  {}  {}  profile={}  1% low={}",
            row.run_id, row.started_at, row.status, row.profile_name, one_low
        );
        if let Some(label) = row.label {
            println!("  label: {label}");
        }
    }
    Ok(())
}

pub async fn handle_show(run_id: String) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let row = db.load_performance_run(&run_id).await?;
    println!("Run: {}", row.run_id);
    println!("Game: {}", row.game_id);
    println!("Profile: {}", row.profile_name);
    println!("Status: {}", row.status);
    println!("Started: {}", row.started_at);
    if let Some(path) = row.mangohud_csv_path {
        println!("CSV: {}", path.display());
    }
    if let Some(label) = row.label {
        println!("Label: {label}");
    }
    print_summary(&row.summary);
    Ok(())
}

pub async fn handle_compare(baseline: String, candidate: String) -> Result<()> {
    let db = modde_core::ModdeDb::open()
        .await
        .context("failed to open database")?;
    let baseline = db.load_performance_run(&baseline).await?;
    let candidate = db.load_performance_run(&candidate).await?;

    println!("Baseline:  {} ({})", baseline.run_id, baseline.profile_name);
    println!(
        "Candidate: {} ({})",
        candidate.run_id, candidate.profile_name
    );
    print_metric_delta(
        "Median FPS",
        baseline.summary.median_fps,
        candidate.summary.median_fps,
        "fps",
    );
    print_metric_delta(
        "Average FPS",
        baseline.summary.average_fps,
        candidate.summary.average_fps,
        "fps",
    );
    print_metric_delta(
        "1% low FPS",
        baseline.summary.one_percent_low_fps,
        candidate.summary.one_percent_low_fps,
        "fps",
    );
    print_metric_delta(
        "0.1% low FPS",
        baseline.summary.point_one_percent_low_fps,
        candidate.summary.point_one_percent_low_fps,
        "fps",
    );
    print_metric_delta(
        "p99 frame time",
        baseline.summary.p99_frame_time_ms,
        candidate.summary.p99_frame_time_ms,
        "ms",
    );
    print_mod_diff(&baseline.mod_snapshot_json, &candidate.mod_snapshot_json)?;
    Ok(())
}

fn write_mangohud_config(
    path: &Path,
    output_folder: &Path,
    run_id: &str,
    duration: u64,
) -> Result<()> {
    let content = format!(
        "\
# Generated by modde perf; do not edit.
no_display
fps
frametime
frame_timing
fps_metrics=0.01,0.001
benchmark_percentiles=97,AVG,1,0.1
autostart_log=1
log_duration={duration}
output_folder={}
output_file={run_id}
log_versioning=0
",
        output_folder.display()
    );
    std::fs::write(path, content)
        .with_context(|| format!("failed to write MangoHud config {}", path.display()))
}

fn find_mangohud_csv(dir: &Path, run_id: &str) -> Option<PathBuf> {
    let exact = dir.join(format!("{run_id}.csv"));
    if exact.is_file() {
        return Some(exact);
    }
    let mut candidates = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().and_then(|e| e.to_str()) == Some("csv")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|name| name.contains(run_id))
        })
        .collect::<Vec<_>>();
    candidates.sort();
    candidates.pop()
}

fn print_summary(summary: &modde_core::PerformanceSummary) {
    println!("Samples: {}", summary.sample_count);
    println!("Median FPS: {}", format_opt(summary.median_fps, " fps"));
    println!("Average FPS: {}", format_opt(summary.average_fps, " fps"));
    println!(
        "1% low FPS: {}",
        format_opt(summary.one_percent_low_fps, " fps")
    );
    println!(
        "0.1% low FPS: {}",
        format_opt(summary.point_one_percent_low_fps, " fps")
    );
    println!(
        "p99 frame time: {}",
        format_opt(summary.p99_frame_time_ms, " ms")
    );
}

fn print_metric_delta(name: &str, baseline: Option<f64>, candidate: Option<f64>, unit: &str) {
    match (baseline, candidate) {
        (Some(b), Some(c)) if b != 0.0 => {
            let delta = c - b;
            let pct = delta / b * 100.0;
            println!("{name}: {b:.2} -> {c:.2} {unit} ({delta:+.2}, {pct:+.1}%)");
        }
        (Some(b), Some(c)) => println!("{name}: {b:.2} -> {c:.2} {unit}"),
        _ => println!("{name}: unavailable"),
    }
}

fn print_mod_diff(baseline_json: &str, candidate_json: &str) -> Result<()> {
    let baseline = decode_mods(baseline_json)?;
    let candidate = decode_mods(candidate_json)?;
    let base_ids: BTreeSet<_> = baseline.keys().cloned().collect();
    let cand_ids: BTreeSet<_> = candidate.keys().cloned().collect();

    let added: Vec<_> = cand_ids.difference(&base_ids).cloned().collect();
    let removed: Vec<_> = base_ids.difference(&cand_ids).cloned().collect();
    let changed: Vec<_> = base_ids
        .intersection(&cand_ids)
        .filter(|id| baseline.get(*id) != candidate.get(*id))
        .cloned()
        .collect();

    println!("Mod changes:");
    print_mod_list("Added", &added, &candidate);
    print_mod_list("Removed", &removed, &baseline);
    if changed.is_empty() {
        println!("  Changed: none");
    } else {
        println!("  Changed:");
        for id in changed {
            println!(
                "    {}: {} -> {}",
                id,
                baseline
                    .get(&id)
                    .and_then(|v| v.as_deref())
                    .unwrap_or("(none)"),
                candidate
                    .get(&id)
                    .and_then(|v| v.as_deref())
                    .unwrap_or("(none)")
            );
        }
    }
    Ok(())
}

fn decode_mods(json: &str) -> Result<BTreeMap<String, Option<String>>> {
    let mods: Vec<PerformanceModSnapshot> = serde_json::from_str(json)
        .with_context(|| "stored performance mod snapshot is invalid JSON")?;
    Ok(mods
        .into_iter()
        .map(|m| (m.mod_id, m.version))
        .collect::<BTreeMap<_, _>>())
}

fn print_mod_list(label: &str, ids: &[String], mods: &BTreeMap<String, Option<String>>) {
    if ids.is_empty() {
        println!("  {label}: none");
    } else {
        println!("  {label}:");
        for id in ids {
            match mods.get(id).and_then(|v| v.as_deref()) {
                Some(version) => println!("    {id} ({version})"),
                None => println!("    {id}"),
            }
        }
    }
}

fn format_opt(value: Option<f64>, suffix: &str) -> String {
    value
        .map(|v| format!("{v:.2}{suffix}"))
        .unwrap_or_else(|| "unavailable".to_string())
}

fn new_run_id(game_id: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let normalized = game_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>();
    format!("{normalized}-{nanos:x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_does_not_turn_an_all_warmup_trace_into_a_measurement() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("run.csv");
        std::fs::write(
            &csv,
            "fps,frametime,elapsed\n60,16.67,0\n60,16.67,1000000000\n",
        )
        .unwrap();
        assert!(parse_capture(&csv, 30.0).is_err());
        assert_eq!(parse_capture(&csv, 0.0).unwrap().summary.sample_count, 2);
        assert!(parse_capture(&csv, f64::NAN).is_err());
    }

    #[test]
    fn paired_benchmark_requires_usable_frame_times_after_warmup() {
        let dir = tempfile::tempdir().unwrap();
        let csv = dir.path().join("run.csv");
        let mut content = "fps,frametime,elapsed\n".to_string();
        for seconds in 0_u64..130 {
            let elapsed = seconds * 1_000_000_000;
            let frame_time = if seconds < 32 { 16.67 } else { -1.0 };
            content.push_str(&format!("60,{frame_time},{elapsed}\n"));
        }
        std::fs::write(&csv, content).unwrap();
        let parsed = parse_capture(&csv, 30.0).unwrap();
        assert_eq!(parsed.summary.sample_count, 100);
        assert_eq!(parsed.summary.p99_frame_time_ms, Some(16.67));
        assert!(benchmark_p99(&parsed.summary, &parsed.samples, 30.0).is_err());

        let valid: Vec<_> = parsed
            .samples
            .iter()
            .cloned()
            .map(|mut sample| {
                sample.frame_time_ms = Some(16.67);
                sample
            })
            .collect();
        assert_eq!(benchmark_p99(&parsed.summary, &valid, 30.0).unwrap(), 16.67);
        assert!(benchmark_p99(&parsed.summary, &valid[..129], 30.0).is_err());
        let mut invalid_summary = parsed.summary;
        invalid_summary.p99_frame_time_ms = Some(0.0);
        assert!(benchmark_p99(&invalid_summary, &valid, 30.0).is_err());
    }

    #[test]
    fn paired_benchmark_cannot_invent_missing_frame_times_from_fps() {
        let root = tempfile::tempdir().unwrap();
        let csv = root.path().join("capture.csv");
        for missing in ["", "NaN", "inf", "invalid", "no-column"] {
            let mut content = if missing == "no-column" {
                "fps,elapsed\n"
            } else {
                "fps,frametime,elapsed\n"
            }
            .to_string();
            for seconds in 0_u64..130 {
                let elapsed = seconds * 1_000_000_000;
                if missing == "no-column" {
                    content.push_str(&format!("60,{elapsed}\n"));
                } else {
                    content.push_str(&format!("60,{missing},{elapsed}\n"));
                }
            }
            std::fs::write(&csv, content).unwrap();
            let parsed = parse_capture(&csv, 30.0).unwrap();
            assert_eq!(parsed.summary.sample_count, 100);
            assert!(
                parsed
                    .samples
                    .iter()
                    .all(|sample| sample.frame_time_ms.is_none()),
                "{missing}"
            );
            assert!(
                benchmark_p99(&parsed.summary, &parsed.samples, 30.0).is_err(),
                "{missing}"
            );
        }
    }

    #[test]
    fn paired_capture_discards_thirty_seconds_of_mangohud_nanosecond_rows() {
        let root = tempfile::tempdir().unwrap();
        let csv = root.path().join("capture.csv");
        let mut content = "fps,frametime,elapsed\n".to_string();
        for seconds in 0_u64..130 {
            let elapsed = seconds * 1_000_000_000;
            let (fps, frame_time) = if seconds < 30 {
                (10, 100.0)
            } else {
                (60, 16.67)
            };
            content.push_str(&format!("{fps},{frame_time},{elapsed}\n"));
        }
        std::fs::write(&csv, content).unwrap();
        let parsed = parse_capture(&csv, 30.0).unwrap();
        assert_eq!(parsed.summary.sample_count, 100);
        assert_eq!(parsed.summary.one_percent_low_fps, Some(60.0));
        assert_eq!(parsed.samples[30].elapsed_seconds, Some(30.0));
        assert_eq!(
            benchmark_p99(&parsed.summary, &parsed.samples, 30.0).unwrap(),
            16.67
        );
        assert!(parse_capture(&csv, 130.0).is_err());
    }

    #[test]
    fn manual_ingestion_cannot_change_a_captured_runs_warmup() {
        let root = tempfile::tempdir().unwrap();
        let configuration = root.path().join("configuration.json");
        std::fs::write(&configuration, r#"{"warmup_seconds":30.0}"#).unwrap();
        assert!(require_ingest_warmup(&configuration, 30.0).is_ok());
        assert!(require_ingest_warmup(&configuration, 0.0).is_err());
        std::fs::write(&configuration, "broken").unwrap();
        assert!(require_ingest_warmup(&configuration, 30.0).is_err());
        std::fs::write(&configuration, "{}").unwrap();
        assert!(require_ingest_warmup(&configuration, 30.0).is_err());
        std::fs::remove_file(&configuration).unwrap();
        // Legacy manual runs without capture provenance keep explicit warmup.
        assert!(require_ingest_warmup(&configuration, 0.0).is_ok());
    }

    #[tokio::test]
    async fn grading_rechecks_measured_csv_provenance_and_stored_samples() {
        let db = modde_core::ModdeDb::open_memory().await.unwrap();
        let root = tempfile::tempdir().unwrap();
        let csv = root.path().join("capture.csv");
        db.create_performance_run(&modde_core::db::NewPerformanceRun {
            run_id: "baseline".into(),
            game_id: "example".into(),
            profile_id: None,
            profile_name: "fixture".into(),
            mod_snapshot: Vec::new(),
            experiment_depth: 0,
            label: None,
        })
        .await
        .unwrap();
        let legacy = "time,fps\n0,60\n31,60\n32,60\n";
        std::fs::write(&csv, legacy).unwrap();
        let estimated = modde_core::performance::parse_mangohud_csv(legacy).unwrap();
        db.complete_performance_run(
            "baseline",
            &csv,
            Some(0),
            &estimated.summary,
            &estimated.samples,
        )
        .await
        .unwrap();
        let row = db.load_performance_run("baseline").await.unwrap();
        assert!(measured_run_samples(&db, &row, 30.0).await.is_err());

        std::fs::write(
            &csv,
            "time,fps,frametime\n0,60,16.67\n31,60,16.67\n32,60,16.67\n",
        )
        .unwrap();
        let measured = parse_capture(&csv, 30.0).unwrap();
        db.complete_performance_run(
            "baseline",
            &csv,
            Some(0),
            &measured.summary,
            &measured.samples,
        )
        .await
        .unwrap();
        let row = db.load_performance_run("baseline").await.unwrap();
        assert_eq!(
            measured_run_samples(&db, &row, 30.0).await.unwrap(),
            measured.samples
        );
        // Same summaries do not suffice: a changed warmup row must be detected.
        std::fs::write(
            &csv,
            "time,fps,frametime\n0,10,100\n31,60,16.67\n32,60,16.67\n",
        )
        .unwrap();
        assert_eq!(parse_capture(&csv, 30.0).unwrap().summary, row.summary);
        assert!(measured_run_samples(&db, &row, 30.0).await.is_err());
        std::fs::remove_file(&csv).unwrap();
        assert!(measured_run_samples(&db, &row, 30.0).await.is_err());
    }

    #[test]
    fn mod_diff_decodes_snapshots() {
        let mods = vec![PerformanceModSnapshot {
            mod_id: "a".into(),
            version: Some("1".into()),
            enabled: true,
        }];
        let json = serde_json::to_string(&mods).unwrap();
        let decoded = decode_mods(&json).unwrap();
        assert_eq!(decoded.get("a").and_then(|v| v.as_deref()), Some("1"));
    }

    #[test]
    fn mod_hash_is_order_independent() {
        let mut a = vec![
            PerformanceModSnapshot {
                mod_id: "b".into(),
                version: None,
                enabled: true,
            },
            PerformanceModSnapshot {
                mod_id: "a".into(),
                version: Some("1".into()),
                enabled: true,
            },
        ];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(
            modde_core::performance::mod_set_hash(&a),
            modde_core::performance::mod_set_hash(&b)
        );
        a[0].version = Some("2".into());
        assert_ne!(
            modde_core::performance::mod_set_hash(&a),
            modde_core::performance::mod_set_hash(&b)
        );
    }
}
