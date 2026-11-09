use super::*;

fn synthetic_server(cdn_archive: Vec<u8>, direct_archive: Vec<u8>) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let thread_count = Arc::clone(&count);
    thread::spawn(move || {
        for stream in listener.incoming().take(8) {
            let mut stream = stream.unwrap();
            let mut buf = [0_u8; 2048];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            thread_count.fetch_add(1, Ordering::SeqCst);
            match path {
                "/authored_files/download/cdn.zip_abc" => {
                    let body = format!(
                        r#"<script>
                              const MUNGED_NAME = "cdn.zip_abc";
                              const FILE_NAME = "cdn.zip";
                              const FILE_SIZE_BYTES = {};
                              const PARTS = [{{"Size":{},"Offset":0,"Index":0}}];
                            </script>"#,
                        cdn_archive.len(),
                        cdn_archive.len()
                    );
                    write_response(&mut stream, "200 OK", body.as_bytes());
                }
                "/authored_files/cdn.zip_abc/parts/0" => {
                    write_response(&mut stream, "200 OK", &cdn_archive);
                }
                "/direct.zip" => {
                    write_response(&mut stream, "200 OK", &direct_archive);
                }
                _ => write_response(&mut stream, "404 Not Found", b"not found"),
            }
        }
    });
    (format!("http://{addr}"), count)
}

fn write_response(stream: &mut std::net::TcpStream, status: &str, body: &[u8]) {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
}

// -----------------------------------------------------------------------
// 1. WabbajackInstaller::new() creates correct directory structure
// -----------------------------------------------------------------------
#[test]
fn new_stores_fields() {
    let store = PathBuf::from("/tmp/store");
    let staging = PathBuf::from("/tmp/staging");
    let inst = WabbajackInstaller::new(
        minimal_manifest(),
        PathBuf::from("/tmp/test.wabbajack"),
        store,
        staging,
    );
    assert_eq!(inst.concurrency, DEFAULT_CONCURRENCY);
    assert!(inst.sources.is_empty());
    assert_eq!(inst.store_dir, PathBuf::from("/tmp/store"));
    assert_eq!(inst.staging_dir, PathBuf::from("/tmp/staging"));
    assert_eq!(inst.manifest.name, "test");
}

#[test]
fn validate_download_sources_rejects_required_source_before_network() {
    let dir = tempfile::tempdir().unwrap();
    let inst = WabbajackInstaller::new(
        manifest_with_nexus_download(1),
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        dir.path().join("staging"),
    );

    let downloads = inst.manifest.download_directives();
    let err = inst.validate_download_sources(&downloads).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("no registered source"),
        "unexpected error: {msg}"
    );
    assert!(msg.contains("nexus:123"), "unexpected error: {msg}");
}

#[tokio::test]
async fn adoption_preserves_existing_outputs_and_writes_sentinels() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("staging");
    let output = staging.join("mods/adopted/file.txt");
    tokio::fs::create_dir_all(output.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&output, b"already applied").await.unwrap();
    tokio::fs::write(staging.join("mods/adopted/output.bsa"), b"bsa")
        .await
        .unwrap();

    let hash = 42_u64;
    let manifest = WabbajackManifest {
        archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
            hash,
            name: "source.zip".into(),
            size: 123,
            state: None,
        }],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(hash.into()),
                    serde_json::Value::String("file.txt".into()),
                ],
                to: "mods/adopted/file.txt".into(),
                size: 0,
            },
            modde_core::manifest::wabbajack::RawDirective::CreateBSA {
                temp_id: "bsa-temp".into(),
                to: "mods/adopted/output.bsa".into(),
                file_states: vec![],
            },
        ],
        ..minimal_manifest()
    };
    let inst = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        staging.clone(),
    );

    let prepare_status = StagingStore::new(&staging)
        .prepare_resumable()
        .await
        .unwrap();
    assert_eq!(prepare_status, StagingPrepareStatus::Adopted);

    let batches = inst.manifest.install_directives_grouped_by_archive();
    let installs = inst.manifest.install_directives();
    let adoption = inst
        .adopt_existing_staging(&batches, &installs)
        .await
        .unwrap();

    assert_eq!(adoption.archive_batches, 1);
    assert_eq!(adoption.create_bsa, 1);
    assert!(output.exists());
    assert!(inst.archive_batch_sentinel_path(hash).exists());
    assert!(inst.create_bsa_sentinel_path(1).exists());
    assert!(inst.archive_batch_sentinel_valid(&batches[0]).await);
    assert!(
        inst.create_bsa_sentinel_valid(1, "bsa-temp", "mods/adopted/output.bsa")
            .await
    );
}

#[tokio::test]
async fn synthetic_wabbajack_pipeline_reaches_late_directives() {
    let dir = tempfile::tempdir().unwrap();
    let cdn_archive = zip_bytes(&[("source.txt", b"basis")]);
    let large_direct = vec![7_u8; 1024 * 1024 + 17];
    let direct_archive = zip_bytes(&[
        ("direct.txt", b"direct"),
        ("large.dds", large_direct.as_slice()),
    ]);
    let cdn_hash = xxh64(&cdn_archive, 0);
    let direct_hash = xxh64(&direct_archive, 0);
    let (base_url, _requests) = synthetic_server(cdn_archive, direct_archive);
    let wabbajack_path = dir.path().join("synthetic.wabbajack");
    create_zip_file(
        &wabbajack_path,
        &[
            ("inline-data", b"inline"),
            ("patch-data", &data_patch(b"patched")),
        ],
    );

    let manifest = WabbajackManifest {
        archives: vec![
            modde_core::manifest::wabbajack::ArchiveEntry {
                hash: cdn_hash,
                name: "cdn.zip".into(),
                size: 0,
                state: Some(ArchiveState::WabbajackCDNDownloader {
                    metadata: HashMap::from([(
                        "Url".into(),
                        serde_json::Value::String(format!(
                            "{base_url}/authored_files/download/cdn.zip_abc"
                        )),
                    )]),
                }),
            },
            modde_core::manifest::wabbajack::ArchiveEntry {
                hash: direct_hash,
                name: "direct.zip".into(),
                size: 0,
                state: Some(ArchiveState::HttpDownloader {
                    url: format!("{base_url}/direct.zip"),
                    headers: HashMap::new(),
                }),
            },
        ],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(cdn_hash.into()),
                    serde_json::Value::String("source.txt".into()),
                ],
                to: "mods/cdn/source.txt".into(),
                size: 0,
            },
            modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(direct_hash.into()),
                    serde_json::Value::String("direct.txt".into()),
                ],
                to: "mods/direct/direct.txt".into(),
                size: 0,
            },
            modde_core::manifest::wabbajack::RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(direct_hash.into()),
                    serde_json::Value::String("large.dds".into()),
                ],
                to: "mods/direct/large.dds".into(),
                size: 0,
            },
            modde_core::manifest::wabbajack::RawDirective::InlineFile {
                hash: 0,
                size: 6,
                source_data_id: "inline-data".into(),
                to: "mods/inline/inline.txt".into(),
            },
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(cdn_hash.into()),
                    serde_json::Value::String("source.txt".into()),
                ],
                to: "mods/patched/patched.txt".into(),
                hash: 0,
                patch_id: "patch-data".into(),
                size: 0,
            },
        ],
        ..minimal_manifest()
    };
    let mut inst = WabbajackInstaller::new(
        manifest,
        wabbajack_path,
        dir.path().join("store"),
        dir.path().join("staging"),
    );
    inst.apply_memory_provider = Some(admission::fixture_provider());
    let client = reqwest::Client::new();
    inst.add_source(crate::AnySource::WabbajackCdn(
        crate::wabbajack::cdn::WabbajackCdnSource::new(client.clone()),
    ));
    inst.add_source(crate::AnySource::Direct(crate::direct::DirectSource::new(
        client,
    )));

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    inst.install(progress_tx).await.unwrap();

    assert_eq!(
        tokio::fs::read(dir.path().join("staging/mods/cdn/source.txt"))
            .await
            .unwrap(),
        b"basis"
    );
    assert_eq!(
        tokio::fs::read(dir.path().join("staging/mods/direct/direct.txt"))
            .await
            .unwrap(),
        b"direct"
    );
    assert_eq!(
        tokio::fs::read(dir.path().join("staging/mods/inline/inline.txt"))
            .await
            .unwrap(),
        b"inline"
    );
    assert_eq!(
        tokio::fs::read(dir.path().join("staging/mods/patched/patched.txt"))
            .await
            .unwrap(),
        b"patched"
    );

    let large_path = dir.path().join("staging/mods/direct/large.dds");
    assert!(compressed_path(&large_path).exists());
    assert!(!large_path.exists());
    let staging_store = StagingStore::new(dir.path().join("staging"));
    let mut reader = staging_store
        .open_logical_reader("mods/direct/large.dds")
        .unwrap();
    let mut decoded = Vec::new();
    reader.read_to_end(&mut decoded).unwrap();
    assert_eq!(decoded, large_direct);

    assert!(
        dir.path()
            .join(format!(
                "staging/_state/archive-batches/{direct_hash:016x}.json"
            ))
            .exists()
    );
    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    inst.install(progress_tx).await.unwrap();
    assert!(compressed_path(&large_path).exists());
}

// -----------------------------------------------------------------------
// 2. set_concurrency changes and clamps the value
// -----------------------------------------------------------------------

#[tokio::test]
async fn nested_archives_feed_copies_and_patches() {
    use modde_core::manifest::wabbajack::{ArchiveEntry, RawDirective};

    let dir = tempfile::tempdir().unwrap();
    let inner = zip_bytes(&[("x.txt", b"inner")]);
    let outer = zip_bytes(&[("inner.zip", inner.as_slice()), ("top.txt", b"top")]);
    let hash = xxh64(&outer, 0);
    let store = dir.path().join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join(format!("{hash:016x}.archive")), &outer).unwrap();
    let wabbajack_path = dir.path().join("nested.wabbajack");
    create_zip_file(
        &wabbajack_path,
        &[
            ("patch-inner", &data_patch(b"patched inner")),
            ("patch-top", &data_patch(b"patched top")),
        ],
    );
    let path = |parts: &[&str]| {
        std::iter::once(serde_json::Value::Number(hash.into()))
            .chain(parts.iter().map(|p| serde_json::Value::String((*p).into())))
            .collect::<Vec<_>>()
    };
    let patched = |parts: &[&str], to: &str, patch_id: &str| RawDirective::PatchedFromArchive {
        archive_hash_path: path(parts),
        to: to.into(),
        hash: 0,
        patch_id: patch_id.into(),
        size: 0,
    };
    let manifest = WabbajackManifest {
        archives: vec![ArchiveEntry {
            hash,
            name: "outer.zip".into(),
            size: outer.len() as u64,
            state: Some(ArchiveState::ManualDownloader {
                url: "https://example.test/outer.zip".into(),
                prompt: String::new(),
            }),
        }],
        directives: vec![
            RawDirective::FromArchive {
                archive_hash_path: path(&["inner.zip", "x.txt"]),
                to: "mods/n/x.txt".into(),
                size: 5,
            },
            patched(&["inner.zip", "x.txt"], "mods/n/inner.txt", "patch-inner"),
            patched(&["top.txt"], "mods/n/top.txt", "patch-top"),
        ],
        ..minimal_manifest()
    };
    let staging = dir.path().join("staging");
    let mut inst = WabbajackInstaller::new(manifest, wabbajack_path, store, staging.clone());
    inst.apply_memory_provider = Some(admission::fixture_provider());
    inst.add_source(crate::AnySource::Manual(crate::manual::ManualSource::new()));

    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    inst.install(progress_tx).await.unwrap();
    for (file, expected) in [
        ("x.txt", &b"inner"[..]),
        ("inner.txt", b"patched inner"),
        ("top.txt", b"patched top"),
    ] {
        assert_eq!(
            std::fs::read(staging.join("mods/n").join(file)).unwrap(),
            expected,
            "{file}"
        );
    }
}

/// One-file zip install into `staging`, with `tmp_dir` as the sweep base.
async fn install_single_file(
    dir: &std::path::Path,
    staging: &std::path::Path,
    tmp_dir: Option<PathBuf>,
) {
    use modde_core::manifest::wabbajack::{ArchiveEntry, RawDirective};

    let archive = zip_bytes(&[("file.txt", b"correct")]);
    let hash = xxh64(&archive, 0);
    let store = dir.join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join(format!("{hash:016x}.archive")), &archive).unwrap();
    let manifest = WabbajackManifest {
        archives: vec![ArchiveEntry {
            hash,
            name: "source.zip".into(),
            size: archive.len() as u64,
            state: Some(ArchiveState::ManualDownloader {
                url: "https://example.test/source.zip".into(),
                prompt: String::new(),
            }),
        }],
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(hash.into()),
                serde_json::Value::String("file.txt".into()),
            ],
            to: "mods/r/file.txt".into(),
            size: 7,
        }],
        ..minimal_manifest()
    };
    let wabbajack_path = dir.join("resume.wabbajack");
    create_zip_file(&wabbajack_path, &[]);
    let mut inst = WabbajackInstaller::new(manifest, wabbajack_path, store, staging.to_path_buf());
    inst.apply_memory_provider = Some(admission::fixture_provider());
    inst.tmp_dir = tmp_dir;
    inst.add_source(crate::AnySource::Manual(crate::manual::ManualSource::new()));
    let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
    inst.install(progress_tx).await.unwrap();
}

#[tokio::test]
async fn resume_reextracts_wrong_sized_output_but_adopts_correct_tree() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("staging");
    let output = staging.join("mods/r/file.txt");

    install_single_file(dir.path(), &staging, None).await;
    assert_eq!(std::fs::read(&output).unwrap(), b"correct");
    assert!(
        std::fs::read_dir(staging.join("_state/archive-batches"))
            .unwrap()
            .next()
            .is_some(),
        "sentinel written"
    );

    // Correct size, different bytes: the batch is adopted, not re-extracted.
    std::fs::write(&output, b"ADOPTED").unwrap();
    install_single_file(dir.path(), &staging, None).await;
    assert_eq!(std::fs::read(&output).unwrap(), b"ADOPTED");

    // Wrong size (old nested-BSA bug) with a valid sentinel: re-extracted.
    std::fs::write(&output, b"whole outer archive blob").unwrap();
    install_single_file(dir.path(), &staging, None).await;
    assert_eq!(std::fs::read(&output).unwrap(), b"correct");
}

#[tokio::test]
async fn install_start_sweeps_only_stale_legacy_extract_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let tmp = dir.path().join("tmp");
    let age = |name: &str, days: u64| {
        let path = tmp.join(name);
        std::fs::create_dir_all(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/f"), b"x").unwrap();
        let old = SystemTime::now() - std::time::Duration::from_hours(days * 24);
        for p in [path.join("nested/f"), path.join("nested"), path.clone()] {
            std::fs::File::open(p).unwrap().set_modified(old).unwrap();
        }
        path
    };
    let stale = age("modde-extract-stale", 30);
    let live = age("modde-extract-live", 0);
    let unrelated = age("other-old-dir", 30);
    // Old root, fresh descendant: an install may still be writing here.
    let busy = age("modde-extract-busy", 30);
    std::fs::write(busy.join("nested/fresh"), b"y").unwrap();
    std::fs::File::open(busy.join("nested"))
        .unwrap()
        .set_modified(SystemTime::now())
        .unwrap();
    std::fs::File::open(&busy)
        .unwrap()
        .set_modified(SystemTime::now() - std::time::Duration::from_hours(720))
        .unwrap();

    install_single_file(dir.path(), &dir.path().join("staging"), Some(tmp)).await;

    assert!(!stale.exists(), "stale legacy dir swept");
    assert!(live.exists(), "recently used dir kept");
    assert!(busy.exists(), "dir with fresh descendant kept");
    assert!(unrelated.exists(), "non-extract dir untouched");
}

#[tokio::test]
async fn compressed_output_size_is_checked_from_frame_header() {
    let dir = tempfile::tempdir().unwrap();
    let staging = dir.path().join("staging");
    install_single_file(dir.path(), &staging, None).await;
    let output = staging.join("mods/r/file.txt");
    let compress = |data: &[u8]| {
        let mut enc = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
        enc.set_pledged_src_size(Some(data.len() as u64)).unwrap();
        std::io::Write::write_all(&mut enc, data).unwrap();
        enc.finish().unwrap()
    };
    std::fs::remove_file(&output).unwrap();
    let zst = compressed_path(&output);

    // Correct decoded size (7): adopted, not re-extracted.
    std::fs::write(&zst, compress(b"ADOPTED")).unwrap();
    install_single_file(dir.path(), &staging, None).await;
    assert!(!output.exists(), "correct compressed output adopted");

    // Wrong decoded size: batch invalid, re-extracted.
    std::fs::write(&zst, compress(b"whole outer archive blob")).unwrap();
    install_single_file(dir.path(), &staging, None).await;
    assert_eq!(std::fs::read(&output).unwrap(), b"correct");
}
