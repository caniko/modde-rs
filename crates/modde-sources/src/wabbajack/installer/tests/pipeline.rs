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
