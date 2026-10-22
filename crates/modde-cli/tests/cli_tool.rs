//! End-to-end tests for `modde tool` against a fake Cyberpunk 2077 install.
//!
//! Everything runs offline: tool availability is only reported, and the
//! `ReShade` / `OptiScaler` payloads come from local fixture directories.
//! `tool releases` is not covered because it always queries GitHub.

#![cfg(all(unix, feature = "cyberpunk", feature = "linux-integrations"))]

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::Fixture;
use modde_core::GameId;
use modde_core::settings::AppSettings;
use serde_json::Value;

const GAME: &str = "cyberpunk2077";

/// Point the fixture's settings at a fake install with Cyberpunk's
/// `bin/x64` executable directory.
fn sandbox() -> (Fixture, PathBuf) {
    let fx = Fixture::new();
    let install = fx.root().join("install");
    std::fs::create_dir_all(install.join("bin/x64")).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install.clone());
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    (fx, install)
}

fn tool(fx: &Fixture, args: &[&str]) -> Output {
    // `--game` goes right after the subcommand so it never lands after `--`.
    let (subcommand, rest) = args.split_first().expect("tool subcommand");
    let mut cmd = fx.cmd();
    cmd.args(["tool", subcommand]);
    if !args.contains(&"--game") {
        cmd.args(["--game", GAME]);
    }
    cmd.args(rest).output().expect("spawn modde tool")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn json(output: &Output) -> Value {
    serde_json::from_str(&ok(output)).expect("valid JSON output")
}

fn write_files(dir: &Path, files: &[&str]) {
    for file in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"x").unwrap();
    }
}

fn files_under(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = walkdir(dir)
        .into_iter()
        .map(|p| p.strip_prefix(dir).unwrap().display().to_string())
        .collect();
    files.sort();
    files
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            out.extend(walkdir(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[test]
fn list_and_status_describe_the_install() {
    let (fx, install) = sandbox();

    let list = ok(&tool(&fx, &["list"]));
    assert!(
        list.contains(&format!("Install: {}", install.display())),
        "{list}"
    );
    assert!(list.contains("Detected tools:"), "{list}");

    let status = ok(&tool(&fx, &["status"]));
    for name in [
        "MangoHud",
        "vkBasalt",
        "GameMode",
        "ReShade",
        "OptiScaler",
        "Proton",
    ] {
        assert!(status.contains(name), "{name} missing from:\n{status}");
    }
    assert!(status.contains("OptiScaler install: absent"), "{status}");
}

#[test]
fn show_and_settings_report_effective_values() {
    let (fx, install) = sandbox();

    let show = json(&tool(&fx, &["show", "mangohud", "--json"]));
    assert_eq!(show["game"], GAME);
    assert_eq!(show["enabled"], false);
    assert_eq!(show["installDir"], install.display().to_string());
    assert_eq!(show["effectiveSettings"]["position"], "top-left");
    assert!(ok(&tool(&fx, &["show", "mangohud"])).contains("MangoHud"));

    let settings = json(&tool(&fx, &["settings", "mangohud", "--json"]));
    let keys: Vec<&str> = settings["settings"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|s| s["key"].as_str())
        .collect();
    assert!(keys.contains(&"fps_limit") && keys.contains(&"position"));
    assert!(ok(&tool(&fx, &["settings", "mangohud"])).contains("fps_limit"));
}

#[test]
fn enable_configure_reset_and_disable_round_trip() {
    let (fx, _install) = sandbox();
    let conf = fx.data_dir().join("tools").join(GAME).join("MangoHud.conf");

    assert!(ok(&tool(&fx, &["enable", "mangohud"])).contains("Enabled MangoHud"));
    assert!(conf.is_file());

    ok(&tool(
        &fx,
        &[
            "configure",
            "mangohud",
            "--",
            "fps_limit=60",
            "position=top-right",
        ],
    ));
    let show = json(&tool(&fx, &["show", "mangohud", "--json"]));
    assert_eq!(show["enabled"], true);
    assert_eq!(show["effectiveSettings"]["position"], "top-right");
    assert!(
        std::fs::read_to_string(&conf)
            .unwrap()
            .contains("fps_limit=60")
    );

    assert!(
        err(&tool(&fx, &["configure", "mangohud", "--", "nope=1"]))
            .contains("unknown setting 'nope'")
    );
    assert!(
        err(&tool(
            &fx,
            &["configure", "mangohud", "--", "position=middle"]
        ))
        .contains("must be one of")
    );
    ok(&tool(
        &fx,
        &["configure", "mangohud", "--reset-key", "position"],
    ));
    let show = json(&tool(&fx, &["show", "mangohud", "--json"]));
    assert_eq!(show["effectiveSettings"]["position"], "top-left");

    ok(&tool(&fx, &["disable", "mangohud"]));
    assert_eq!(
        json(&tool(&fx, &["show", "mangohud", "--json"]))["enabled"],
        false
    );
}

#[test]
fn reshade_preview_apply_and_revert() {
    let (fx, install) = sandbox();
    let bin = install.join("bin/x64");

    // Without a source the plan reports missing inputs and apply refuses.
    assert!(
        ok(&tool(&fx, &["preview", "reshade"]))
            .contains("reshade: source directory is not configured")
    );
    assert!(ok(&tool(&fx, &["apply", "reshade", "--dry-run"])).contains("Missing inputs"));
    assert!(err(&tool(&fx, &["apply", "reshade"])).contains("'source_dir' setting is required"));

    let source = fx.root().join("reshade");
    write_files(&source, &["dxgi.dll", "ReShade.ini"]);
    let source_arg = format!("source_dir={}", source.display());
    ok(&tool(&fx, &["configure", "reshade", "--", &source_arg]));

    assert!(ok(&tool(&fx, &["preview", "reshade"])).contains("Changed files: 2"));
    assert!(ok(&tool(&fx, &["apply", "reshade"])).contains("Applied ReShade (2 files)"));
    assert_eq!(files_under(&bin), ["ReShade.ini", "dxgi.dll"]);
    assert!(
        ok(&tool(&fx, &["apply", "reshade", "--dry-run"]))
            .contains("All 2 planned file(s) already current")
    );

    assert!(ok(&tool(&fx, &["revert", "reshade"])).contains("Reverted ReShade (2 files)"));
    assert_eq!(files_under(&bin), [] as [&str; 0]);
    assert!(ok(&tool(&fx, &["revert", "reshade"])).contains("No applied files to revert"));
}

#[test]
fn mangohud_has_no_game_files_to_apply() {
    let (fx, _install) = sandbox();
    assert!(ok(&tool(&fx, &["apply", "mangohud"])).contains("No files to apply for MangoHud"));
    assert!(
        ok(&tool(&fx, &["apply", "mangohud", "--dry-run"]))
            .contains("All 0 planned file(s) already current")
    );
}

#[test]
fn optiscaler_inspection_commands() {
    let (fx, _install) = sandbox();

    let diagnose = json(&tool(&fx, &["diagnose", "optiscaler", "--json"]));
    assert_eq!(diagnose["tool"], "optiscaler");
    assert_eq!(diagnose["installStatus"], "absent");
    assert!(ok(&tool(&fx, &["diagnose", "optiscaler"])).contains("absent"));

    let profiles = json(&tool(&fx, &["profiles", "optiscaler", "--json"]));
    assert!(
        profiles.to_string().contains("community-dxgi"),
        "{profiles}"
    );
    assert!(ok(&tool(&fx, &["profiles", "optiscaler"])).contains("community-dxgi"));

    let sources = json(&tool(&fx, &["sources", "optiscaler", "--json"]));
    assert_eq!(sources["currentSourceDir"], Value::Null);
    ok(&tool(&fx, &["sources", "optiscaler"]));
}

#[test]
fn doctor_reports_every_tool_and_single_tools() {
    let (fx, install) = sandbox();
    ok(&tool(&fx, &["enable", "mangohud"]));

    let doctor = json(&tool(&fx, &["doctor", "--json"]));
    assert_eq!(doctor["installDir"], install.display().to_string());
    let reports = doctor["reports"].as_array().unwrap();
    assert!(
        reports
            .iter()
            .any(|r| r["displayName"] == "MangoHud" && r["enabled"] == true)
    );

    let single = ok(&tool(&fx, &["doctor", "mangohud"]));
    assert!(
        single.contains("Tool doctor for Cyberpunk 2077"),
        "{single}"
    );
    assert!(single.contains("mangohud:"), "{single}");
}

#[test]
fn optiscaler_local_setup_applies_and_reverts() {
    let (fx, install) = sandbox();
    let source = fx.root().join("optiscaler");
    write_files(
        &source,
        &[
            "OptiScaler.dll",
            "OptiScaler.ini",
            "OptiPatcher.asi",
            "FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll",
            "FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll",
        ],
    );
    let source = source.display().to_string();
    let local = ["--source", "local", "--local-source-dir", &source];

    let dry_run = ok(&tool(
        &fx,
        &[&["setup", "optiscaler", "--dry-run"][..], &local].concat(),
    ));
    assert!(dry_run.contains("[dry-run] OptiScaler setup"), "{dry_run}");
    assert_eq!(files_under(&install.join("bin/x64")), [] as [&str; 0]);

    let applied = ok(&tool(
        &fx,
        &[&["setup", "optiscaler", "--apply", "--yes"][..], &local].concat(),
    ));
    assert!(
        applied.contains("Applied OptiScaler (4 files)"),
        "{applied}"
    );
    assert_eq!(
        files_under(&install.join("bin/x64")),
        [
            "OptiScaler.ini",
            "amd_fidelityfx_upscaler_dx12.dll",
            "dxgi.dll",
            "plugins/OptiPatcher.asi",
        ]
    );
    let sources = json(&tool(&fx, &["sources", "optiscaler", "--json"]));
    assert_eq!(sources["currentSourceDir"], source);

    assert!(ok(&tool(&fx, &["revert", "optiscaler"])).contains("Reverted OptiScaler (4 files)"));
}

#[test]
fn optiscaler_setup_refuses_incomplete_sources() {
    let (fx, _install) = sandbox();
    let source = fx.root().join("partial");
    write_files(&source, &["OptiScaler.dll", "OptiScaler.ini"]);
    let source = source.display().to_string();

    let stderr = err(&tool(
        &fx,
        &[
            "setup",
            "optiscaler",
            "--source",
            "local",
            "--local-source-dir",
            &source,
            "--dry-run",
        ],
    ));
    assert!(stderr.contains("setup has missing inputs"), "{stderr}");
}

#[test]
fn rejects_unknown_tools_games_and_missing_installs() {
    let (fx, _install) = sandbox();
    assert!(err(&tool(&fx, &["show", "nope"])).contains("unknown tool: 'nope'"));
    assert!(
        err(&tool(&fx, &["show", "mangohud", "--game", "not-a-game"]))
            .contains("unsupported game: 'not-a-game'")
    );
    assert!(
        err(&tool(&fx, &["show", "mangohud", "--game", "witcher3"]))
            .contains("could not detect install dir")
    );
    assert!(err(&tool(&fx, &["apply", "nope"])).contains("unknown tool: 'nope'"));
}
