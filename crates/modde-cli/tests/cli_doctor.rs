//! End-to-end tests for `modde doctor` with a seeded profile database, a
//! `CrashLoggerSSE` log, and a mocked OpenAI-compatible endpoint.

#![cfg(feature = "bethesda")]

mod common;

use std::path::PathBuf;
use std::process::Output;

use common::Fixture;
use modde_core::installer::{InstallMethod, InstallPlan, InstallStatus, StagedFile};
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModId, ModdeDb};
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const GAME: &str = "skyrim-se";

const CRASH_LOG: &str = "Skyrim SSE v1.6.1170\n\
CrashLoggerSSE v1-14-1-0\n\
Unhandled exception \"EXCEPTION_ACCESS_VIOLATION\" at 0x7FF61D2A4B30\n\
\n\
PROBABLE CALL STACK:\n\
\t[0] badplugin.dll+0012F30\n\
\t[1] SkyrimSE.exe+05B11F0\n\
\n\
MODULES:\n\
\tSkyrimSE.exe\n\
\tbadplugin.dll\n\
\n\
SKSE PLUGINS:\n\
\tbadplugin.dll v1.2.3\n\
\n\
PLUGINS:\n\
\t[00] Skyrim.esm\n";

fn enabled(mod_id: &str) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(mod_id.to_string()),
        enabled: true,
        ..Default::default()
    }
}

/// Seed profile `main` with two mods; `bad-skse-plugin` ships `badplugin.dll`.
async fn seeded() -> Fixture {
    let fx = Fixture::new();
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let profile = Profile {
        id: None,
        name: "main".to_string(),
        game_id: GameId::from(GAME),
        source: ProfileSource::Manual,
        mods: vec![enabled("broken-mod"), enabled("bad-skse-plugin")],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: None,
    };
    let id = db.create_profile(&profile).await.unwrap();
    let plan = InstallPlan {
        method: InstallMethod::BareExtract,
        strip_prefix: None,
        source_archive_hash: "hash".to_string(),
        staged_files: vec![StagedFile {
            rel_path: "SKSE/Plugins/badplugin.dll".to_string(),
            origin_rel_path: "SKSE/Plugins/badplugin.dll".to_string(),
            size: 1,
            merge_group: None,
        }],
    };
    db.record_install(
        id,
        &ModId::from("bad-skse-plugin"),
        &plan,
        InstallStatus::Installed,
    )
    .await
    .unwrap();
    fx
}

fn crash_log(fx: &Fixture) -> String {
    let path = fx.root().join("crash.log");
    std::fs::write(&path, CRASH_LOG).unwrap();
    path.display().to_string()
}

fn doctor(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("doctor")
        .args(args)
        .output()
        .expect("spawn modde doctor")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn json_out(output: &Output) -> Value {
    serde_json::from_str(&ok(output)).expect("valid JSON")
}

#[tokio::test]
async fn profile_report_counts_seeded_state() {
    let fx = seeded().await;

    let text = ok(&doctor(
        &fx,
        &["profile", "--game", GAME, "--profile", "main"],
    ));
    assert!(text.contains("Doctor profile: main (skyrim-se)"), "{text}");
    assert!(text.contains("Mods: 2 enabled / 2 total"), "{text}");
    assert!(text.contains("Installed file records: 1"), "{text}");

    let report = json_out(&doctor(
        &fx,
        &["profile", "--game", GAME, "--profile", "main", "--json"],
    ));
    assert_eq!(report["profile_name"], "main");
    assert_eq!(report["mod_set"].as_array().unwrap().len(), 2);
    assert_eq!(report["crash"], Value::Null);
}

#[tokio::test]
async fn crash_report_correlates_the_dll_with_its_mod() {
    let fx = seeded().await;
    let log = crash_log(&fx);

    let text = ok(&doctor(
        &fx,
        &["crash", "--game", GAME, "--profile", "main", &log],
    ));
    assert!(text.contains("(crash-logger-sse, sha256"), "{text}");
    assert!(text.contains("Exception:"), "{text}");
    assert!(text.contains("bad-skse-plugin"), "{text}");
    assert!(text.contains("badplugin.dll"), "{text}");

    let report = json_out(&doctor(
        &fx,
        &[
            "crash",
            "--game",
            GAME,
            "--profile",
            "main",
            "--format",
            "crash-logger-sse",
            "--json",
            &log,
        ],
    ));
    assert!(
        report["crash"]["suspects"]
            .to_string()
            .contains("bad-skse-plugin")
    );
}

#[tokio::test]
async fn crash_and_profile_errors_are_reported() {
    let fx = seeded().await;

    let no_active = doctor(&fx, &["profile", "--game", GAME]);
    assert!(!no_active.status.success());
    assert!(String::from_utf8_lossy(&no_active.stderr).contains("no active profile"));

    let missing_log = doctor(
        &fx,
        &[
            "crash",
            "--game",
            GAME,
            "--profile",
            "main",
            "/nonexistent.log",
        ],
    );
    assert!(!missing_log.status.success());
    assert!(String::from_utf8_lossy(&missing_log.stderr).contains("failed to read crash log"));

    let no_log = doctor(&fx, &["crash", "--game", GAME, "--profile", "main"]);
    assert!(!no_log.status.success());
    assert!(String::from_utf8_lossy(&no_log.stderr).contains("no crash log found"));
}

#[tokio::test]
async fn crash_without_a_path_finds_the_latest_logger_output() {
    let fx = seeded().await;
    let skse = fx
        .home()
        .join("Documents/My Games/Skyrim Special Edition/SKSE");
    std::fs::create_dir_all(&skse).unwrap();
    std::fs::write(skse.join("crash-2026-10-22.log"), CRASH_LOG).unwrap();

    let text = ok(&doctor(
        &fx,
        &["crash", "--game", GAME, "--profile", "main"],
    ));
    assert!(text.contains("crash-2026-10-22.log"), "{text}");
    assert!(text.contains("bad-skse-plugin"), "{text}");
}

#[tokio::test]
async fn explain_keeps_only_hypotheses_citing_known_evidence() {
    let fx = seeded().await;
    let log = crash_log(&fx);
    let context = json_out(&doctor(
        &fx,
        &["crash", "--game", GAME, "--profile", "main", "--json", &log],
    ));
    let evidence_id = context["evidence"][0]["id"]
        .as_str()
        .expect("crash context has evidence")
        .to_string();

    let server = MockServer::start().await;
    let answer = json!({
        "hypotheses": [
            {
                "rank": 1, "title": "badplugin.dll crashes", "confidence": "high",
                "summary": "The crashing frame is in badplugin.dll.",
                "evidence_ids": [evidence_id], "recommended_checks": ["Disable bad-skse-plugin"]
            },
            {
                "rank": 2, "title": "Invented cause", "confidence": "low",
                "summary": "Not grounded.", "evidence_ids": ["NOT-AN-ID"], "recommended_checks": []
            }
        ],
        "unsupported": []
    });
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "content": answer.to_string() } }]
        })))
        .mount(&server)
        .await;
    let mut settings = AppSettings::default();
    settings.doctor.llm.local_endpoint = server.uri();
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));

    let text = ok(&doctor(
        &fx,
        &["explain", "--game", GAME, "--profile", "main", &log],
    ));
    assert!(text.contains("LLM-grounded hypotheses:"), "{text}");
    assert!(text.contains("badplugin.dll crashes"), "{text}");
    assert!(text.contains("checks: Disable bad-skse-plugin"), "{text}");
    assert!(text.contains("Unsupported hypotheses rejected:"), "{text}");
    assert!(text.contains("Invented cause"), "{text}");

    let explanation = json_out(&doctor(
        &fx,
        &[
            "explain",
            "--game",
            GAME,
            "--profile",
            "main",
            "--json",
            &log,
        ],
    ));
    assert_eq!(explanation["hypotheses"].as_array().unwrap().len(), 1);
}
