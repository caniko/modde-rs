//! End-to-end tests for `modde nexus status` and `modde nexus auth`
//! against a mock Nexus API.
//!
//! `MockServer` from wiremock listens on a random port; we point the
//! CLI at it via `MODDE_NEXUS_BASE_URL`. The CLI never reaches the
//! real Nexus, so these tests are deterministic and offline-safe.
//!
//! Covers:
//! * happy path — premium account renders "Premium"
//! * happy path — free account renders "Free"
//! * 401 — auth error is mapped to a user-friendly hint, not a raw HTTP error

mod common;

use common::Fixture;
use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn start_mock_validate(is_premium: bool) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .and(header("apikey", "test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "is_premium": is_premium,
            "name": "test-user",
        })))
        .mount(&server)
        .await;
    server
}

fn run_status(fx: &Fixture, base_url: &str) -> std::process::Output {
    fx.cmd()
        .env("MODDE_NEXUS_BASE_URL", base_url)
        .env("NEXUS_API_KEY", "test-key")
        .args(["nexus", "status"])
        .output()
        .expect("spawn modde")
}

#[tokio::test]
async fn nexus_status_premium_account() {
    let fx = Fixture::new();
    let server = start_mock_validate(true).await;

    let output = run_status(&fx, &server.uri());

    assert!(
        output.status.success(),
        "expected success; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Nexus API key: valid"),
        "missing 'valid' line in stdout:\n{stdout}"
    );
    assert!(
        stdout.contains("Premium"),
        "premium account should render 'Premium'; got:\n{stdout}"
    );
}

#[tokio::test]
async fn nexus_status_free_account() {
    let fx = Fixture::new();
    let server = start_mock_validate(false).await;

    let output = run_status(&fx, &server.uri());

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Free"),
        "free account should render 'Free'; got:\n{stdout}"
    );
}

#[tokio::test]
async fn nexus_status_rejects_invalid_key_with_friendly_message() {
    let fx = Fixture::new();
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/users/validate.json"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let output = run_status(&fx, &server.uri());

    assert!(!output.status.success(), "401 must surface as failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    // The CLI's error mapper rewrites raw HTTP errors into actionable
    // advice. Pin the user-facing wording so a future refactor that
    // drops the friendly message gets caught.
    assert!(
        stderr.contains("API key rejected") || stderr.contains("revoked"),
        "expected friendly auth-error message in stderr; got:\n{stderr}"
    );
}

fn run_auth(
    fx: &Fixture,
    base_url: &str,
    env_key: Option<&str>,
    stdin: &str,
) -> std::process::Output {
    let mut cmd = fx.cmd();
    cmd.env("MODDE_NEXUS_BASE_URL", base_url)
        .args(["nexus", "auth"])
        .write_stdin(stdin);
    if let Some(key) = env_key {
        cmd.env("NEXUS_API_KEY", key);
    }
    cmd.output().expect("spawn modde")
}

#[tokio::test]
async fn nexus_auth_validates_and_stores_the_key() {
    let fx = Fixture::new();
    let stored = fx.home().join(".config/modde/nexus_api_key");
    let premium = start_mock_validate(true).await;
    let free = start_mock_validate(false).await;

    let from_env = run_auth(&fx, &premium.uri(), Some("test-key"), "");
    let stdout = String::from_utf8_lossy(&from_env.stdout);
    assert!(from_env.status.success(), "{stdout}");
    assert!(
        stdout.contains("Loaded Nexus API key from NEXUS_API_KEY."),
        "{stdout}"
    );
    assert!(
        stdout.contains("API key validated and saved to"),
        "{stdout}"
    );
    assert!(stdout.contains("Account type: Premium"), "{stdout}");
    assert_eq!(std::fs::read_to_string(&stored).unwrap().trim(), "test-key");

    let prompted = run_auth(&fx, &free.uri(), None, "  test-key \n");
    let stdout = String::from_utf8_lossy(&prompted.stdout);
    assert!(prompted.status.success(), "{stdout}");
    assert!(stdout.contains("Enter your Nexus API key: "), "{stdout}");
    assert!(!stdout.contains("NEXUS_API_KEY"), "{stdout}");
    assert!(stdout.contains("Account type: Free"), "{stdout}");

    let empty = run_auth(&fx, &free.uri(), None, "\n");
    assert!(String::from_utf8_lossy(&empty.stderr).contains("API key cannot be empty"));
}

#[tokio::test]
async fn nexus_auth_explains_rate_limits_and_other_failures() {
    let fx = Fixture::new();
    for (status, expected) in [
        (429, "rate limit exceeded (HTTP 429)"),
        (401, "API key rejected by Nexus (HTTP 401/403)"),
        (500, "API key validation failed"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/users/validate.json"))
            .respond_with(ResponseTemplate::new(status))
            .mount(&server)
            .await;
        let output = run_auth(&fx, &server.uri(), Some("test-key"), "");
        assert!(!output.status.success());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains(expected), "{status}: {stderr}");
    }
    assert!(!fx.home().join(".config/modde/nexus_api_key").exists());
}
