use super::*;

#[test]
fn set_concurrency_changes_value() {
    let mut inst = WabbajackInstaller::new(
        minimal_manifest(),
        PathBuf::new(),
        PathBuf::new(),
        PathBuf::new(),
    );
    inst.set_concurrency(16);
    assert_eq!(inst.concurrency, 16);
}

#[test]
fn set_concurrency_clamps_zero_to_one() {
    let mut inst = WabbajackInstaller::new(
        minimal_manifest(),
        PathBuf::new(),
        PathBuf::new(),
        PathBuf::new(),
    );
    inst.set_concurrency(0);
    assert_eq!(inst.concurrency, 1);
}

#[tokio::test]
async fn install_reads_game_file_source_from_game_dir() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    let store_dir = dir.path().join("store");
    let staging_dir = dir.path().join("staging");
    std::fs::create_dir_all(game_dir.join("Data")).unwrap();
    let source_bytes = b"game file bytes";
    std::fs::write(game_dir.join("Data/Update.esm"), source_bytes).unwrap();

    let manifest = manifest_with_game_file(
        xxh64(source_bytes, 0),
        "Data\\Update.esm",
        "mods/Skyrim Base/Update.esm",
    );

    let mut inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        store_dir,
        staging_dir.clone(),
    );
    inst.set_game_dir(game_dir);

    // The tiny fixture exercises installation, not the runner's live memory pressure.
    inst.apply_memory_provider = Some(admission::fixture_provider());

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    inst.install(progress_tx).await.unwrap();

    assert_eq!(
        std::fs::read(staging_dir.join("mods/Skyrim Base/Update.esm")).unwrap(),
        b"game file bytes"
    );
}

#[tokio::test]
async fn install_requires_game_dir_for_game_file_sources() {
    let dir = tempfile::tempdir().unwrap();
    let manifest = WabbajackManifest {
        archives: vec![game_file_archive(0x1234, "Data\\Update.esm")],
        directives: vec![],
        ..minimal_manifest()
    };

    let inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        dir.path().join("staging"),
    );

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    let err = inst.install(progress_tx).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("pass --game-dir"),
        "unexpected error message: {msg}"
    );
}

#[tokio::test]
async fn install_rejects_mismatched_game_file_hash() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    std::fs::create_dir_all(game_dir.join("Data")).unwrap();
    std::fs::write(game_dir.join("Data/Update.esm"), b"actual bytes").unwrap();

    let manifest = manifest_with_game_file(
        xxh64(b"expected bytes", 0),
        "Data\\Update.esm",
        "mods/Update.esm",
    );
    let mut inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        dir.path().join("staging"),
    );
    inst.set_game_dir(game_dir);

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    let err = inst.install(progress_tx).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("Data\\Update.esm"), "unexpected error: {msg}");
    assert!(
        msg.contains("expected xxh64"),
        "unexpected error message: {msg}"
    );
}

#[tokio::test]
async fn install_rejects_missing_game_file() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();

    let manifest = manifest_with_game_file(
        xxh64(b"missing bytes", 0),
        "Data\\Update.esm",
        "mods/Update.esm",
    );
    let mut inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        dir.path().join("staging"),
    );
    inst.set_game_dir(game_dir);

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    let err = inst.install(progress_tx).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(msg.contains("Data\\Update.esm"), "unexpected error: {msg}");
    assert!(msg.contains("missing"), "unexpected error: {msg}");
}

#[tokio::test]
async fn install_reports_all_invalid_game_files() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    std::fs::create_dir_all(game_dir.join("Data")).unwrap();
    std::fs::write(game_dir.join("Data/Update.esm"), b"actual bytes").unwrap();

    let mismatch_hash = xxh64(b"expected bytes", 0);
    let missing_hash = xxh64(b"missing bytes", 0);
    let manifest = WabbajackManifest {
        archives: vec![
            game_file_archive(mismatch_hash, "Data\\Update.esm"),
            game_file_archive(missing_hash, "SkyrimSE.exe"),
        ],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![serde_json::Value::Number(mismatch_hash.into())],
                to: "mods/Update.esm".into(),
                size: 0,
            },
            modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![serde_json::Value::Number(missing_hash.into())],
                to: "mods/SkyrimSE.exe".into(),
                size: 0,
            },
        ],
        ..minimal_manifest()
    };
    let mut inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        dir.path().join("staging"),
    );
    inst.set_game_dir(game_dir);

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    let err = inst.install(progress_tx).await.unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("validation failed for 2 file(s)"),
        "unexpected error: {msg}"
    );
    assert!(msg.contains("Data\\Update.esm"), "unexpected error: {msg}");
    assert!(msg.contains("got"), "unexpected error: {msg}");
    assert!(msg.contains("SkyrimSE.exe"), "unexpected error: {msg}");
    assert!(msg.contains("missing"), "unexpected error: {msg}");
}

#[tokio::test]
async fn install_resolves_game_file_path_case_insensitively() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    let store_dir = dir.path().join("store");
    let staging_dir = dir.path().join("staging");
    std::fs::create_dir_all(game_dir.join("DATA")).unwrap();
    let source_bytes = b"case insensitive game file";
    std::fs::write(game_dir.join("DATA/update.ESM"), source_bytes).unwrap();

    let manifest = manifest_with_game_file(
        xxh64(source_bytes, 0),
        "Data\\Update.esm",
        "mods/Update.esm",
    );
    let mut inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        store_dir,
        staging_dir.clone(),
    );
    inst.set_game_dir(game_dir);
    inst.apply_memory_provider = Some(admission::fixture_provider());

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    inst.install(progress_tx).await.unwrap();

    assert_eq!(
        std::fs::read(staging_dir.join("mods/Update.esm")).unwrap(),
        source_bytes
    );
}

// -----------------------------------------------------------------------
// 3. find_entry_in_archive with exact match
// -----------------------------------------------------------------------
