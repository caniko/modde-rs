use super::*;
use modde_core::manifest::wabbajack::WabbajackManifest;
use reqwest::{Client, Url};
use tempfile::TempDir;
use tokio::io::AsyncWriteExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xxhash_rust::xxh64::xxh64;

fn manifest_for(bytes: &[u8]) -> WabbajackManifest {
    WabbajackManifest {
        name: "test".into(),
        author: "a".into(),
        description: "d".into(),
        game: "SkyrimSE".into(),
        version: "1".into(),
        archives: vec![ArchiveEntry {
            hash: xxh64(bytes, 0),
            name: "manual.7z".into(),
            size: bytes.len() as u64,
            state: Some(ArchiveState::ManualDownloader {
                url: "https://example.test/manual.7z".into(),
                prompt: String::new(),
            }),
        }],
        directives: vec![],
    }
}

fn manifest_with_manual_and_nexus(manual: &[u8], nexus: &[u8]) -> WabbajackManifest {
    WabbajackManifest {
        name: "test".into(),
        author: "a".into(),
        description: "d".into(),
        game: "SkyrimSE".into(),
        version: "1".into(),
        archives: vec![
            ArchiveEntry {
                hash: xxh64(manual, 0),
                name: "manual.7z".into(),
                size: manual.len() as u64,
                state: Some(ArchiveState::ManualDownloader {
                    url: "https://example.test/manual.7z".into(),
                    prompt: String::new(),
                }),
            },
            ArchiveEntry {
                hash: xxh64(nexus, 0),
                name: "nexus.7z".into(),
                size: nexus.len() as u64,
                state: Some(ArchiveState::NexusDownloader {
                    game_name: "SkyrimSpecialEdition".into(),
                    mod_id: 631.into(),
                    file_id: 5118.into(),
                }),
            },
        ],
        directives: vec![],
    }
}

#[test]
fn missing_archive_scanner_skips_existing_store_file() {
    let bytes = b"archive";
    let manifest = manifest_for(bytes);
    let temp = TempDir::new().unwrap();
    let store = temp.path().join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(archive_path(&store, &manifest.archives[0].hash), bytes).unwrap();

    assert_eq!(missing_archives(&manifest, &store, false).len(), 0);
}

#[test]
fn missing_archive_scanner_reports_manual_archive() {
    let manifest = manifest_for(b"archive");
    let temp = TempDir::new().unwrap();
    let missing = missing_archives(&manifest, &temp.path().join("store"), false);

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].source_kind, MissingArchiveSourceKind::Manual);
    assert_eq!(missing[0].name, "manual.7z");
}

#[test]
fn missing_archive_scanner_omits_nexus_unless_included() {
    let manifest = manifest_with_manual_and_nexus(b"manual archive", b"nexus archive");
    let temp = TempDir::new().unwrap();
    let store = temp.path().join("store");

    let manual_only = missing_archives(&manifest, &store, false);
    assert_eq!(manual_only.len(), 1);
    assert_eq!(manual_only[0].source_kind, MissingArchiveSourceKind::Manual);

    let all = missing_archives(&manifest, &store, true);
    assert_eq!(all.len(), 2);
    assert!(all.iter().any(|archive| {
        archive.source_kind == MissingArchiveSourceKind::Nexus
            && archive.source_hint.contains("mod_id=631")
            && archive.source_hint.contains("file_id=5118")
    }));
}

#[test]
fn missing_archive_scanner_reports_only_entries_missing_from_store() {
    let manifest = manifest_with_manual_and_nexus(b"manual archive", b"nexus archive");
    let temp = TempDir::new().unwrap();
    let store = temp.path().join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(
        archive_path(&store, &manifest.archives[0].hash),
        b"manual archive",
    )
    .unwrap();

    let missing = missing_archives(&manifest, &store, true);
    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].name, "nexus.7z");
    assert_eq!(missing[0].source_kind, MissingArchiveSourceKind::Nexus);
}

#[test]
fn partial_download_extensions_are_rejected() {
    assert!(partial_download_path(Path::new("file.7z.crdownload")));
    assert!(partial_download_path(Path::new("file.7z.part")));
    assert!(partial_download_path(Path::new("file.7z.download")));
    assert!(partial_download_path(Path::new("file.7z.tmp")));
    assert!(partial_download_path(Path::new("file.7z.opdownload")));
    assert!(!partial_download_path(Path::new("file.7z")));
}

#[test]
fn modding_tools_nexus_entries_get_site_browser_url() {
    assert_eq!(normalize_nexus_game_domain("ModdingTools"), "site");
    assert_eq!(
        nexus_browser_url(
            "ModdingTools",
            NexusModId::from(631),
            NexusFileId::from(5118),
        )
        .unwrap(),
        "https://www.nexusmods.com/site/mods/631?tab=files&file_id=5118"
    );
}

#[test]
fn direct_action_parser_reads_form_and_link_text() {
    let archive = MissingArchive {
        hash: xxh64(b"archive", 0),
        name: "archive.7z".into(),
        size: 7,
        source_kind: MissingArchiveSourceKind::Manual,
        url: Some("https://example.test/file".into()),
        source_hint: "test".into(),
    };
    let base = Url::parse("https://example.test/file").unwrap();
    let form_actions = extract_manual_actions(
        r#"<form method="post" action="/create"><button>Create download link</button></form>"#,
        &base,
        &archive,
    )
    .unwrap();
    assert_eq!(form_actions.len(), 1);
    assert!(matches!(
        form_actions[0].method,
        ManualActionMethod::Post(_)
    ));

    let link_actions =
        extract_manual_actions(r#"<a href="/start">Start Download</a>"#, &base, &archive).unwrap();
    assert_eq!(link_actions.len(), 1);
    assert_eq!(link_actions[0].url.as_str(), "https://example.test/start");
}

#[test]
fn browser_event_adapter_returns_download_path() {
    let event = BrowserDownloadEvent {
        path: PathBuf::from("/tmp/archive.7z"),
    };
    assert_eq!(
        browser_event_download_path(&event),
        Path::new("/tmp/archive.7z")
    );
}

#[tokio::test]
async fn watcher_accepts_stable_matching_download() {
    let bytes = b"correct archive";
    let manifest = manifest_for(bytes);
    let archive = missing_archives(&manifest, Path::new("/missing"), false)
        .pop()
        .unwrap();
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("manual.7z");
    tokio::fs::write(&path, bytes).await.unwrap();

    let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(found.matched);
    assert_eq!(found.path, path);
}

#[tokio::test]
async fn watcher_reports_same_name_mismatch_after_timeout() {
    let manifest = manifest_for(b"correct archive");
    let archive = missing_archives(&manifest, Path::new("/missing"), false)
        .pop()
        .unwrap();
    let temp = TempDir::new().unwrap();
    tokio::fs::write(temp.path().join("manual.7z"), b"wrong archive")
        .await
        .unwrap();

    let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(3))
        .await
        .unwrap();
    assert!(!found.matched);
    assert!(found.name_matched);
}

#[tokio::test]
async fn watcher_accepts_file_renamed_from_partial_to_final() {
    let bytes = b"correct archive";
    let manifest = manifest_for(bytes);
    let archive = missing_archives(&manifest, Path::new("/missing"), false)
        .pop()
        .unwrap();
    let temp = TempDir::new().unwrap();
    let partial = temp.path().join("manual.7z.crdownload");
    let final_path = temp.path().join("manual.7z");
    tokio::fs::write(&partial, bytes).await.unwrap();

    let partial_for_task = partial.clone();
    let final_for_task = final_path.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(750)).await;
        tokio::fs::rename(partial_for_task, final_for_task)
            .await
            .unwrap();
    });

    let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(6))
        .await
        .unwrap();
    assert!(found.matched);
    assert_eq!(found.path, final_path);
}

#[tokio::test]
async fn watcher_ignores_wrong_name_wrong_hash_and_continues_waiting() {
    let bytes = b"correct archive";
    let manifest = manifest_for(bytes);
    let archive = missing_archives(&manifest, Path::new("/missing"), false)
        .pop()
        .unwrap();
    let temp = TempDir::new().unwrap();
    tokio::fs::write(temp.path().join("unrelated.7z"), b"wrong archive")
        .await
        .unwrap();

    let correct = temp.path().join("renamed-but-correct.7z");
    let correct_for_task = correct.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(750)).await;
        tokio::fs::write(correct_for_task, bytes).await.unwrap();
    });

    let found = wait_for_matching_download(temp.path(), &archive, Duration::from_secs(6))
        .await
        .unwrap();
    assert!(found.matched);
    assert!(!found.name_matched);
    assert_eq!(found.path, correct);
}

#[tokio::test]
async fn watcher_matches_multiple_pending_archives_in_any_order() {
    let manifest = manifest_with_manual_and_nexus(b"manual archive", b"nexus archive");
    let pending = missing_archives(&manifest, Path::new("/missing"), true);
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("second-first.7z");
    tokio::fs::write(&path, b"nexus archive").await.unwrap();

    let found = wait_for_next_matching_download(temp.path(), &pending, Duration::from_secs(5))
        .await
        .unwrap();
    assert!(found.matched);
    assert_eq!(found.path, path);
    assert_eq!(found.matched_hash, Some(xxh64(b"nexus archive", 0)));
}

#[tokio::test]
async fn import_acquired_archive_imports_exact_hash() {
    let bytes = b"correct archive";
    let manifest = manifest_for(bytes);
    let archive = missing_archives(&manifest, Path::new("/missing"), false)
        .pop()
        .unwrap();
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("manual.7z");
    let store = temp.path().join("store");
    let mut file = tokio::fs::File::create(&source).await.unwrap();
    file.write_all(bytes).await.unwrap();
    file.flush().await.unwrap();

    let result = import_acquired_archive(&manifest, &store, &archive, &source)
        .await
        .unwrap();
    assert_eq!(result.status, AcquireStatus::Imported);
    assert!(archive_path(&store, &archive.hash).exists());
}

#[tokio::test]
async fn import_acquired_archive_refuses_same_name_wrong_hash() {
    let manifest = manifest_for(b"correct archive");
    let archive = missing_archives(&manifest, Path::new("/missing"), false)
        .pop()
        .unwrap();
    let temp = TempDir::new().unwrap();
    let source = temp.path().join("manual.7z");
    let store = temp.path().join("store");
    tokio::fs::write(&source, b"wrong archive").await.unwrap();

    let result = import_acquired_archive(&manifest, &store, &archive, &source)
        .await
        .unwrap();
    assert_eq!(result.status, AcquireStatus::Mismatched);
    assert!(!archive_path(&store, &archive.hash).exists());
}

mod direct;
