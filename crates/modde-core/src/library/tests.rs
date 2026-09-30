use super::*;

#[test]
fn installation_identity_includes_store_and_path() {
    let a = installation_id("steam:1", Path::new("/games/a"));
    assert_ne!(a, installation_id("steam:1", Path::new("/games/b")));
    assert_ne!(a, installation_id("gog:1", Path::new("/games/a")));
    assert_eq!(a, installation_id("steam:1", Path::new("/games/a")));
}

#[test]
fn old_preferences_default_to_unsandboxed() {
    let settings: LibraryPreferences = serde_json::from_str("{}").unwrap();
    assert!(!settings.launch_for("missing").sandbox.enabled);
    assert!(settings.launch_for("missing").sandbox.network);
    assert!(settings.launch_for("missing").use_active_profile);
}

#[test]
fn old_session_journals_never_authorize_preparation_rollback() {
    let session: PendingSession = serde_json::from_value(serde_json::json!({
        "installation": "old", "name": "Example", "game_id": "example", "scope": "example",
        "profile": "base", "save_directory": "/saves", "capture": true
    })).unwrap();
    assert_eq!(session.phase, SessionPhase::Launching);
    assert!(!session.phase.is_preparation());
}

#[test]
fn updates_preserve_other_installations_and_reject_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.json");
    LibraryPreferences::update_at(&path, |settings| {
        settings.favorites.insert("steam:1".into());
    }).unwrap();
    LibraryPreferences::update_at(&path, |settings| {
        settings.launches.insert("install-b".into(), LaunchSettings::default());
    }).unwrap();
    assert!(LibraryPreferences::load_at(&path).unwrap().favorites.contains("steam:1"));
    std::fs::write(&path, "broken").unwrap();
    assert!(LibraryPreferences::update_at(&path, |_| {}).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "broken");
}

#[test]
fn session_locks_reject_concurrent_launches_and_release_on_drop() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("session.lock");
    let first = lock_file(&path).unwrap();
    assert!(lock_file(&path).is_err());
    drop(first);
    assert!(lock_file(&path).is_ok());
}

#[test]
fn reassociation_preserves_identity_settings_and_legacy_binding() {
    let dir = tempfile::tempdir().unwrap();
    let old = installation_id("local:example", dir.path());
    let store = installation_id("steam:42", dir.path());
    let mut prefs = LibraryPreferences::default();
    let mut settings = LaunchSettings::default();
    settings.sandbox.enabled = true;
    prefs.launches.insert(old.clone(), settings.clone());
    prefs.legacy_save_bindings.insert("example".into(), LegacySaveBinding {
        installation: old.clone(), save_directory: None,
    });
    let id = prefs.bind_installation(&[dir.path().to_path_buf()], &[old.clone(), store], &["steam:42".into()]).unwrap();
    assert_eq!(id, old);
    assert_eq!(prefs.launch_for(&id), settings);
    assert_eq!(prefs.legacy_save_bindings["example"].installation, id);
    assert_eq!(prefs.bind_installation(&[dir.path().into()], &[], &["gog:99".into()]).unwrap(), id);
}

#[test]
fn conflicting_existing_identities_require_explicit_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let mut prefs = LibraryPreferences::default();
    prefs.launches.insert("a".into(), LaunchSettings::default());
    prefs.launches.insert("b".into(), LaunchSettings::default());
    let before = prefs.clone();
    assert!(prefs.bind_installation(&[dir.path().into()], &["a".into(), "b".into()], &[]).is_err());
    assert_eq!(prefs, before);
}

#[cfg(unix)]
#[test]
fn missing_child_under_path_alias_keeps_identity_when_created() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("real")).unwrap();
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias")).unwrap();
    let alias = dir.path().join("alias/game");
    let real = dir.path().join("real/game");
    let mut prefs = LibraryPreferences::default();
    let before = prefs.bind_installation(&[alias], &[], &[]).unwrap();
    std::fs::create_dir(&real).unwrap();
    assert_eq!(prefs.bind_installation(&[real], &[], &[]).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn retargeted_alias_cannot_transfer_an_installation_identity() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let alias = root.path().join("current");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    std::os::unix::fs::symlink(&a, &alias).unwrap();
    let mut prefs = LibraryPreferences::default();
    let id = prefs.bind_installation(&[alias.clone()], &[], &[]).unwrap();
    let before = prefs.clone();
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&b, &alias).unwrap();
    assert!(prefs.bind_installation(&[alias], &[], &[]).unwrap_err().to_string().contains("rebind"));
    assert_eq!(prefs, before);
    assert_eq!(prefs.bind_installation(&[a], &[], &[]).unwrap(), id);
    assert_ne!(prefs.bind_installation(&[b], &[], &[]).unwrap(), id);
}

#[cfg(unix)]
#[test]
fn legacy_alias_records_do_not_follow_a_new_target() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let alias = root.path().join("current");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    std::os::unix::fs::symlink(&b, &alias).unwrap();
    let mut prefs: LibraryPreferences = serde_json::from_value(serde_json::json!({
        "installations": {"old": {"paths": [a, alias], "entitlements": []}}
    })).unwrap();
    assert!(prefs.bind_installation(&[alias], &[], &[]).is_err());
    assert_ne!(prefs.bind_installation(&[b], &[], &[]).unwrap(), "old");
}
