#![allow(clippy::wildcard_imports)]
use super::*;
use std::path::PathBuf;

use modde_core::PerformanceSample;
use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::{
    BisectOracle, BisectSaveSafety, BisectStatus, GameId, NewBisectSession, NewBisectStep,
};

use super::flow::{render_history, validate_retry};
use super::perf::{PerfRegressionConfig, PerfRegressionVerdict, perf_regression_verdict};

fn config() -> PerfRegressionConfig {
    PerfRegressionConfig {
        p99_frame_time_ratio: 1.15,
        one_percent_low_fps_ratio: 0.85,
        alpha: 0.05,
        min_samples: 30,
    }
}

fn samples(values: &[f64]) -> Vec<PerformanceSample> {
    values
        .iter()
        .enumerate()
        .map(|(idx, fps)| PerformanceSample {
            elapsed_seconds: Some(31.0 + idx as f64),
            fps: *fps,
            frame_time_ms: Some(1000.0 / fps),
            cpu_load: None,
            gpu_load: None,
        })
        .collect()
}

fn verdict(base: &[f64], candidate: &[f64]) -> PerfRegressionVerdict {
    let base_samples = samples(base);
    let candidate_samples = samples(candidate);
    let base_summary = modde_core::performance::summarize_samples_with_warmup(&base_samples, 0.0);
    let candidate_summary =
        modde_core::performance::summarize_samples_with_warmup(&candidate_samples, 0.0);
    perf_regression_verdict(
        &base_summary,
        &candidate_summary,
        &base_samples,
        &candidate_samples,
        config(),
    )
    .unwrap()
}

#[test]
fn perf_verdict_identical_runs_do_not_regress() {
    let values = vec![60.0; 60];
    assert!(!verdict(&values, &values).regressed);
}

#[test]
fn perf_verdict_high_variance_runs_do_not_false_positive() {
    let base = (0..60)
        .map(|idx| if idx % 2 == 0 { 40.0 } else { 80.0 })
        .collect::<Vec<_>>();
    let mut candidate = base.clone();
    candidate.rotate_left(1);
    assert!(!verdict(&base, &candidate).regressed);
}

#[test]
fn perf_verdict_clear_low_variance_drop_regresses() {
    let base = vec![60.0; 60];
    let candidate = vec![45.0; 60];
    assert!(verdict(&base, &candidate).regressed);
}

#[test]
fn perf_verdict_cannot_grade_missing_or_sparse_measurements_as_good() {
    let baseline = samples(&[60.0; 60]);
    let summary = modde_core::performance::summarize_samples_with_warmup(&baseline, 0.0);
    for missing_elapsed in [true, false] {
        let mut candidate = baseline.clone();
        for sample in &mut candidate {
            if missing_elapsed {
                sample.elapsed_seconds = None;
            } else {
                sample.frame_time_ms = None;
            }
        }
        assert!(
            perf_regression_verdict(&summary, &summary, &baseline, &candidate, config()).is_err()
        );
    }
    assert!(
        perf_regression_verdict(&summary, &summary, &baseline, &baseline[..40], config()).is_err()
    );
    let mut missing_metric = summary.clone();
    missing_metric.p99_frame_time_ms = None;
    assert!(
        perf_regression_verdict(&summary, &missing_metric, &baseline, &baseline, config()).is_err()
    );
}

// ── History + retry (DB-seeded) ──────────────────────────────

use modde_core::ModdeDb;
use modde_core::profile::EnabledMod;

#[tokio::test]
async fn performance_baseline_must_match_the_source_profile_and_enabled_mod_order() {
    let db = ModdeDb::open_memory().await.unwrap();
    let mut profile = test_profile();
    profile.id = Some(db.create_profile(&profile).await.unwrap());
    db.create_performance_run(&modde_core::db::NewPerformanceRun {
        run_id: "baseline".into(),
        game_id: profile.game_id.clone(),
        profile_id: profile.id,
        profile_name: profile.name.clone(),
        mod_snapshot: modde_core::performance::mod_snapshot(&profile.mods),
        experiment_depth: 0,
        label: None,
    })
    .await
    .unwrap();
    let baseline = db.load_performance_run("baseline").await.unwrap();
    super::perf::require_baseline_profile(&baseline, &profile).unwrap();
    let mut other = profile.clone();
    other.id = Some(profile.id.unwrap() + 1);
    assert!(super::perf::require_baseline_profile(&baseline, &other).is_err());
    for change in 0..3 {
        let mut changed = profile.clone();
        match change {
            0 => changed.mods.swap(0, 1),
            1 => changed.mods[0].version = Some("new-version".into()),
            _ => changed.mods[0].enabled = false,
        }
        assert!(super::perf::require_baseline_profile(&baseline, &changed).is_err());
    }
}

#[tokio::test]
async fn candidate_cleanup_can_resume_after_partial_deletion() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let session = seed_session_for_cleanup(&pm).await;
    let first = pm
        .db()
        .list_bisect_steps(&session.session_id)
        .await
        .unwrap()
        .remove(0);
    pm.delete(&first.candidate_profile, Some(&session.game_id))
        .await
        .unwrap();
    super::candidate::cleanup_candidates(&pm, &session)
        .await
        .unwrap();
    super::candidate::cleanup_candidates(&pm, &session)
        .await
        .unwrap();
    assert!(
        pm.list_for_game(&session.game_id)
            .await
            .unwrap()
            .iter()
            .all(|profile| !profile.name.starts_with("__bisect_"))
    );
}

async fn seed_session_for_cleanup(pm: &ProfileManager) -> modde_core::BisectSession {
    let source = test_profile();
    let source_id = pm.create(&source).await.unwrap();
    let session_id = "cleanup-retry";
    pm.db()
        .create_bisect_session(&NewBisectSession {
            session_id: session_id.into(),
            game_id: source.game_id.clone(),
            source_profile_id: source_id,
            source_profile_name: source.name.clone(),
            oracle: BisectOracle::Manual,
            save_safety: BisectSaveSafety::Force,
            keep_profiles: false,
            suspect_mod_ids: vec!["mod-a".into(), "mod-b".into()],
        })
        .await
        .unwrap();
    for index in 1..=2 {
        let step = new_step(session_id, index, (&["mod-a"], &["mod-b"]));
        let mut candidate = source.clone();
        candidate.name.clone_from(&step.candidate_profile);
        pm.create(&candidate).await.unwrap();
        pm.db().create_bisect_step(&step).await.unwrap();
    }
    pm.db().load_bisect_session(session_id).await.unwrap()
}

fn enabled_mod(mod_id: &str) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        enabled: true,
        ..EnabledMod::default()
    }
}

fn test_profile() -> Profile {
    Profile {
        id: None,
        name: "bisect-source".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            enabled_mod("mod-a"),
            enabled_mod("mod-b"),
            enabled_mod("mod-c"),
            enabled_mod("mod-d"),
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: Default::default(),
        load_order_lock: None,
    }
}

fn new_step(session_id: &str, step_index: usize, half: (&[&str], &[&str])) -> NewBisectStep {
    let (enabled, disabled) = half;
    NewBisectStep {
        session_id: session_id.to_string(),
        step_index,
        candidate_profile: format!("__bisect_{session_id}_{step_index}"),
        candidate_mod_ids: enabled.iter().map(ToString::to_string).collect(),
        enabled_mod_ids: enabled.iter().map(ToString::to_string).collect(),
        disabled_mod_ids: disabled.iter().map(ToString::to_string).collect(),
    }
}

/// Seed an in-memory DB with one bisect session for `test_profile`.
async fn seed_session(db: &ModdeDb, session_id: &str) {
    let profile_id = db.create_profile(&test_profile()).await.expect("profile");
    db.create_bisect_session(&NewBisectSession {
        session_id: session_id.to_string(),
        game_id: GameId::from("skyrim-se"),
        source_profile_id: profile_id,
        source_profile_name: "bisect-source".to_string(),
        oracle: BisectOracle::Manual,
        suspect_mod_ids: vec![
            "mod-a".to_string(),
            "mod-b".to_string(),
            "mod-c".to_string(),
            "mod-d".to_string(),
        ],
        save_safety: BisectSaveSafety::Refuse,
        keep_profiles: false,
    })
    .await
    .expect("session");
}

#[cfg(unix)]
#[tokio::test]
async fn delayed_launch_completion_cannot_grade_a_new_candidate() {
    use std::os::unix::process::ExitStatusExt;
    let db = ModdeDb::open_memory().await.unwrap();
    let sid = "b-late";
    seed_session(&db, sid).await;
    let old = db
        .create_bisect_step(&new_step(sid, 1, (&["mod-a"], &["mod-b"])))
        .await
        .unwrap();
    let next = db
        .create_bisect_step(&new_step(sid, 2, (&["mod-b"], &["mod-a"])))
        .await
        .unwrap();
    db.update_bisect_session_state(
        sid,
        BisectStatus::Waiting,
        &["mod-a".into(), "mod-b".into()],
        &[],
        &[],
        Some(next),
        Some("__bisect_b-late_2"),
    )
    .await
    .unwrap();
    let pm = modde_core::profile::ProfileManager::with_db(db);
    complete_launch(
        &pm,
        &Completion {
            session: sid.into(),
            step: old,
            started_unix_ms: Some(0),
        },
        std::process::ExitStatus::from_raw(0),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        pm.db()
            .load_bisect_session(sid)
            .await
            .unwrap()
            .current_step_id,
        Some(next)
    );
    assert!(
        pm.db()
            .load_bisect_step(next)
            .await
            .unwrap()
            .result
            .is_none()
    );
}

#[tokio::test]
async fn history_lists_steps_halves_results_and_signals() {
    let db = ModdeDb::open_memory().await.expect("db");
    let sid = "b-history";
    seed_session(&db, sid).await;

    // Two completed steps with observed signals…
    let step1 = db
        .create_bisect_step(&new_step(
            sid,
            1,
            (&["mod-a", "mod-b"], &["mod-c", "mod-d"]),
        ))
        .await
        .expect("step 1");
    db.complete_bisect_step(
        step1,
        BisectResult::Good,
        Some("no new crash log observed"),
        None,
    )
    .await
    .expect("complete step 1");
    let step2 = db
        .create_bisect_step(&new_step(
            sid,
            2,
            (&["mod-c"], &["mod-a", "mod-b", "mod-d"]),
        ))
        .await
        .expect("step 2");
    db.complete_bisect_step(
        step2,
        BisectResult::Bad,
        Some("new crash log: /tmp/crash.log"),
        Some("crashed at the main menu"),
    )
    .await
    .expect("complete step 2");
    // …plus one still-pending candidate.
    let step3 = db
        .create_bisect_step(&new_step(
            sid,
            3,
            (&["mod-d"], &["mod-a", "mod-b", "mod-c"]),
        ))
        .await
        .expect("step 3");
    db.update_bisect_session_state(
        sid,
        BisectStatus::Waiting,
        &["mod-c".to_string(), "mod-d".to_string()],
        &["mod-a".to_string(), "mod-b".to_string()],
        &[],
        Some(step3),
        Some(&format!("__bisect_{sid}_3")),
    )
    .await
    .expect("update session");

    let session = db.load_bisect_session(sid).await.expect("load session");
    let steps = db.list_bisect_steps(sid).await.expect("list steps");
    let rendered = render_history(&session, &steps);

    assert!(rendered.contains("Bisect session: b-history"));
    assert!(rendered.contains("Status: waiting"));
    assert!(rendered.contains("Current suspects: mod-c, mod-d"));
    // Step indices + results.
    assert!(rendered.contains("#1 __bisect_b-history_1 result=good"));
    assert!(rendered.contains("#2 __bisect_b-history_2 result=bad"));
    assert!(rendered.contains("#3 __bisect_b-history_3 result=pending"));
    // Enabled/disabled halves.
    assert!(rendered.contains("  enabled half: mod-a, mod-b"));
    assert!(rendered.contains("  disabled half: mod-c, mod-d"));
    assert!(rendered.contains("  enabled half: mod-c\n"));
    assert!(rendered.contains("  disabled half: mod-a, mod-b, mod-d"));
    // Observed signals + notes; the pending step has neither.
    assert!(rendered.contains("  signal: no new crash log observed"));
    assert!(rendered.contains("  signal: new crash log: /tmp/crash.log"));
    assert!(rendered.contains("  notes: crashed at the main menu"));
    let step3_block = rendered
        .split("#3 ")
        .nth(1)
        .expect("step 3 section present");
    assert!(!step3_block.contains("signal:"));
    assert!(!step3_block.contains("notes:"));
}

#[tokio::test]
async fn retry_validation_rejects_session_without_pending_candidate() {
    let db = ModdeDb::open_memory().await.expect("db");
    let sid = "b-retry-none";
    seed_session(&db, sid).await;

    let session = db.load_bisect_session(sid).await.expect("load session");
    let err = validate_retry(&session).expect_err("no candidate must be rejected");
    assert!(
        err.to_string().contains("no pending candidate to retry"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn retry_validation_accepts_waiting_session_with_pending_candidate() {
    let db = ModdeDb::open_memory().await.expect("db");
    let sid = "b-retry-ok";
    seed_session(&db, sid).await;
    let step = db
        .create_bisect_step(&new_step(
            sid,
            1,
            (&["mod-a", "mod-b"], &["mod-c", "mod-d"]),
        ))
        .await
        .expect("step");
    let candidate = format!("__bisect_{sid}_1");
    db.update_bisect_session_state(
        sid,
        BisectStatus::Waiting,
        &[
            "mod-a".to_string(),
            "mod-b".to_string(),
            "mod-c".to_string(),
            "mod-d".to_string(),
        ],
        &[],
        &[],
        Some(step),
        Some(&candidate),
    )
    .await
    .expect("update session");

    let session = db.load_bisect_session(sid).await.expect("load session");
    let validated = validate_retry(&session).expect("waiting session must validate");
    assert_eq!(validated, candidate);
}

#[tokio::test]
async fn retry_validation_rejects_candidate_when_not_waiting() {
    let db = ModdeDb::open_memory().await.expect("db");
    let sid = "b-retry-stale";
    seed_session(&db, sid).await;
    let step = db
        .create_bisect_step(&new_step(
            sid,
            1,
            (&["mod-a", "mod-b"], &["mod-c", "mod-d"]),
        ))
        .await
        .expect("step");
    // Candidate name recorded but the session is back to Active — e.g. a
    // crash-oracle run that already auto-marked the step.
    db.update_bisect_session_state(
        sid,
        BisectStatus::Active,
        &["mod-a".to_string(), "mod-b".to_string()],
        &[],
        &[],
        Some(step),
        Some(&format!("__bisect_{sid}_1")),
    )
    .await
    .expect("update session");

    let session = db.load_bisect_session(sid).await.expect("load session");
    let err = validate_retry(&session).expect_err("non-waiting session must be rejected");
    assert!(
        err.to_string()
            .contains("is not waiting on a pending candidate (status: active)"),
        "unexpected error: {err}"
    );
}
