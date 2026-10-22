//! End-to-end dispatch tests for `modde nxm`.
//!
//! These exercise the path from clap parsing into [`commands::nxm`]
//! without making any network calls. The validation tests fail before
//! the handler ever reaches `NexusApi`, so they are deterministic and
//! offline-safe. The download tests serve a mocked Nexus API and CDN.

mod common;

use common::Fixture;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn nxm_handle_requires_uri() {
    // Positional `<uri>` is required — clap must reject the bare
    // subcommand with a non-zero exit and a usage hint on stderr.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "modde nxm handle without a URI must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("<URI>") || stderr.contains("required"),
        "stderr should mention the missing URI; got:\n{stderr}"
    );
}

#[test]
fn nxm_handle_rejects_non_nxm_scheme() {
    // The handler's first step is `NxmUri::parse`, which bails on
    // anything that isn't an `nxm://` URI. This must surface as a
    // process failure, not a silent success.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "https://nexusmods.com/skyrim/mods/1"])
        .output()
        .expect("spawn modde");
    assert!(!output.status.success(), "non-nxm scheme must be rejected");
}

#[test]
fn nxm_handle_rejects_malformed_path() {
    // Path must look like `<domain>/mods/<id>/files/<id>`. Garbage in
    // the path is rejected before any Nexus API call is made.
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "nxm://skyrim/garbage/path"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "malformed nxm:// URI must be rejected"
    );
}

#[test]
fn nxm_handle_rejects_non_numeric_ids() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "nxm://skyrim/mods/abc/files/def"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "non-numeric mod_id/file_id must be rejected"
    );
}

/// Serve a Premium account, mod 7's info and files, and its CDN download.
async fn mock_nexus(server: &MockServer, file_name: &str) {
    for (route, body) in [
        ("/users/validate.json", json!({ "is_premium": true })),
        (
            "/games/stardewvalley/mods/7.json",
            json!({ "mod_id": 7, "name": "Probe", "summary": null, "version": "1.2", "author": "me" }),
        ),
        (
            "/games/stardewvalley/mods/7/files.json",
            json!({ "files": [{
                "file_id": 8, "name": "main", "version": "1.2", "size_kb": 1,
                "file_name": file_name, "category_name": "MAIN", "uploaded_timestamp": 1,
            }] }),
        ),
        (
            "/games/stardewvalley/mods/7/files/8/download_link.json",
            json!([{ "URI": format!("{}/cdn/archive", server.uri()) }]),
        ),
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/cdn/archive"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(b"archive".to_vec()))
        .mount(server)
        .await;
}

fn nxm_handle(fx: &Fixture, server: &MockServer, uri: &str) -> std::process::Output {
    fx.cmd()
        .env("MODDE_NEXUS_BASE_URL", server.uri())
        .env("NEXUS_API_KEY", "test-key")
        .args(["nxm", "handle", uri])
        .output()
        .expect("spawn modde")
}

#[tokio::test]
async fn nxm_handle_downloads_under_the_sanitized_file_name() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(&server, "../../Probe 1.2.zip").await;

    let output = nxm_handle(
        &fx,
        &server,
        "nxm://stardewvalley/mods/7/files/8?key=k&expires=1",
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(stdout.contains("Mod: Probe by me"), "{stdout}");
    assert!(stdout.contains("Version: 1.2"), "{stdout}");
    assert!(
        stdout.contains("Install with: modde install mod"),
        "{stdout}"
    );
    let dest = fx.data_dir().join("downloads/Probe 1.2.zip");
    assert_eq!(std::fs::read(dest).unwrap(), b"archive");
}

#[tokio::test]
async fn nxm_handle_falls_back_to_ids_for_an_unusable_file_name() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    mock_nexus(&server, "..").await;

    let output = nxm_handle(&fx, &server, "nxm://stardewvalley/mods/7/files/8");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read(fx.data_dir().join("downloads/7_8.zip")).unwrap(),
        b"archive"
    );
}

#[test]
fn nxm_handle_requires_an_api_key() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["nxm", "handle", "nxm://stardewvalley/mods/7/files/8"])
        .output()
        .expect("spawn modde");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Mod ID: 7"));
}
