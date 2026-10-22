//! End-to-end tests for `modde hot-deploy` toggling a mod in a deployed
//! Cyberpunk profile without a full redeploy.

#![cfg(feature = "cyberpunk")]

mod common;

use std::path::PathBuf;
use std::process::Output;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModdeDb};

const GAME: &str = "cyberpunk2077";

/// A deployed `main` profile with `mod_a` and `mod_b`, each shipping one
/// archive; returns the fixture and the deployed archive directory.
async fn deployed() -> (Fixture, PathBuf) {
    let fx = Fixture::new();
    let install = fx.root().join("install");
    std::fs::create_dir_all(install.join("bin/x64")).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install.clone());
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    for mod_id in ["mod_a", "mod_b"] {
        let file = fx
            .data_dir()
            .join("store")
            .join(mod_id)
            .join(format!("archive/pc/mod/{mod_id}.archive"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, mod_id).unwrap();
    }
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    db.create_profile(&Profile {
        id: None,
        name: "main".to_string(),
        game_id: GameId::from(GAME),
        source: ProfileSource::Manual,
        mods: ["mod_a", "mod_b"]
            .into_iter()
            .map(|id| EnabledMod {
                mod_id: id.to_string(),
                enabled: true,
                ..Default::default()
            })
            .collect(),
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: None,
    })
    .await
    .unwrap();
    drop(db);
    ok(&modde(&fx, &["deploy"]));
    (fx, install.join("mods/archive/pc/mod"))
}

fn modde(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .args(args)
        .args(["--profile", "main", "--game", GAME])
        .output()
        .expect("spawn modde")
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

async fn mod_b_enabled(fx: &Fixture) -> bool {
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    db.load_profile("main", &GameId::from(GAME))
        .await
        .unwrap()
        .mods
        .iter()
        .find(|m| m.mod_id == "mod_b")
        .unwrap()
        .enabled
}

#[tokio::test]
async fn hot_deploy_toggles_one_mod_in_place() {
    let (fx, archives) = deployed().await;
    assert!(archives.join("mod_b.archive").exists());

    let dry_run = ok(&modde(
        &fx,
        &["hot-deploy", "--mod", "mod_b", "--disable", "--dry-run"],
    ));
    assert!(dry_run.contains("Removed paths: 1"), "{dry_run}");
    assert!(
        dry_run.contains("Dry run: no files or profile state changed."),
        "{dry_run}"
    );
    assert!(archives.join("mod_b.archive").exists());
    assert!(mod_b_enabled(&fx).await);

    let disabled = ok(&modde(&fx, &["hot-deploy", "--mod", "mod_b", "--disable"]));
    assert!(disabled.contains("Live VFS patched."), "{disabled}");
    assert!(!archives.join("mod_b.archive").exists());
    assert!(archives.join("mod_a.archive").exists());
    assert!(!mod_b_enabled(&fx).await);

    let enabled = ok(&modde(&fx, &["hot-deploy", "--mod", "mod_b", "--enable"]));
    assert!(enabled.contains("Added paths: 1"), "{enabled}");
    assert_eq!(
        std::fs::read_to_string(archives.join("mod_b.archive")).unwrap(),
        "mod_b"
    );
    assert!(mod_b_enabled(&fx).await);
}

#[tokio::test]
async fn hot_deploy_rejects_unknown_mods_and_ambiguous_flags() {
    let (fx, _archives) = deployed().await;
    assert!(
        err(&modde(&fx, &["hot-deploy", "--mod", "nope", "--enable"]))
            .contains("mod 'nope' is not part of profile 'main'")
    );
    assert!(
        err(&modde(&fx, &["hot-deploy", "--mod", "mod_a"]))
            .contains("choose exactly one of --enable or --disable")
    );
}
