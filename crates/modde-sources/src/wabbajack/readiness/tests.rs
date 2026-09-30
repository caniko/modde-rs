use super::*;
use std::io::Write as _;

use modde_core::manifest::wabbajack::ArchiveState;
use tempfile::tempdir;
use xxhash_rust::xxh64::xxh64;

fn base_manifest() -> WabbajackManifest {
    WabbajackManifest {
        name: "Test List".into(),
        author: "tester".into(),
        description: "test".into(),
        game: "SkyrimSpecialEdition".into(),
        version: "1.0".into(),
        archives: Vec::new(),
        directives: Vec::new(),
    }
}

fn write_wabbajack(path: &Path, manifest: &WabbajackManifest) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("modlist", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(serde_json::to_string(manifest).unwrap().as_bytes())
        .unwrap();
    zip.finish().unwrap();
}

fn test_options(manifest_path: &Path, root: &Path) -> WabbajackReadinessOptions {
    let mut options = WabbajackReadinessOptions::new(manifest_path);
    options.store_dir = root.join("store");
    options.staging_root = root.join("staging");
    options.nexus_credentials_available = Some(true);
    options
}

#[tokio::test]
async fn readiness_reports_ready_manifest() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("ready.wabbajack");
    write_wabbajack(&path, &base_manifest());

    let report = assess_wabbajack_readiness(test_options(&path, temp.path()))
        .await
        .unwrap();

    assert!(report.install_ready);
    assert!(report.hard_blockers.is_empty());
    assert_eq!(
        report.normalized_game,
        if cfg!(feature = "bethesda") {
            "skyrim-se"
        } else {
            "skyrimspecialedition"
        }
    );
}

#[tokio::test]
async fn readiness_blocks_missing_manual_archive() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("manual.wabbajack");
    let mut manifest = base_manifest();
    manifest.archives.push(ArchiveEntry {
        hash: 0x1234,
        name: "manual.7z".into(),
        size: 10,
        state: Some(ArchiveState::ManualDownloader {
            url: "https://example.test/manual.7z".into(),
            prompt: "download".into(),
        }),
    });
    write_wabbajack(&path, &manifest);

    let report = assess_wabbajack_readiness(test_options(&path, temp.path()))
        .await
        .unwrap();

    assert!(!report.install_ready);
    assert_eq!(report.manual_downloads.len(), 1);
    assert!(report.hard_blockers.is_empty());
}

#[tokio::test]
async fn readiness_blocks_missing_game_file_source() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("game-file.wabbajack");
    let mut manifest = base_manifest();
    manifest.archives.push(ArchiveEntry {
        hash: 0x1234,
        name: "Skyrim.esm".into(),
        size: 10,
        state: Some(ArchiveState::GameFileSourceDownloader {
            metadata: serde_json::json!({ "File": "Data\\Skyrim.esm" })
                .as_object()
                .unwrap()
                .clone()
                .into_iter()
                .collect(),
        }),
    });
    write_wabbajack(&path, &manifest);
    let mut options = test_options(&path, temp.path());
    options.game_dir = Some(temp.path().join("game"));

    let report = assess_wabbajack_readiness(options).await.unwrap();

    assert!(!report.install_ready);
    assert_eq!(report.game_file_sources.missing.len(), 1);
    assert!(
        report
            .hard_blockers
            .iter()
            .any(|blocker| blocker.contains("game-file source"))
    );
}

#[tokio::test]
async fn readiness_blocks_mismatched_game_file_source() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("game-file.wabbajack");
    let game_dir = temp.path().join("game");
    std::fs::create_dir_all(game_dir.join("Data")).unwrap();
    std::fs::write(game_dir.join("Data/Skyrim.esm"), b"wrong").unwrap();
    let mut manifest = base_manifest();
    manifest.archives.push(ArchiveEntry {
        hash: xxh64(b"right", 0),
        name: "Skyrim.esm".into(),
        size: 5,
        state: Some(ArchiveState::GameFileSourceDownloader {
            metadata: serde_json::json!({ "File": "Data\\Skyrim.esm" })
                .as_object()
                .unwrap()
                .clone()
                .into_iter()
                .collect(),
        }),
    });
    write_wabbajack(&path, &manifest);
    let mut options = test_options(&path, temp.path());
    options.game_dir = Some(game_dir);

    let report = assess_wabbajack_readiness(options).await.unwrap();

    assert!(!report.install_ready);
    assert_eq!(report.game_file_sources.mismatched.len(), 1);
}

#[tokio::test]
async fn readiness_blocks_unknown_directive() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("unknown.wabbajack");
    let mut manifest = base_manifest();
    manifest.directives.push(RawDirective::Unknown);
    write_wabbajack(&path, &manifest);

    let report = assess_wabbajack_readiness(test_options(&path, temp.path()))
        .await
        .unwrap();

    assert!(!report.install_ready);
    assert!(
        report
            .hard_blockers
            .iter()
            .any(|blocker| blocker.contains("unsupported Wabbajack directive"))
    );
}

#[tokio::test]
async fn readiness_reports_staging_adopt() {
    let temp = tempdir().unwrap();
    let path = temp.path().join("staging.wabbajack");
    write_wabbajack(&path, &base_manifest());
    std::fs::create_dir_all(temp.path().join("staging/Test List")).unwrap();

    let report = assess_wabbajack_readiness(test_options(&path, temp.path()))
        .await
        .unwrap();

    assert_eq!(report.staging.layout_action, "adopt");
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| warning.contains("existing staging"))
    );
}
