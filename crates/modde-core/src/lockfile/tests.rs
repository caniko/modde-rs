use super::*;
use crate::profile::ProfileSource;

fn minimal_payload() -> LockPayload {
    LockPayload {
        modde_version: "test".to_string(),
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        reproducible: true,
        incomplete_reasons: Vec::new(),
        profile: LockProfile {
            name: "main".to_string(),
            game_id: "skyrim-se".to_string(),
            source: ProfileSource::Manual,
            load_order_lock: None,
            mod_order: vec!["mod-a".to_string()],
        },
        mods: vec![LockedMod {
            order: 0,
            mod_id: "mod-a".to_string(),
            display_name: None,
            enabled: true,
            version: None,
            nexus: Some(NexusProvenance {
                game_domain: "skyrimspecialedition".to_string(),
                mod_id: 1,
                file_id: 2,
            }),
            source_archive_hash: Some("abc".to_string()),
            install_method: None,
            fomod_config: None,
            files: Vec::new(),
        }],
        plugin_order: Vec::new(),
        hidden_files: Vec::new(),
        patchers: Vec::new(),
        tool_outputs: Vec::new(),
        wabbajack_manifest: None,
    }
}

#[test]
fn rejects_path_traversal() {
    let mut payload = minimal_payload();
    payload.mods[0].files.push(LockedFile {
        rel_path: "../evil".to_string(),
        origin_rel_path: "evil".to_string(),
        size: 0,
        sha256: String::new(),
        xxh64: String::new(),
        xxh3: String::new(),
        merge_group: None,
    });

    assert!(validate_payload(&payload).is_err());
}

#[test]
fn rejects_duplicate_mod_ids() {
    let mut payload = minimal_payload();
    payload.mods.push(payload.mods[0].clone());

    assert!(validate_payload(&payload).is_err());
}

#[test]
fn sign_and_verify_roundtrip() {
    let (secret, _) = generate_keypair().unwrap();
    let signing_key = signing_key_from_secret_file(&to_pretty_json(&secret).unwrap()).unwrap();
    let mut lock = ModdeLock {
        kind: LOCK_KIND.to_string(),
        format_version: LOCK_FORMAT_VERSION,
        payload: minimal_payload(),
        signatures: Vec::new(),
    };

    sign_lock(&mut lock, &signing_key).unwrap();
    verify_signatures(&lock).unwrap();

    lock.payload.profile.name = "tampered".to_string();
    assert!(verify_signatures(&lock).is_err());
}

#[tokio::test]
async fn verify_locked_file_accepts_untouched_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.bin");
    tokio::fs::write(&path, b"locked file contents")
        .await
        .unwrap();
    let locked = lock_file_at(&path, "file.bin", "file.bin", None)
        .await
        .unwrap();

    verify_locked_file(&path, &locked).await.unwrap();
}

#[tokio::test]
async fn verify_locked_file_rejects_corrupted_xxh3_digest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.bin");
    tokio::fs::write(&path, b"locked file contents")
        .await
        .unwrap();
    let locked = lock_file_at(&path, "file.bin", "file.bin", None)
        .await
        .unwrap();

    // Size, sha256, and xxh64 stay correct; only the xxh3 digest is wrong.
    let mut corrupted = locked.clone();
    let flipped = u64::from_str_radix(&locked.xxh3, 16).unwrap() ^ 1;
    corrupted.xxh3 = format!("{flipped:016x}");

    let error = verify_locked_file(&path, &corrupted).await.unwrap_err();
    match error {
        CoreError::HashMismatch {
            expected, actual, ..
        } => {
            assert_eq!(expected, corrupted.xxh3, "the xxh3 check must have failed");
            assert_eq!(actual, locked.xxh3);
        }
        other => panic!("expected hash mismatch, got: {other}"),
    }
    let message = verify_locked_file(&path, &corrupted)
        .await
        .unwrap_err()
        .to_string();
    assert!(
        message.contains("hash mismatch"),
        "error should report a hash mismatch: {message}"
    );
}

#[tokio::test]
async fn verify_locked_file_checks_xxh_before_sha256() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("file.bin");
    tokio::fs::write(&path, b"locked file contents")
        .await
        .unwrap();
    let locked = lock_file_at(&path, "file.bin", "file.bin", None)
        .await
        .unwrap();

    // Same-size corruption invalidates every digest; the xxh64 check
    // must trip first, so the reported expected digest is the 16-char
    // xxh64 value rather than the 64-char sha256.
    tokio::fs::write(&path, b"LOCKED FILE CONTENTS")
        .await
        .unwrap();
    let error = verify_locked_file(&path, &locked).await.unwrap_err();
    match error {
        CoreError::HashMismatch { expected, .. } => {
            assert_eq!(
                expected, locked.xxh64,
                "xxh64 must be verified before sha256"
            );
        }
        other => panic!("expected hash mismatch, got: {other}"),
    }
}

#[test]
fn rejects_future_lock_versions() {
    let lock = ModdeLock {
        kind: LOCK_KIND.to_string(),
        format_version: LOCK_FORMAT_VERSION + 1,
        payload: minimal_payload(),
        signatures: Vec::new(),
    };

    assert!(validate_lock(&lock).is_err());
}

fn locked_file(rel_path: &str) -> LockedFile {
    LockedFile {
        rel_path: rel_path.to_string(),
        origin_rel_path: rel_path.to_string(),
        size: 0,
        sha256: String::new(),
        xxh64: String::new(),
        xxh3: String::new(),
        merge_group: None,
    }
}

fn patcher(name: &str, outputs: &[&str]) -> PatcherStageLock {
    PatcherStageLock {
        name: name.to_string(),
        stage_kind: crate::patcher::PatcherStageKind::RustNative,
        enabled: true,
        sort_index: 0,
        settings: crate::patcher::PatcherStageSettings::RustNative,
        output_mod: "out".to_string(),
        outputs: outputs.iter().map(|path| locked_file(path)).collect(),
    }
}

#[test]
fn validation_rejects_every_invalid_or_duplicate_entry() {
    let plugin = |name: &str| PluginEntryLock {
        plugin_name: name.to_string(),
        sort_index: 0,
        enabled: true,
    };
    let hidden = |path: &str| HiddenFileLock {
        mod_id: "mod-a".to_string(),
        rel_path: path.to_string(),
    };
    let tool = |path: &str| ToolOutputLock {
        tool_id: "reshade".to_string(),
        rel_path: path.to_string(),
        file: locked_file(path),
    };
    let cases: Vec<(&str, Box<dyn Fn(&mut LockPayload)>)> = vec![
        (
            "invalid profile name",
            Box::new(|p| p.profile.name = "a/b".into()),
        ),
        (
            "invalid mod id",
            Box::new(|p| p.mods[0].mod_id = "..".into()),
        ),
        (
            "duplicate file",
            Box::new(|p| p.mods[0].files = vec![locked_file("a"), locked_file("a")]),
        ),
        (
            "invalid relative path",
            Box::new(|p| {
                let mut file = locked_file("a");
                file.origin_rel_path = "dir\\a".into();
                p.mods[0].files = vec![file];
            }),
        ),
        (
            "duplicate plugin",
            Box::new(move |p| p.plugin_order = vec![plugin("A.esp"), plugin("a.ESP")]),
        ),
        (
            "invalid relative path",
            Box::new(move |p| p.hidden_files = vec![hidden("/abs")]),
        ),
        (
            "duplicate hidden file",
            Box::new(move |p| p.hidden_files = vec![hidden("x"), hidden("x")]),
        ),
        (
            "invalid patcher stage name",
            Box::new(|p| p.patchers = vec![patcher("", &[])]),
        ),
        (
            "duplicate patcher stage",
            Box::new(|p| p.patchers = vec![patcher("s", &[]), patcher("s", &[])]),
        ),
        (
            "duplicate patcher output",
            Box::new(|p| p.patchers = vec![patcher("s", &["o", "o"])]),
        ),
        (
            "invalid relative path",
            Box::new(move |p| p.tool_outputs = vec![tool("")]),
        ),
        (
            "duplicate tool output",
            Box::new(move |p| p.tool_outputs = vec![tool("t"), tool("t")]),
        ),
    ];
    for (expected, corrupt) in cases {
        let mut payload = minimal_payload();
        corrupt(&mut payload);
        let err = validate_payload(&payload).unwrap_err().to_string();
        assert!(err.contains(expected), "{expected}: {err}");
    }

    let mut valid = minimal_payload();
    valid.plugin_order = vec![plugin("A.esp")];
    valid.hidden_files = vec![hidden("x")];
    valid.patchers = vec![patcher("s", &["o"])];
    valid.tool_outputs = vec![tool("t")];
    validate_payload(&valid).unwrap();

    let lock = ModdeLock {
        kind: "other".to_string(),
        format_version: LOCK_FORMAT_VERSION,
        payload: minimal_payload(),
        signatures: Vec::new(),
    };
    assert!(
        validate_lock(&lock)
            .unwrap_err()
            .to_string()
            .contains("unsupported lock kind")
    );
}

#[tokio::test]
async fn export_refuses_or_records_missing_provenance() {
    let db = crate::db::ModdeDb::open_memory().await.unwrap();
    let mut profile = crate::profile::Profile {
        id: None,
        name: "main".to_string(),
        game_id: crate::resolver::GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![crate::profile::EnabledMod {
            mod_id: "bare".to_string(),
            enabled: true,
            ..Default::default()
        }],
        overrides: std::path::PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: None,
    };
    let export = |profile: crate::profile::Profile, allow_incomplete: bool| {
        let db = db.clone();
        async move { export_profile_lock(&db, &profile, ExportOptions { allow_incomplete }).await }
    };

    let unsaved = export(profile.clone(), true).await.unwrap_err();
    assert!(
        unsaved.to_string().contains("is not persisted"),
        "{unsaved}"
    );
    profile.id = Some(db.create_profile(&profile).await.unwrap());

    let refused = export(profile.clone(), false)
        .await
        .unwrap_err()
        .to_string();
    for reason in [
        "missing required provenance",
        "no tracked installed_mod_files rows",
        "missing source archive hash",
        "missing portable upstream locator",
    ] {
        assert!(refused.contains(reason), "{reason}: {refused}");
    }
    let lock = export(profile.clone(), true).await.unwrap();
    assert!(!lock.payload.reproducible);
    assert_eq!(lock.payload.incomplete_reasons.len(), 3);

    // Wabbajack provenance stands in for an upstream locator.
    profile.source = ProfileSource::Wabbajack {
        manifest_hash: "lockfile-test-uncached-manifest".to_string(),
    };
    let lock = export(profile.clone(), true).await.unwrap();
    assert_eq!(lock.payload.incomplete_reasons.len(), 2);
    assert_eq!(
        lock.payload.wabbajack_manifest,
        Some(WabbajackManifestLock {
            manifest_hash: "lockfile-test-uncached-manifest".to_string(),
            cached_path: None,
            sha256: None,
        })
    );

    profile.mods.push(profile.mods[0].clone());
    let duplicate = export(profile, true).await.unwrap_err();
    assert!(
        duplicate.to_string().contains("duplicate mod id 'bare'"),
        "{duplicate}"
    );
}
