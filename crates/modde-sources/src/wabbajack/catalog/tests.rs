use super::*;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::thread;

#[test]
fn official_catalog_parses_and_filters() {
    let json = r#"[{
          "title":"Legends of the Frost",
          "author":"Phoenix",
          "game":"skyrimspecialedition",
          "official":false,
          "tags":["Official","Lightweight"],
          "nsfw":false,
          "force_down":false,
          "links":{"download":"https://example/lotf.wabbajack","machineURL":"lotf"},
          "download_metadata":{"Size":10,"NumberOfArchives":2,"TotalSize":30},
          "version":"1.0"
        }]"#;
    let entries = parse_official_catalog(json).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].machine_url.as_deref(), Some("lotf"));
    let filtered = filter_entries(
        &entries,
        &CatalogFilter {
            query: Some("frost".into()),
            include_nsfw: false,
            include_down: false,
            ..Default::default()
        },
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn filter_entries_matches_normalized_wabbajack_game_names() {
    let entries = vec![WabbajackCatalogEntry {
        title: "Legends of the Frost".to_string(),
        game: Some("skyrimspecialedition".to_string()),
        author: None,
        version: None,
        tags: Vec::new(),
        image_url: None,
        readme_url: None,
        download_url: "https://example/lotf.wabbajack".to_string(),
        repository_name: None,
        machine_url: None,
        discord_url: None,
        website_url: None,
        official: true,
        nsfw: false,
        force_down: false,
        size: WabbajackSizeMetadata::default(),
        source: CatalogEntrySource::Official,
    }];

    let filtered = filter_entries(
        &entries,
        &CatalogFilter {
            game: Some("skyrim-se".to_string()),
            include_nsfw: false,
            include_down: false,
            ..Default::default()
        },
    );

    assert_eq!(filtered.len(), usize::from(cfg!(feature = "bethesda")));
}

#[test]
fn repositories_json_parses_named_catalog_sources() {
    let json = r#"{
          "wj-featured": "https://raw.githubusercontent.com/wabbajack-tools/mod-lists/master/modlists.json",
          "WakingDreams": "https://raw.githubusercontent.com/Oghma-Infinium/modlists/main/modlists.json"
        }"#;
    let repositories = parse_repositories(json).unwrap();
    assert_eq!(
        repositories.get("WakingDreams").map(String::as_str),
        Some("https://raw.githubusercontent.com/Oghma-Infinium/modlists/main/modlists.json")
    );
}

#[test]
fn repository_catalog_parses_twisted_skyrim_metadata() {
    let entries = parse_repository_catalog(Some("WakingDreams"), twisted_skyrim_json()).unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(entry.title, "Twisted Skyrim");
    assert_eq!(entry.repository_name.as_deref(), Some("WakingDreams"));
    assert_eq!(entry.machine_url.as_deref(), Some("TwistedSkyrim"));
    assert_eq!(entry.game.as_deref(), Some("skyrimspecialedition"));
    assert_eq!(entry.author.as_deref(), Some("TwistedModding"));
    assert_eq!(entry.version.as_deref(), Some("1.7.0.0"));
    assert!(entry.nsfw);
    assert!(!entry.official);
    assert_eq!(
        entry.image_url.as_deref(),
        Some(
            "https://raw.githubusercontent.com/Oghma-Infinium/Twisted-Skyrim/refs/heads/main/Twisted%20Skyrim%20Logo%20(1).webp"
        )
    );
    assert_eq!(
        entry.readme_url.as_deref(),
        Some("https://twistedskyrim.com/#installation")
    );
    assert_eq!(
        entry.download_url,
        "https://authored-files.wabbajack.org/Twisted Skyrim.wabbajack_2f42b496-5d7b-4588-bde0-b90a445a04cb"
    );
    assert_eq!(entry.size.modlist_size, Some(123));
    assert_eq!(entry.size.archive_count, Some(456));
    assert_eq!(entry.size.total_size, Some(789));
}

#[test]
fn repository_catalog_search_returns_metadata_entry() {
    let entries = parse_repository_catalog(Some("WakingDreams"), twisted_skyrim_json()).unwrap();
    let filtered = filter_entries(
        &entries,
        &CatalogFilter {
            query: Some("twisted".into()),
            include_nsfw: true,
            include_down: false,
            ..Default::default()
        },
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].source, CatalogEntrySource::Official);
    assert_eq!(filtered[0].author.as_deref(), Some("TwistedModding"));
    assert_eq!(filtered[0].machine_url.as_deref(), Some("TwistedSkyrim"));

    let by_repo_machine = find_entry(&entries, "WakingDreams/TwistedSkyrim").unwrap();
    assert_eq!(by_repo_machine.title, "Twisted Skyrim");
}

#[test]
fn authored_files_keep_only_wabbajack_rows() {
    let html = r#"
          <tr><td><a href="https://authored-files.wabbajack.org/List.wabbajack_abc">List.wabbajack abc</a></td><td>github/me</td></tr>
          <tr><td>Not a list.7z</td></tr>
        "#;
    let entries = parse_authored_files(html);
    assert_eq!(entries.len(), 1);
    assert!(entries[0].download_url.contains(".wabbajack"));
}

#[test]
fn authored_files_url_is_detected() {
    assert_eq!(
        authored_files_munged_name(
            "https://authored-files.wabbajack.org/Twisted%20Skyrim.wabbajack_abc"
        )
        .as_deref(),
        Some("Twisted Skyrim.wabbajack_abc")
    );
    assert_eq!(
        authored_files_munged_name(
            "https://build.wabbajack.org/authored_files/download/Twisted%20Skyrim.wabbajack_abc"
        )
        .as_deref(),
        Some("Twisted Skyrim.wabbajack_abc")
    );
    assert_eq!(
        authored_files_download_target(
            "https://example.test/authored_files/download/Twisted%20Skyrim.wabbajack_abc?x=1"
        )
        .as_ref()
        .map(|(base, munged)| (base.as_str(), munged.as_str())),
        Some((
            "https://example.test/authored_files",
            "Twisted Skyrim.wabbajack_abc",
        ))
    );
    assert!(authored_files_munged_name("https://example/lotf.wabbajack").is_none());
}

#[test]
fn authored_files_download_page_url_encodes_munged_name() {
    assert_eq!(
        authored_files_download_page_url(
            "https://build.wabbajack.org/authored_files",
            "Twisted Skyrim.wabbajack_abc"
        ),
        "https://build.wabbajack.org/authored_files/download/Twisted%20Skyrim.wabbajack_abc"
    );
}

#[test]
fn authored_files_download_page_parses_js_constants() {
    let page = parse_authored_files_download_page(
            r#"
            <script>
              const MUNGED_NAME = "Twisted Skyrim.wabbajack_abc";
              const FILE_NAME = "Twisted Skyrim.wabbajack";
              const FILE_SIZE_BYTES = 5;
              const PARTS = [{"Size":2,"Offset":0,"Hash":"a","Index":0},{"Size":3,"Offset":2,"Hash":"b","Index":1}];
            </script>
            "#,
        )
        .unwrap();

    assert_eq!(page.munged_name, "Twisted Skyrim.wabbajack_abc");
    assert_eq!(page.file_name, "Twisted Skyrim.wabbajack");
    assert_eq!(page.file_size_bytes, 5);
    assert_eq!(page.parts.len(), 2);
    assert_eq!(page.parts[1].index, 1);
}

#[test]
fn dedupe_prefers_first_matching_url() {
    let entry = WabbajackCatalogEntry {
        title: "A".into(),
        game: None,
        author: None,
        version: Some("1".into()),
        tags: Vec::new(),
        image_url: None,
        readme_url: None,
        download_url: "https://example/a.wabbajack".into(),
        repository_name: None,
        machine_url: Some("a".into()),
        discord_url: None,
        website_url: None,
        official: true,
        nsfw: false,
        force_down: false,
        size: WabbajackSizeMetadata::default(),
        source: CatalogEntrySource::Official,
    };
    let entries = deduplicate_entries(vec![entry.clone(), entry]);
    assert_eq!(entries.len(), 1);
}

#[test]
fn snippet_contains_hm_fields() {
    let snippet = format_hm_snippet(
        "lotf",
        "skyrim-se",
        Some(Path::new("/games/Skyrim")),
        "https://example/lotf.wabbajack",
        "sha256-abc",
    );
    assert!(snippet.contains("programs.modde.profiles.lotf"));
    assert!(snippet.contains("gameDir"));
    assert!(snippet.contains("sha256-abc"));
}

#[tokio::test]
async fn hm_snippet_for_local_file_uses_path_source() {
    let temp = tempfile::tempdir().unwrap();
    let modlist = temp.path().join("Legends of the Frost.wabbajack");
    tokio::fs::write(&modlist, b"fake modlist").await.unwrap();
    let client = reqwest::Client::new();

    let (snippet, cached_path) = hm_snippet_for_source(
        &client,
        &modlist.to_string_lossy(),
        "lotf",
        "skyrim-se",
        Some(Path::new("/games/Skyrim")),
        temp.path(),
    )
    .await
    .unwrap();

    assert_eq!(cached_path.as_deref(), Some(modlist.as_path()));
    assert!(snippet.contains("path = \""));
    assert!(snippet.contains("Legends of the Frost.wabbajack"));
    assert!(!snippet.contains("hash = "));
    assert!(!snippet.contains("url = "));
}

#[tokio::test]
async fn authored_files_downloader_assembles_parts() {
    let (base_url, request_count) = start_authored_files_test_server();
    let temp = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();

    let path = download_authored_wabbajack_file(
        &client,
        "Test List.wabbajack_abc",
        &format!("{base_url}/authored_files"),
        temp.path(),
    )
    .await
    .unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Test List.wabbajack")
    );
    assert_eq!(bytes, b"hello world");
    assert_eq!(request_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn download_wabbajack_file_uses_chunked_authored_download_page_url() {
    let (base_url, request_count) = start_authored_files_test_server();
    let temp = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let url = format!("{base_url}/authored_files/download/Test%20List.wabbajack_abc");

    let path = download_wabbajack_file(&client, &url, temp.path())
        .await
        .unwrap();

    let bytes = tokio::fs::read(&path).await.unwrap();
    assert_eq!(
        path.file_name().and_then(|name| name.to_str()),
        Some("Test List.wabbajack")
    );
    assert_eq!(bytes, b"hello world");
    assert_eq!(request_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn authored_files_resume_skips_completed_prefix_parts() {
    let (base_url, request_count) = start_authored_files_test_server();
    let temp = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let dest = temp.path().join("out.archive");
    tokio::fs::write(dest.with_extension("part"), b"hello")
        .await
        .unwrap();
    tokio::fs::write(
        dest.with_extension("part.json"),
        r#"{
              "munged_name":"Test List.wabbajack_abc",
              "file_size_bytes":11,
              "parts":[{"Size":5,"Offset":0,"Index":0},{"Size":6,"Offset":5,"Index":1}],
              "completed_parts":[0]
            }"#,
    )
    .await
    .unwrap();

    download_authored_file_to_path(
        &client,
        &format!("{base_url}/authored_files/download/Test%20List.wabbajack_abc"),
        &dest,
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(tokio::fs::read(&dest).await.unwrap(), b"hello world");
    assert!(!dest.with_extension("part.json").exists());
    assert_eq!(request_count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn authored_files_resume_discards_corrupt_partial_state() {
    let (base_url, request_count) = start_authored_files_test_server();
    let temp = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let dest = temp.path().join("out.archive");
    tokio::fs::write(dest.with_extension("part"), b"bad")
        .await
        .unwrap();
    tokio::fs::write(
        dest.with_extension("part.json"),
        r#"{
              "munged_name":"Test List.wabbajack_abc",
              "file_size_bytes":11,
              "parts":[{"Size":5,"Offset":0,"Index":0},{"Size":6,"Offset":5,"Index":1}],
              "completed_parts":[0]
            }"#,
    )
    .await
    .unwrap();

    download_authored_file_to_path(
        &client,
        &format!("{base_url}/authored_files/download/Test%20List.wabbajack_abc"),
        &dest,
        None,
        None,
    )
    .await
    .unwrap();

    assert_eq!(tokio::fs::read(&dest).await.unwrap(), b"hello world");
    assert_eq!(request_count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn hm_snippet_preserves_original_authored_files_url() {
    let (base_url, _request_count) = start_authored_files_test_server();
    let temp = tempfile::tempdir().unwrap();
    let client = reqwest::Client::new();
    let selected_url = "https://authored-files.wabbajack.org/Test List.wabbajack_abc";

    let path = download_authored_wabbajack_file(
        &client,
        &authored_files_munged_name(selected_url).unwrap(),
        &format!("{base_url}/authored_files"),
        temp.path(),
    )
    .await
    .unwrap();
    let hash = nix_sha256_sri(&path).await.unwrap();
    let snippet = format_hm_snippet("test", "skyrim-se", None, selected_url, &hash);

    assert!(
        snippet.contains("url = \"https://authored-files.wabbajack.org/Test List.wabbajack_abc\";")
    );
    assert!(snippet.contains("hash = \"sha256-"));
}

fn start_authored_files_test_server() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let request_count = Arc::new(AtomicUsize::new(0));
    let thread_count = Arc::clone(&request_count);
    thread::spawn(move || {
        for stream in listener.incoming().take(3) {
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
                "/authored_files/download/Test%20List.wabbajack_abc" => {
                    let body = r#"
                            <script>
                              const MUNGED_NAME = "Test List.wabbajack_abc";
                              const FILE_NAME = "Test List.wabbajack";
                              const FILE_SIZE_BYTES = 11;
                              const PARTS = [{"Size":5,"Offset":0,"Hash":"a","Index":0},{"Size":6,"Offset":5,"Hash":"b","Index":1}];
                            </script>
                        "#;
                    write_response(&mut stream, "200 OK", "text/html", body.as_bytes());
                }
                "/authored_files/Test%20List.wabbajack_abc/parts/0" => {
                    write_response(&mut stream, "200 OK", "application/octet-stream", b"hello");
                }
                "/authored_files/Test%20List.wabbajack_abc/parts/1" => {
                    write_response(&mut stream, "200 OK", "application/octet-stream", b" world");
                }
                _ => write_response(&mut stream, "404 Not Found", "text/plain", b"not found"),
            }
        }
    });
    (format!("http://{addr}"), request_count)
}

fn write_response(stream: &mut std::net::TcpStream, status: &str, content_type: &str, body: &[u8]) {
    let headers = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(headers.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
}

fn twisted_skyrim_json() -> &'static str {
    r#"[{
          "title":"Twisted Skyrim",
          "description":"A personal modlist for Skyrim Special Edition.",
          "author":"TwistedModding",
          "game":"skyrimspecialedition",
          "official":false,
          "tags":["NSFW","Combat"],
          "nsfw":true,
          "force_down":false,
          "links":{
            "image":"https://raw.githubusercontent.com/Oghma-Infinium/Twisted-Skyrim/refs/heads/main/Twisted%20Skyrim%20Logo%20(1).webp",
            "readme":"https://twistedskyrim.com/#installation",
            "download":"https://authored-files.wabbajack.org/Twisted Skyrim.wabbajack_2f42b496-5d7b-4588-bde0-b90a445a04cb",
            "machineURL":"TwistedSkyrim",
            "discordURL":"https://discord.gg/4WwqfK5yHg",
            "websiteURL":"https://twistedskyrim.com/#overview"
          },
          "download_metadata":{"Size":123,"NumberOfArchives":456,"TotalSize":789},
          "version":"1.7.0.0"
        }]"#
}
