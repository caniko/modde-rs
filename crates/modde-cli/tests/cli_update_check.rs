//! End-to-end tests for `modde update check`.
//!
//! Covers the no-tracked-mods short-circuit (offline) and modde's own
//! release check against a mocked release endpoint. The "updates
//! available" branch for mods needs a profile seeded with mods that have
//! `nexus_mod_id` set — that's reachable through `install mod` (covered by
//! `cli_install_mod.rs`) but seeding it here would require a real install,
//! which is too much for a unit-style integration test.

mod common;

use common::Fixture;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn create_profile(fx: &Fixture, name: &str, game: &str) {
    let output = fx
        .cmd()
        .args(["profile", "create", name, "--game", game])
        .output()
        .expect("spawn modde profile create");
    assert!(
        output.status.success(),
        "profile create failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn update_check_short_circuits_when_no_tracked_mods() {
    // Empty profile → no Nexus calls → no API key needed → fully offline.
    let fx = Fixture::new();
    create_profile(&fx, "empty", "skyrim-se");

    let output = fx
        .cmd()
        .args([
            "update",
            "check",
            "--profile",
            "empty",
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("spawn modde update check");

    assert!(
        output.status.success(),
        "expected success on empty profile; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No mods with Nexus metadata"),
        "expected the no-tracked-mods short-circuit message; got:\n{stdout}"
    );
}

/// Answer the latest-release query at `/{route}`, expecting `hits` requests.
async fn release(server: &MockServer, route: &str, response: ResponseTemplate, hits: u64) {
    Mock::given(method("GET"))
        .and(path(format!("/{route}")))
        .respond_with(response)
        .expect(hits)
        .mount(server)
        .await;
}

/// A `modde` invocation whose product update check queries `/{route}`.
fn checking(fx: &Fixture, server: &MockServer, route: &str) -> assert_cmd::Command {
    let mut cmd = fx.cmd();
    cmd.env_remove("MODDE_NO_UPDATE_CHECK").env(
        "MODDE_UPDATE_CHECK_URL",
        format!("{}/{route}", server.uri()),
    );
    cmd
}

fn run(mut cmd: assert_cmd::Command, args: &[&str]) -> std::process::Output {
    cmd.args(args).output().expect("spawn modde")
}

#[tokio::test]
async fn product_check_reports_newer_releases_and_caches_them() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    let newer = json!({ "tag_name": "v99.0.0", "html_url": "https://example.test/r" });
    release(
        &server,
        "latest",
        ResponseTemplate::new(200).set_body_json(newer),
        1,
    )
    .await;

    let check = run(checking(&fx, &server, "latest"), &["update", "check"]);
    assert!(check.status.success());
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(
        stdout.contains("modde 99.0.0 is available (current: "),
        "{stdout}"
    );
    assert!(stdout.contains("https://example.test/r"), "{stdout}");
    assert!(fx.home().join(".cache/modde/update-check.json").is_file());

    // Read-only commands reuse the fresh cache instead of asking again.
    let diagnostics = fx.root().join("diagnostics");
    std::fs::create_dir_all(&diagnostics).unwrap();
    let notice = run(
        checking(&fx, &server, "latest"),
        &[
            "wabbajack",
            "analyze-diagnostics",
            diagnostics.to_str().unwrap(),
        ],
    );
    assert!(notice.status.success());
    assert!(
        String::from_utf8_lossy(&notice.stderr)
            .contains("modde update available: 99.0.0 (current: "),
        "{}",
        String::from_utf8_lossy(&notice.stderr)
    );
    server.verify().await;
}

#[tokio::test]
async fn product_check_handles_current_disabled_and_broken_releases() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    let ok = |tag: &str| ResponseTemplate::new(200).set_body_json(json!({ "tag_name": tag }));
    release(&server, "current", ok("v0.0.1"), 1).await;
    release(&server, "nightly", ok("nightly"), 1).await;
    release(&server, "broken", ResponseTemplate::new(500), 1).await;

    let current = run(checking(&fx, &server, "current"), &["update", "check"]);
    assert!(String::from_utf8_lossy(&current.stdout).contains("is up to date."));
    let nightly = run(checking(&fx, &server, "nightly"), &["update", "check"]);
    assert!(String::from_utf8_lossy(&nightly.stderr).contains("invalid latest release version"));
    let broken = run(checking(&fx, &server, "broken"), &["update", "check"]);
    assert!(
        String::from_utf8_lossy(&broken.stderr)
            .contains("latest modde release endpoint returned an error")
    );

    // Opting out, by environment or by setting, never reaches the server.
    let disabled = run(fx.cmd(), &["update", "check"]);
    assert!(String::from_utf8_lossy(&disabled.stdout).contains("update checks are disabled"));
    let mut settings = modde_core::settings::AppSettings::default();
    settings.update_check.enabled = false;
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    let off = run(checking(&fx, &server, "current"), &["update", "check"]);
    assert!(String::from_utf8_lossy(&off.stdout).contains("update checks are disabled"));
    server.verify().await;
}
