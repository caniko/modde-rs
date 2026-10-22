//! `modde deploy` configuring the detected launcher for proxy DLLs and
//! enabled tools in a fake Cyberpunk 2077 install.

#![cfg(all(unix, feature = "cyberpunk", feature = "linux-integrations"))]

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModdeDb};
use serde_json::{Value, json};

const GAME: &str = "cyberpunk2077";

/// A `main` profile for an install at `install` that already has CET's
/// `version.dll` proxy next to the game executable.
async fn sandbox(fx: &Fixture, install: &Path) {
    // Launcher configuration belongs to the legacy game-wide save scope,
    // which an existing game-wide save vault selects.
    std::fs::create_dir_all(fx.data_dir().join("saves").join(GAME)).unwrap();
    std::fs::create_dir_all(install.join("bin/x64")).unwrap();
    std::fs::write(install.join("bin/x64/version.dll"), b"cet").unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install.to_path_buf());
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    let archive = fx.data_dir().join("store/mod_a/archive/pc/mod/a.archive");
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    std::fs::write(archive, b"a").unwrap();
    ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap()
        .create_profile(&Profile {
            id: None,
            name: "main".to_string(),
            game_id: GameId::from(GAME),
            source: ProfileSource::Manual,
            mods: vec![EnabledMod {
                mod_id: "mod_a".to_string(),
                enabled: true,
                ..Default::default()
            }],
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: smallvec::SmallVec::new(),
            load_order_lock: None,
        })
        .await
        .unwrap();
}

fn modde(fx: &Fixture, args: &[&str]) -> String {
    let output: Output = fx.cmd().args(args).output().expect("spawn modde");
    assert!(
        output.status.success(),
        "{args:?} failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn heroic_config_gets_overrides_wrapper_and_tool_environment() {
    let fx = Fixture::new();
    let install = fx.root().join("games/Cyberpunk 2077");
    sandbox(&fx, &install).await;
    let heroic = fx.home().join(".config/heroic");
    let config_path = heroic.join("GamesConfig/cp2077.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        json!({ "cp2077": { "enviromentOptions": [], "wrapperOptions": [] } }).to_string(),
    )
    .unwrap();
    std::fs::create_dir_all(heroic.join("gog_store")).unwrap();
    std::fs::write(
        heroic.join("gog_store/installed.json"),
        json!({ "installed": [{ "appName": "cp2077", "install_path": install }] }).to_string(),
    )
    .unwrap();
    modde(&fx, &["tool", "enable", "mangohud", "--game", GAME]);

    let out = modde(&fx, &["deploy"]);
    for expected in [
        "Detected proxy DLLs needing Wine overrides: version.dll",
        "Updated Heroic config with WINEDLLOVERRIDES: version=n,b",
        "Launch wrapper: exports",
        "Registered modde launch wrapper in Heroic",
        "tool env var(s) to Heroic config",
    ] {
        assert!(out.contains(expected), "missing {expected:?}:\n{out}");
    }

    let config: Value =
        serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
    let game = &config["cp2077"];
    assert!(
        game["enviromentOptions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["key"] == "WINEDLLOVERRIDES" && entry["value"] == "version=n,b"),
        "{game}"
    );
    assert!(
        game["wrapperOptions"]
            .to_string()
            .contains("modde-launch-wrapper"),
        "{game}"
    );
}

#[tokio::test]
async fn steam_and_unknown_launchers_get_instructions() {
    let fx = Fixture::new();
    let steamapps = fx.root().join("steamapps");
    let install = steamapps.join("common/Cyberpunk 2077");
    sandbox(&fx, &install).await;

    // Without an app manifest the launcher is unknown.
    let unknown = modde(&fx, &["deploy"]);
    assert!(
        unknown.contains(
            "Set this environment variable before launching:\n  WINEDLLOVERRIDES=\"version=n,b\""
        ),
        "{unknown}"
    );
    assert!(!unknown.contains("Launch wrapper"), "{unknown}");

    std::fs::write(
        steamapps.join("appmanifest_1091500.acf"),
        "\"installdir\" \"Cyberpunk 2077\"",
    )
    .unwrap();
    let steam = modde(&fx, &["deploy"]);
    assert!(
        steam.contains(
            "Steam: Add this to your launch options:\n  WINEDLLOVERRIDES=\"version=n,b\" %command%"
        ),
        "{steam}"
    );
}
