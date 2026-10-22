//! End-to-end tests for `modde install mod` against a mock Nexus API.
//!
//! These exercise the failure paths the user is most likely to hit:
//! * malformed URL — parse error (offline)
//! * Nexus 404 on the mod info lookup — friendly error
//! * Nexus account is not Premium — the install bails before
//!   downloading anything
//!
//! The happy paths serve a full mocked Nexus API and CDN for a game with
//! no registered plugin, so the generic installer decides the layout and
//! the CLI's file selection, profile wiring and install records are
//! exercised end to end.

mod common;

use common::Fixture;
use serde_json::json;
use wiremock::matchers::{method, path, path_regex};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn run_install_mod(fx: &Fixture, base_url: &str, url: &str) -> std::process::Output {
    fx.cmd()
        .env("MODDE_NEXUS_BASE_URL", base_url)
        .env("MODDE_NEXUS_GRAPHQL_URL", base_url) // unused here, kept consistent
        .env("NEXUS_API_KEY", "test-key")
        .args(["install", "mod", url])
        .output()
        .expect("spawn modde")
}

#[test]
fn install_mod_rejects_non_nexus_url() {
    // No Nexus base URL needed — the URL parser bails before any HTTP.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .env("NEXUS_API_KEY", "test-key")
        .args(["install", "mod", "not a url"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "malformed URL must surface as failure"
    );
}

#[tokio::test]
async fn install_mod_surfaces_404_on_mod_lookup() {
    // file_id is provided so the handler skips the files.json call and
    // jumps straight to the get_mod lookup — which we make 404.
    let fx = Fixture::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/games/skyrimspecialedition/mods/99999.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let output = run_install_mod(
        &fx,
        &server.uri(),
        "https://www.nexusmods.com/skyrimspecialedition/mods/99999?tab=files&file_id=1",
    );
    assert!(!output.status.success(), "404 must surface as failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to fetch mod info") || stderr.contains("404"),
        "expected mod-info failure context in stderr; got:\n{stderr}"
    );
}

#[tokio::test]
async fn install_mod_bails_on_non_premium_account() {
    // Mock the entire pre-download pipeline up to generate_download_link.
    // generate_download_link calls auth::check_premium first; we make
    // validate.json say the account is free, which must short-circuit
    // the install with a "Premium required" error.
    let fx = Fixture::new();
    let server = MockServer::start().await;

    // Mod info — needed by handle_single_mod for the display name.
    Mock::given(method("GET"))
        .and(path("/games/skyrimspecialedition/mods/12604.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mod_id": 12604,
            "name": "SkyUI",
            "summary": null,
            "version": "5.2",
            "author": "SkyUI Team",
        })))
        .mount(&server)
        .await;

    // Premium gate — non-premium accounts are blocked from automated
    // downloads. Pin the error message so the user-facing wording is
    // tested, not just the exit code.
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "is_premium": false,
            "name": "test-user",
        })))
        .mount(&server)
        .await;

    let output = run_install_mod(
        &fx,
        &server.uri(),
        "https://www.nexusmods.com/skyrimspecialedition/mods/12604?tab=files&file_id=35834",
    );
    assert!(
        !output.status.success(),
        "non-premium accounts must not be allowed to download"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Premium"),
        "expected 'Premium' in error message; got:\n{stderr}"
    );
}

/// A Nexus game with no registered plugin, so the generic installer decides.
const GAME: &str = "unlistedgame";

fn zip(entries: &[&str]) -> Vec<u8> {
    use std::io::Write as _;

    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    for name in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
    }
    zip.finish().unwrap();
    cursor.into_inner()
}

fn nexus_file(file_id: u64, category: &str, uploaded: u64) -> serde_json::Value {
    json!({
        "file_id": file_id, "name": "file", "version": "1.0", "size_kb": 1,
        "file_name": "file.zip", "category_name": category, "uploaded_timestamp": uploaded,
    })
}

/// Serve mod info, its file list, a Premium account, and `archive` as the
/// CDN download for every file of `mod_id`.
async fn mock_nexus(server: &MockServer, mod_id: u64, files: serde_json::Value, archive: Vec<u8>) {
    let mod_path = format!("/games/{GAME}/mods/{mod_id}");
    Mock::given(method("GET"))
        .and(path(format!("{mod_path}.json")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "mod_id": mod_id, "name": "Test Mod", "summary": null, "version": "1.0", "author": "a",
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{mod_path}/files.json")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "files": files })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "is_premium": true })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path_regex(format!(
            r"^{mod_path}/files/\d+/download_link\.json$"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!([{ "URI": format!("{}/cdn/{mod_id}.zip", server.uri()) }])),
        )
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/cdn/{mod_id}.zip")))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(archive))
        .mount(server)
        .await;
}

fn mod_url(mod_id: u64) -> String {
    format!("https://www.nexusmods.com/{GAME}/mods/{mod_id}")
}

fn stdout(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "install failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn install_mod_installs_the_newest_main_file_and_records_it() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    let files = json!([
        nexus_file(1, "MAIN", 100),
        nexus_file(2, "MAIN", 200),
        nexus_file(3, "OPTIONAL", 300),
    ]);
    mock_nexus(&server, 100, files, zip(&["version.dll"])).await;

    let out = stdout(&run_install_mod(&fx, &server.uri(), &mod_url(100)));
    assert!(out.contains("file 2)"), "{out}");
    assert!(out.contains("Staged 1 files into"), "{out}");
    assert!(out.contains("Mod 'unlistedgame_100_2' added to profile 'unlistedgame'"));
    assert!(
        fx.data_dir()
            .join("store/unlistedgame_100_2/__dll_overlay__/version.dll")
            .is_file()
    );

    let again = stdout(&run_install_mod(&fx, &server.uri(), &mod_url(100)));
    assert!(
        again.contains("Already in store, skipping download"),
        "{again}"
    );

    let db = modde_core::ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let profile = db
        .load_profile(GAME, &modde_core::GameId::from(GAME))
        .await
        .unwrap();
    assert_eq!(
        profile.mods.len(),
        1,
        "a reinstall must not duplicate the mod"
    );
    assert_eq!(
        profile.mods[0].install_status,
        Some(modde_core::installer::InstallStatus::Installed)
    );
    let installed = db
        .installed_files_for_mod(
            profile.id.unwrap(),
            &modde_core::ModId::from("unlistedgame_100_2"),
        )
        .await
        .unwrap();
    assert_eq!(installed.len(), 1);
}

#[tokio::test]
async fn install_mod_falls_back_to_the_first_file_without_main() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(
        &server,
        110,
        json!([nexus_file(7, "OPTIONAL", 1)]),
        zip(&["version.dll"]),
    )
    .await;

    let out = stdout(&run_install_mod(&fx, &server.uri(), &mod_url(110)));
    assert!(out.contains("Mod 'unlistedgame_110_7' added"), "{out}");
}

#[tokio::test]
async fn install_mod_fails_when_the_mod_has_no_files() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(&server, 120, json!([]), zip(&["version.dll"])).await;

    let output = run_install_mod(&fx, &server.uri(), &mod_url(120));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no files found for mod 120"));
}

#[tokio::test]
async fn install_mod_reports_pending_and_unknown_layouts() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(
        &server,
        130,
        json!([nexus_file(1, "MAIN", 1)]),
        zip(&["fomod/ModuleConfig.xml"]),
    )
    .await;
    mock_nexus(
        &server,
        131,
        json!([nexus_file(1, "MAIN", 1)]),
        zip(&["readme.txt"]),
    )
    .await;

    let pending = stdout(&run_install_mod(&fx, &server.uri(), &mod_url(130)));
    assert!(
        pending.contains("Install method 'fomod' needs user input"),
        "{pending}"
    );

    let unknown = stdout(&run_install_mod(&fx, &server.uri(), &mod_url(131)));
    assert!(unknown.contains("unknown install layout"), "{unknown}");
    assert!(
        unknown.contains("unknown-installers/unlistedgame_131_1"),
        "{unknown}"
    );
}

#[tokio::test]
async fn install_mod_warns_when_adding_to_a_locked_profile() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(
        &server,
        140,
        json!([nexus_file(1, "MAIN", 1)]),
        zip(&["a.dll"]),
    )
    .await;
    mock_nexus(
        &server,
        141,
        json!([nexus_file(1, "MAIN", 1)]),
        zip(&["b.dll"]),
    )
    .await;
    stdout(&run_install_mod(&fx, &server.uri(), &mod_url(140)));

    let lock = fx
        .cmd()
        .args(["profile", "lock", GAME])
        .output()
        .expect("spawn modde");
    stdout(&lock);

    let output = run_install_mod(&fx, &server.uri(), &mod_url(141));
    stdout(&output);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is locked"),
        "expected a lock warning"
    );
}

fn modde(fx: &Fixture, args: &[&str]) -> std::process::Output {
    fx.cmd().args(args).output().expect("spawn modde")
}

#[tokio::test]
async fn installed_mods_can_be_removed_and_unknown_ones_diagnosed() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(
        &server,
        150,
        json!([nexus_file(1, "MAIN", 1)]),
        zip(&["version.dll"]),
    )
    .await;
    mock_nexus(
        &server,
        151,
        json!([nexus_file(1, "MAIN", 1)]),
        zip(&["readme.txt"]),
    )
    .await;

    // Nothing to diagnose before any unknown layout was installed.
    assert!(
        !modde(&fx, &["mod", "diagnose", "unlistedgame_151"])
            .status
            .success()
    );

    stdout(&run_install_mod(&fx, &server.uri(), &mod_url(150)));
    stdout(&run_install_mod(&fx, &server.uri(), &mod_url(151)));

    let diagnose = stdout(&modde(&fx, &["mod", "diagnose", "unlistedgame_151"]));
    assert!(diagnose.contains("----- PROMPT.md -----"), "{diagnose}");
    assert!(diagnose.contains("Critical files"), "{diagnose}");
    assert!(
        !modde(&fx, &["mod", "diagnose", "unlistedgame_999"])
            .status
            .success()
    );

    let store = fx.data_dir().join("store/unlistedgame_150_1");
    let dry_run = stdout(&modde(
        &fx,
        &[
            "mod",
            "remove",
            "unlistedgame_150_1",
            "--profile",
            GAME,
            "--dry-run",
        ],
    ));
    assert!(
        dry_run.contains("No record-level save dependency analyzer"),
        "{dry_run}"
    );
    assert!(store.is_dir(), "a dry run must not remove anything");

    let removed = stdout(&modde(
        &fx,
        &["mod", "remove", "unlistedgame_150_1", "--profile", GAME],
    ));
    assert!(
        removed.contains(
            "Removed 'unlistedgame_150_1' from profile 'unlistedgame' (1 staged files tracked)."
        ),
        "{removed}"
    );
    assert!(!store.exists());

    let db = modde_core::ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let profile = db
        .load_profile(GAME, &modde_core::GameId::from(GAME))
        .await
        .unwrap();
    let mods: Vec<&str> = profile.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(mods, ["unlistedgame_151_1"]);

    stdout(&modde(&fx, &["profile", "lock", GAME]));
    let locked = modde(
        &fx,
        &["mod", "remove", "unlistedgame_151_1", "--profile", GAME],
    );
    assert!(!locked.status.success());
    assert!(String::from_utf8_lossy(&locked.stderr).contains("is locked"));
}

/// Report `mod_id` as having a file newer than any install timestamp.
async fn mock_updated(server: &MockServer, mod_id: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/games/{GAME}/mods/updated.json")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "mod_id": mod_id, "latest_file_update": 4_102_444_800_u64, "latest_mod_activity": 4_102_444_800_u64,
        }])))
        .mount(server)
        .await;
}

#[tokio::test]
async fn mods_installed_by_url_are_tracked_for_updates() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    let files = json!([nexus_file(1, "MAIN", 100), nexus_file(2, "MAIN", 200)]);
    mock_nexus(&server, 170, files, zip(&["version.dll"])).await;
    mock_updated(&server, 170).await;
    let pinned = format!("{}?tab=files&file_id=1", mod_url(170));
    stdout(&run_install_mod(&fx, &server.uri(), &pinned));

    let update = |args: &[&str]| {
        fx.cmd()
            .env("MODDE_NEXUS_BASE_URL", server.uri())
            .env("NEXUS_API_KEY", "test-key")
            .arg("update")
            .args(args)
            .args(["--profile", GAME])
            .output()
            .expect("spawn modde update")
    };

    let check = stdout(&update(&["check", "--mods"]));
    assert!(
        check.contains("Checking updates for 1 tracked mods"),
        "{check}"
    );
    assert!(
        check.contains("unlistedgame_170_1 (installed: 1.0"),
        "{check}"
    );

    let dry_run = stdout(&update(&["apply", "--dry-run"]));
    assert!(
        dry_run.contains("would update unlistedgame_170_1"),
        "{dry_run}"
    );

    let applied = stdout(&update(&["apply"]));
    assert!(
        applied.contains("Updating unlistedgame_170_1 → file 2"),
        "{applied}"
    );
    assert!(
        applied.contains("Applied 1 update(s); 0 failure(s); 0 unchanged."),
        "{applied}"
    );

    let db = modde_core::ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let profile = db
        .load_profile(GAME, &modde_core::GameId::from(GAME))
        .await
        .unwrap();
    let mods: Vec<&str> = profile.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(mods, ["unlistedgame_170_2"]);

    let again = stdout(&update(&["apply"]));
    assert!(
        again.contains("Applied 0 update(s); 0 failure(s); 1 unchanged."),
        "{again}"
    );
}
