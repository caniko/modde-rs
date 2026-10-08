use modde_core::library::{PendingSession, diagnostics};

#[test]
fn retention_preserves_pending_completed_experiments_and_incomplete_attempts() {
    let root = tempfile::tempdir().unwrap();
    modde_core::paths::set_data_dir(root.path().join("data"));
    modde_core::paths::set_config_dir(root.path().join("config"));
    let old = diagnostics::begin("old", "test").unwrap();
    diagnostics::finish(&old, "failed").unwrap();
    let experiment = diagnostics::begin("experiment", "test").unwrap();
    diagnostics::finish(&experiment, "completed").unwrap();
    diagnostics::log_file(&experiment.join("retained")).unwrap();
    let pending = diagnostics::begin("pending", "test").unwrap();
    diagnostics::finish(&pending, "failed").unwrap();
    modde_core::library::atomic_json(
        &PendingSession::path(),
        &serde_json::json!({
            "installation": "pending", "name": "Pending", "game_id": null,
            "scope": "pending", "profile": null, "save_directory": null,
            "capture": false, "phase": "ready", "diagnostics": pending,
        }),
    )
    .unwrap();
    let interrupted = diagnostics::begin("interrupted", "test").unwrap();
    let newest = diagnostics::begin("newest", "test").unwrap();
    diagnostics::finish(&newest, "completed").unwrap();
    diagnostics::prune(1).unwrap();
    assert!(!old.exists());
    for directory in [pending, experiment, interrupted, newest] {
        assert!(directory.join("launch.json").exists());
    }
}
