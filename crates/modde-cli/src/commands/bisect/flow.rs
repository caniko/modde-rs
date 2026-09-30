//! Bisect command flow and session state transitions.

use std::time::{Duration, UNIX_EPOCH};

use anyhow::{Context, Result};

use modde_core::bisect::next_candidate_with_dependencies;
use modde_core::profile::ProfileManager;
use modde_core::{
    BisectOracle, BisectResult, BisectSession, BisectStatus, BisectStep, NewBisectStep,
};

use super::BisectResultArg;
use super::candidate::{
    analyze_crash_log, cleanup_candidates, complete_and_advance, create_candidate_profile,
    enforce_save_safety, finish_without_candidate, launch_candidate, newest_file_after,
    print_completion_if_any, restore_source_profile,
};
use super::perf::{PerfRegressionConfig, perf_regression_verdict, run_perf_candidate};
use super::start::bisect_dependency_map;

pub async fn handle_run(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    ensure_runnable(&session)?;
    if session.current_step_id.is_some() {
        anyhow::bail!(
            "session '{session_id}' already has a pending candidate; mark it with: modde bisect mark {session_id} good|bad"
        );
    }

    let source = pm
        .load(&session.source_profile_name, Some(&session.game_id))
        .await?;
    let dependencies = bisect_dependency_map(&pm, &source).await?;
    let Some(plan) = next_candidate_with_dependencies(&session, &dependencies) else {
        finish_without_candidate(&pm, &session).await?;
        return Ok(());
    };
    let step_index = pm.db().list_bisect_steps(&session_id).await?.len() + 1;
    let candidate_name = format!("__bisect_{}_{}", session.session_id, step_index);
    let candidate =
        create_candidate_profile(&pm, &source, &candidate_name, &plan.disabled_mod_ids).await?;
    enforce_save_safety(&session, &source, &candidate)?;

    let step_id = pm
        .db()
        .create_bisect_step(&NewBisectStep {
            session_id: session_id.clone(),
            step_index,
            candidate_profile: candidate_name.clone(),
            candidate_mod_ids: plan.candidate_mod_ids,
            enabled_mod_ids: plan.enabled_mod_ids,
            disabled_mod_ids: plan.disabled_mod_ids,
        })
        .await?;
    pm.db()
        .update_bisect_session_state(
            &session_id,
            BisectStatus::Waiting,
            &session.suspect_mod_ids,
            &session.known_good_mod_ids,
            &session.known_bad_mod_ids,
            Some(step_id),
            Some(&candidate_name),
        )
        .await?;

    launch_step(&pm, &session, &candidate, step_id).await
}

async fn launch_step(pm: &ProfileManager, session: &BisectSession, candidate: &modde_core::profile::Profile, step_id: i64) -> Result<()> {
    let pending = if matches!(session.oracle, BisectOracle::Perf { .. }) {
        run_perf_candidate(pm, candidate, session, step_id).await?.is_none()
    } else {
        launch_candidate(pm, &candidate.name, session, false).await?.is_none()
    };
    if pending {
        println!("Candidate requested in the store: {}. Its wrapper will complete capture and evaluate the selected oracle.", candidate.name);
    }
    Ok(())
}

pub(crate) async fn complete_launch(
    pm: &ProfileManager, completion: &super::Completion, status: std::process::ExitStatus, performance_run: Option<&str>,
) -> Result<()> {
    let session_id = completion.session.clone();
    let step_id = completion.step;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let step = pm.db().load_bisect_step(step_id).await?;
    anyhow::ensure!(step.session_id == session_id, "bisect completion belongs to another session");
    if matches!(session.status, BisectStatus::Complete | BisectStatus::Inconclusive)
        && step.result.is_some()
        && pm.db().list_bisect_steps(&session_id).await?.last().is_some_and(|last| last.id == step_id) {
        // The result committed before source restoration/cleanup. Resume those
        // operations after a crash rather than silently dropping the receipt.
        restore_source_profile(pm, &session).await?;
        cleanup_candidates(pm, &session).await?;
        return Ok(());
    }
    // A delayed completion must not grade a newer candidate or advance twice.
    if session.current_step_id != Some(step_id) || session.status != BisectStatus::Waiting { return Ok(()); }
    anyhow::ensure!(session.current_candidate_profile.as_deref() == Some(step.candidate_profile.as_str()), "bisect candidate changed during play");
    if let Some(result) = step.result {
        return complete_and_advance(pm, session_id, step_id, result, step.observed_signal, step.notes).await;
    }
    let candidate = pm.load(&step.candidate_profile, Some(&session.game_id)).await?;
    match session.oracle.clone() {
        BisectOracle::Manual => {
            println!("Mark result with: modde bisect mark {session_id} good|bad");
        }
        BisectOracle::Crash { crash_dir } => {
            let started = UNIX_EPOCH + Duration::from_millis(completion.started_unix_ms.context("bisect launch time is unavailable; mark this candidate manually")?);
            let new_log = newest_file_after(&crash_dir, started)?;
            let (result, signal) = if let Some(log) = new_log {
                analyze_crash_log(pm, &candidate, &log).await?;
                (
                    BisectResult::Bad,
                    Some(format!("new crash log: {}", log.display())),
                )
            } else if !status.success() {
                (BisectResult::Bad, Some(format!("game process failed: {status}")))
            } else {
                (
                    BisectResult::Good,
                    Some("no new crash log observed".to_string()),
                )
            };
            complete_and_advance(pm, session_id, step_id, result, signal, None).await?;
        }
        BisectOracle::Perf {
            baseline_run,
            p99_frame_time_percent,
            one_percent_low_fps_percent,
            alpha_micros,
            min_samples,
        } => {
            anyhow::ensure!(status.success(), "failed game run cannot be graded as a performance improvement; retry or mark manually");
            let run_id = performance_run.context("performance capture is unavailable; retry or mark this candidate manually")?;
            let baseline = pm.db().load_performance_run(&baseline_run).await?;
            let candidate_run = pm.db().load_performance_run(run_id).await?;
            anyhow::ensure!(baseline.status == "complete" && baseline.exit_status == Some(0) && candidate_run.status == "complete"
                && baseline.summary.sample_count >= min_samples && candidate_run.summary.sample_count >= min_samples,
                "insufficient completed performance samples; retry or mark this candidate manually");
            let baseline_samples = pm.db().list_performance_samples(&baseline_run).await?;
            let candidate_samples = pm.db().list_performance_samples(run_id).await?;
            let verdict = perf_regression_verdict(
                &baseline.summary,
                &candidate_run.summary,
                &baseline_samples,
                &candidate_samples,
                PerfRegressionConfig {
                    p99_frame_time_ratio: f64::from(p99_frame_time_percent) / 100.0,
                    one_percent_low_fps_ratio: f64::from(one_percent_low_fps_percent) / 100.0,
                    alpha: f64::from(alpha_micros) / 1_000_000.0,
                    min_samples,
                },
            )?;
            complete_and_advance(
                pm,
                session_id,
                step_id,
                if verdict.regressed {
                    BisectResult::Bad
                } else {
                    BisectResult::Good
                },
                Some(format!("performance run: {run_id}; {}", verdict.signal)),
                None,
            )
            .await?;
        }
    }
    Ok(())
}

pub async fn handle_mark(
    session_id: String,
    result: BisectResultArg,
    notes: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let step_id = session
        .current_step_id
        .ok_or_else(|| anyhow::anyhow!("session '{session_id}' has no pending candidate"))?;
    complete_and_advance(
        &pm,
        session_id,
        step_id,
        result.into(),
        Some("manual mark".to_string()),
        notes,
    )
    .await
}

pub async fn handle_status(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let steps = pm.db().list_bisect_steps(&session_id).await?;
    println!("Bisect session: {}", session.session_id);
    println!("Game: {}", session.game_id);
    println!("Source profile: {}", session.source_profile_name);
    println!("Oracle: {}", session.oracle.as_str());
    println!("Status: {}", session.status.as_str());
    println!("Suspects: {}", session.suspect_mod_ids.len());
    if let Some(candidate) = &session.current_candidate_profile {
        println!("Current candidate: {candidate}");
    }
    for step in steps {
        let result = step.result.map(BisectResult::as_str).unwrap_or("pending");
        println!(
            "  #{} {} result={} disabled={}",
            step.step_index,
            step.candidate_profile,
            result,
            step.disabled_mod_ids.join(", ")
        );
    }
    print_completion_if_any(&pm, &session).await
}

pub async fn handle_history(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let steps = pm.db().list_bisect_steps(&session_id).await?;
    print!("{}", render_history(&session, &steps));
    Ok(())
}

/// Render the `bisect history` report. Extracted from [`handle_history`] so
/// the output can be asserted in tests without a real profile database.
pub(super) fn render_history(session: &BisectSession, steps: &[BisectStep]) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    let _ = writeln!(out, "Bisect session: {}", session.session_id);
    let _ = writeln!(out, "Status: {}", session.status.as_str());
    let _ = writeln!(
        out,
        "Current suspects: {}",
        session.suspect_mod_ids.join(", ")
    );
    for step in steps {
        let result = step.result.map(BisectResult::as_str).unwrap_or("pending");
        let _ = writeln!(
            out,
            "#{} {} result={result}",
            step.step_index, step.candidate_profile
        );
        let _ = writeln!(
            out,
            "  candidate disabled: {}",
            step.candidate_mod_ids.join(", ")
        );
        let _ = writeln!(out, "  enabled half: {}", step.enabled_mod_ids.join(", "));
        let _ = writeln!(out, "  disabled half: {}", step.disabled_mod_ids.join(", "));
        if let Some(signal) = &step.observed_signal {
            let _ = writeln!(out, "  signal: {signal}");
        }
        if let Some(notes) = &step.notes {
            let _ = writeln!(out, "  notes: {notes}");
        }
    }
    out
}

pub async fn handle_retry(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    let candidate_name = validate_retry(&session)?;
    let candidate = pm.load(candidate_name, Some(&session.game_id)).await?;
    launch_step(&pm, &session, &candidate, session.current_step_id.context("bisect step missing")?).await
}

/// Validate that `session` has a pending candidate that can be retried and
/// return its profile name. Extracted from [`handle_retry`] so the
/// error/ok boundary can be tested without launching a game.
pub(super) fn validate_retry(session: &BisectSession) -> Result<&str> {
    let candidate = session
        .current_candidate_profile
        .as_deref()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "session '{}' has no pending candidate to retry",
                session.session_id
            )
        })?;
    if session.current_step_id.is_none() || session.status != BisectStatus::Waiting {
        anyhow::bail!(
            "session '{}' is not waiting on a pending candidate (status: {})",
            session.session_id,
            session.status.as_str()
        );
    }
    Ok(candidate)
}

pub async fn handle_abort(session_id: String) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let session = pm.db().load_bisect_session(&session_id).await?;
    restore_source_profile(&pm, &session).await?;
    cleanup_candidates(&pm, &session).await?;
    pm.db()
        .update_bisect_session_state(
            &session_id,
            BisectStatus::Aborted,
            &session.suspect_mod_ids,
            &session.known_good_mod_ids,
            &session.known_bad_mod_ids,
            None,
            None,
        )
        .await?;
    println!("Aborted bisect session: {session_id}");
    Ok(())
}

pub(super) fn ensure_runnable(session: &BisectSession) -> Result<()> {
    match session.status {
        BisectStatus::Active => Ok(()),
        BisectStatus::Waiting => anyhow::bail!(
            "session '{}' is waiting for a result; run: modde bisect mark {} good|bad",
            session.session_id,
            session.session_id
        ),
        _ => anyhow::bail!(
            "session '{}' is not active (status: {})",
            session.session_id,
            session.status.as_str()
        ),
    }
}
