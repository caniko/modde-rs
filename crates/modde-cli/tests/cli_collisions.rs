//! End-to-end tests for `modde collisions` with a seeded Stardew Valley
//! profile and store.

#![cfg(feature = "stardew")]

mod common;

use std::path::PathBuf;
use std::process::Output;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::resolver::ModId;
use modde_core::{GameId, ModdeDb};

const GAME: &str = "stardew-valley";

/// Seed `store/<mod_id>/<rel_path>` files and a `main` profile enabling
/// `mods` in load order (lowest priority first).
async fn sandbox(files: &[(&str, &str)], mods: &[&str], hidden: &[(&str, &str)]) -> Fixture {
    let fx = Fixture::new();
    for (mod_id, rel_path) in files {
        let path = fx.data_dir().join("store").join(mod_id).join(rel_path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, mod_id).unwrap();
    }
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let id = db
        .create_profile(&Profile {
            id: None,
            name: "main".to_string(),
            game_id: GameId::from(GAME),
            source: ProfileSource::Manual,
            mods: mods
                .iter()
                .map(|mod_id| EnabledMod {
                    mod_id: (*mod_id).to_string(),
                    enabled: true,
                    ..Default::default()
                })
                .collect(),
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: Default::default(),
            load_order_lock: None,
        })
        .await
        .unwrap();
    for (mod_id, rel_path) in hidden {
        db.hide_file(id, &ModId::from(*mod_id), rel_path)
            .await
            .unwrap();
    }
    fx
}

fn collisions(fx: &Fixture, extra: &[&str]) -> Output {
    fx.cmd()
        .args(["collisions", "--profile", "main", "--game", GAME])
        .args(extra)
        .output()
        .expect("spawn modde collisions")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "collisions failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn reports_pairs_shadowed_mods_and_redundant_files() {
    let fx = sandbox(
        &[
            ("dup", "B/config.json"),
            ("base", "B/config.json"),
            ("base", "B/tex.dds"),
            ("base", "B/only.dll"),
            ("patch", "B/config.json"),
            ("tex", "B/tex.dds"),
        ],
        &["dup", "base", "patch", "tex"],
        &[("base", "B/config.json")],
    )
    .await;

    let out = ok(&collisions(&fx, &[]));
    assert!(
        out.contains("Collision Report for profile \"main\" (stardew-valley)"),
        "{out}"
    );
    assert!(
        out.contains("3 file collisions across 3 mod pairs"),
        "{out}"
    );
    assert!(out.contains("[CONFIG] base vs patch (1 files)"), "{out}");
    assert!(
        out.contains("  B/config.json  ->  winner: patch (hidden)"),
        "{out}"
    );
    assert!(out.contains("[CONFIG] dup vs patch (1 files)"), "{out}");
    assert!(!out.contains("base vs tex"), "cosmetic pair shown:\n{out}");
    assert!(
        out.contains("  - \"dup\" (1 files, all overridden by patch)"),
        "{out}"
    );
    assert!(out.contains("Redundant files (never win): 3"), "{out}");
    assert!(out.contains("Run with --suggest-hides"), "{out}");

    let all = ok(&collisions(&fx, &["--all", "--suggest-hides"]));
    assert!(all.contains("[COSMETIC] base vs tex (1 files)"), "{all}");
    assert!(all.contains("Suggested hide commands:"), "{all}");
    assert!(
        all.contains("  modde profile hide \"base\" \"B/tex.dds\""),
        "{all}"
    );
}

#[tokio::test]
async fn disjoint_mods_have_no_collisions() {
    let fx = sandbox(
        &[("a", "A/manifest.json"), ("b", "B/manifest.json")],
        &["a", "b", "not-in-store"],
        &[],
    )
    .await;

    let out = ok(&collisions(&fx, &[]));
    assert!(
        out.contains("0 file collisions across 0 mod pairs"),
        "{out}"
    );
    assert!(out.contains("No collisions detected."), "{out}");
}

#[test]
fn missing_profile_fails() {
    let fx = Fixture::new();
    let out = collisions(&fx, &[]);
    assert!(!out.status.success());
}
