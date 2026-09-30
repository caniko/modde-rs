#![cfg(target_os = "linux")]

mod common;

use common::Fixture;
use std::ffi::OsString;
use std::path::PathBuf;

fn request(fixture: &Fixture, script: &str) -> (PathBuf, PathBuf) {
    let sessions = fixture.home().join(".config/modde/sessions");
    let run = sessions.join("run-test");
    std::fs::create_dir_all(&run).unwrap();
    let marker = fixture.root().join("descendant-finished");
    let journal = sessions.join("pending-session.json");
    let command = serde_json::json!({
        "program": OsString::from("sh"),
        "arguments": vec![OsString::from("-c"), OsString::from(script), OsString::from("test-game"), marker.as_os_str().to_owned()],
        "environment": vec![(OsString::from("PATH"), std::env::var_os("PATH").unwrap_or_default())],
        "directory": fixture.root(), "journal": journal, "sample_directory": null,
    });
    let path = run.join("request.json");
    std::fs::write(&path, serde_json::to_vec(&command).unwrap()).unwrap();
    std::fs::write(&journal, serde_json::to_vec(&serde_json::json!({
        "installation": "install-test", "name": "Test", "game_id": null, "scope": "install-test",
        "profile": null, "save_directory": null, "capture": false, "phase": "launching",
        "data_directory": fixture.data_dir(), "observation": {
            "directory": run, "unit": null,
            "boot_id": std::fs::read_to_string("/proc/sys/kernel/random/boot_id").unwrap().trim(),
        },
    })).unwrap()).unwrap();
    (path, marker)
}

#[test]
fn supervisor_waits_for_orphaned_descendants_and_keeps_leader_status() {
    let fixture = Fixture::new();
    let (path, marker) = request(&fixture, "( (sleep 0.2; printf done > \"$1\") & ) & exit 7");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "done");
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(path.parent().unwrap().join("evidence.json")).unwrap()).unwrap();
    assert_eq!(result["completed"], true);
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(std::process::ExitStatus::from_raw(result["raw_status"].as_i64().unwrap() as i32).code(), Some(7));
    assert!(!path.exists());
    fixture.cmd().args(["library", "finish"]).assert().success();
}

#[test]
fn cancelled_supervisor_request_cannot_start_a_late_game() {
    let fixture = Fixture::new();
    let (path, marker) = request(&fixture, "printf bad > \"$1\"");
    std::fs::remove_file(fixture.home().join(".config/modde/sessions/pending-session.json")).unwrap();
    fixture.cmd().args(["library", "supervise"]).arg(path).assert().failure();
    assert!(!marker.exists());
}

#[test]
fn confirmation_cannot_override_a_live_observer_lease() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "exit 0");
    let _lease = modde_core::library::lock_file(&path.parent().unwrap().join("observer.lock")).unwrap();
    fixture.cmd().args(["library", "finish", "--confirm-exited"]).assert().failure();
    assert!(fixture.home().join(".config/modde/sessions/pending-session.json").exists());
}

#[test]
fn observed_completion_worker_finishes_without_the_launching_client() {
    let fixture = Fixture::new();
    let (path, marker) = request(&fixture, "printf done > \"$1\"");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    fixture.cmd().args(["library", "complete-observed"]).arg(path.parent().unwrap()).assert().success();
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "done");
    assert!(!fixture.home().join(".config/modde/sessions/pending-session.json").exists());
    // A duplicate worker has no authority over another launch.
    let (other, _) = request(&fixture, "exit 0");
    fixture.cmd().args(["library", "complete-observed"]).arg(other.parent().unwrap().with_file_name("run-old")).assert().success();
    assert!(fixture.home().join(".config/modde/sessions/pending-session.json").exists());
}

#[test]
fn detached_failure_does_not_become_a_successful_benchmark() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "(sleep 0.1; exit 23) & exit 0");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    let result: serde_json::Value = serde_json::from_slice(&std::fs::read(path.parent().unwrap().join("evidence.json")).unwrap()).unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(std::process::ExitStatus::from_raw(result["leader_status"].as_i64().unwrap() as i32).code(), Some(0));
    assert_eq!(std::process::ExitStatus::from_raw(result["raw_status"].as_i64().unwrap() as i32).code(), Some(23));
    assert_eq!(result["descendant_failures"], 1);
}

#[test]
fn failed_exec_remains_recoverable_without_capturing_prepared_saves() {
    let fixture = Fixture::new();
    let (path, marker) = request(&fixture, "printf bad > \"$1\"");
    let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["program"] = serde_json::to_value(fixture.root().join("missing-executable").into_os_string()).unwrap();
    std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().failure();
    fixture.cmd().args(["library", "finish", "--confirm-exited"]).assert().failure();
    fixture.cmd().args(["library", "recover"]).assert().success();
    assert!(!marker.exists());
    assert!(!path.exists());
}

#[test]
fn completion_receipt_survives_cleared_launch_journal_and_blocks_new_mutations() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "exit 0");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    let sessions = path.parent().unwrap().parent().unwrap();
    let journal = sessions.join("pending-session.json");
    let mut session: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    session["phase"] = "captured".into();
    // Capture has committed. Its original save directory is no longer needed
    // by the completion retry and must never be captured a second time.
    session["capture"] = true.into();
    session["profile"] = "already-captured".into();
    session["game_id"] = "example".into();
    session["save_directory"] = serde_json::to_value(fixture.root().join("missing-saves")).unwrap();
    let receipt = sessions.join("pending-completion.json");
    std::fs::write(&receipt, serde_json::to_vec(&session).unwrap()).unwrap();
    std::fs::remove_file(&journal).unwrap();
    fixture.cmd().args(["profile", "create", "blocked", "--game", "example"]).assert().failure();
    fixture.cmd().args(["library", "complete-observed"]).arg(path.parent().unwrap()).assert().success();
    assert!(!receipt.exists());
    assert!(!journal.exists());
}

#[test]
fn missing_performance_output_retains_receipt_until_explicitly_skipped() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "exit 0");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    let sessions = path.parent().unwrap().parent().unwrap();
    let journal = sessions.join("pending-session.json");
    let mut session: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    session["launch_request"] = serde_json::json!({"performance": {
        "run_id": "missing-run", "directory": fixture.root().join("performance"), "warmup_seconds": 30.0
    }});
    std::fs::write(&journal, serde_json::to_vec(&session).unwrap()).unwrap();
    fixture.cmd().args(["library", "finish"]).assert().failure();
    assert!(!journal.exists());
    assert!(sessions.join("pending-completion.json").exists());
    fixture.cmd().args(["library", "finish", "--skip-analysis"]).assert().success();
    assert!(!sessions.join("pending-completion.json").exists());
}

#[test]
fn unwritable_analysis_output_does_not_prevent_session_capture_or_skip() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "exit 0");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    let sessions = path.parent().unwrap().parent().unwrap();
    let journal = sessions.join("pending-session.json");
    let mut session: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    let output = fixture.root().join("not-a-directory");
    std::fs::write(&output, "keep this file").unwrap();
    session["launch_request"] = serde_json::json!({"performance": {
        "run_id": "unwritable-run", "directory": output, "warmup_seconds": 30.0
    }});
    std::fs::write(&journal, serde_json::to_vec(&session).unwrap()).unwrap();

    fixture.cmd().args(["library", "finish"]).assert().failure();
    assert!(!journal.exists());
    let receipt = sessions.join("pending-completion.json");
    let captured: serde_json::Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(captured["phase"], "captured");
    fixture.cmd().args(["library", "finish", "--skip-analysis"]).assert().success();
    assert!(!receipt.exists());
    assert_eq!(std::fs::read_to_string(output).unwrap(), "keep this file");
}

#[test]
fn captured_receipt_can_be_skipped_with_lost_evidence_but_not_a_live_observer() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "exit 0");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    let sessions = path.parent().unwrap().parent().unwrap();
    let journal = sessions.join("pending-session.json");
    let mut session: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    session["phase"] = "captured".into();
    let receipt = sessions.join("pending-completion.json");
    std::fs::write(&receipt, serde_json::to_vec(&session).unwrap()).unwrap();
    std::fs::remove_file(&journal).unwrap();
    std::fs::write(path.parent().unwrap().join("evidence.json"), "interrupted JSON").unwrap();
    let lease = modde_core::library::lock_file(&path.parent().unwrap().join("observer.lock")).unwrap();
    fixture.cmd().args(["library", "finish", "--skip-analysis"]).assert().failure();
    assert!(receipt.exists());
    drop(lease);
    fixture.cmd().args(["library", "finish", "--skip-analysis"]).assert().success();
    assert!(!receipt.exists());
}

#[test]
fn confirmed_exit_captures_saves_before_reading_corrupt_analysis_evidence() {
    let fixture = Fixture::new();
    let (path, _) = request(&fixture, "exit 0");
    fixture.cmd().args(["library", "supervise"]).arg(&path).assert().success();
    let sessions = path.parent().unwrap().parent().unwrap();
    let journal = sessions.join("pending-session.json");
    let mut session: serde_json::Value = serde_json::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    let saves = fixture.root().join("saves");
    std::fs::create_dir(&saves).unwrap();
    std::fs::write(saves.join("slot.sav"), "completed game save").unwrap();
    session["capture"] = true.into();
    session["profile"] = "captured-profile".into();
    session["game_id"] = "unregistered-fixture-game".into();
    session["save_directory"] = serde_json::to_value(&saves).unwrap();
    std::fs::write(&journal, serde_json::to_vec(&session).unwrap()).unwrap();
    std::fs::write(path.parent().unwrap().join("evidence.json"), "interrupted JSON").unwrap();

    fixture.cmd().args(["library", "finish", "--confirm-exited"]).assert().failure();
    let vault_save = fixture.data_dir().join("saves/install-test/slot.sav");
    assert_eq!(std::fs::read_to_string(&vault_save).unwrap(), "completed game save");
    assert!(!journal.exists());
    let receipt = sessions.join("pending-completion.json");
    let captured: serde_json::Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    assert_eq!(captured["phase"], "captured");

    // Releasing analysis must not replace the committed snapshot with later
    // live data, even if the original exit-status JSON cannot be recovered.
    std::fs::write(saves.join("slot.sav"), "later live data").unwrap();
    fixture.cmd().args(["library", "finish", "--skip-analysis"]).assert().success();
    assert_eq!(std::fs::read_to_string(vault_save).unwrap(), "completed game save");
    assert!(!receipt.exists());
}

#[test]
fn manager_bridge_refuses_an_existing_marker_from_another_config_directory() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    let install = fixture.root().join("managed-game");
    let prefix = fixture.root().join("prefix");
    std::fs::create_dir(&install).unwrap();
    std::fs::create_dir(&prefix).unwrap();
    let marker = install.join(".modde-library-session.json");
    std::fs::write(&marker, "original owner").unwrap();
    let runner = fixture.root().join("runner");
    let executable = install.join("game.exe");
    std::fs::write(&runner, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&runner, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(&executable, "not executed").unwrap();
    let error = fixture.cmd().args(["library", "manager-wrap", "manager:test", "--root"]).arg(&install)
        .arg("--prefix").arg(&prefix).arg("--").arg(runner).arg(executable)
        .assert().failure().get_output().stderr.clone();
    assert!(String::from_utf8_lossy(&error).contains("already has a session marker"));
    assert_eq!(std::fs::read_to_string(marker).unwrap(), "original owner");
    assert!(!fixture.home().join(".config/modde/sessions/pending-session.json").exists());
}

#[tokio::test]
async fn manual_reingestion_preserves_success_failure_and_unknown_exit_status() {
    let fixture = Fixture::new();
    let db = modde_core::ModdeDb::open_at(&fixture.data_dir().join("modde.db")).await.unwrap();
    let csv = fixture.root().join("capture.csv");
    std::fs::write(&csv, "fps,frametime,elapsed\n45,22.22,0\n45,22.22,1000000000\n").unwrap();
    let original = modde_core::performance::parse_mangohud_csv("fps,frametime,elapsed\n60,16.67,0\n60,16.67,1000000000\n").unwrap();
    for (run_id, exit_status) in [("success", Some(0)), ("failure", Some(23)), ("unknown", None)] {
        db.create_performance_run(&modde_core::db::NewPerformanceRun {
            run_id: run_id.into(), game_id: "example".into(), profile_id: None,
            profile_name: "fixture".into(), mod_snapshot: Vec::new(), experiment_depth: 0, label: None,
        }).await.unwrap();
        db.complete_performance_run(run_id, &csv, exit_status, &original.summary, &original.samples).await.unwrap();
        modde_core::library::atomic_json(&fixture.data_dir().join("performance/example").join(run_id).join("configuration.json"),
            &serde_json::json!({"warmup_seconds": 0.0})).unwrap();

        fixture.cmd().args(["perf", "ingest", "--run", run_id, "--csv"]).arg(&csv)
            .args(["--warmup-seconds", "0"]).assert().success();

        let updated = db.load_performance_run(run_id).await.unwrap();
        assert_eq!(updated.exit_status, exit_status);
        assert_eq!(updated.summary.median_fps, Some(45.0));
    }
}
