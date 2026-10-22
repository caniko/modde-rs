//! End-to-end tests for `modde deploy` and `modde verify` with a seeded
//! profile and store against a fake Stardew Valley install (mods deploy into `Mods/`).

#![cfg(feature = "stardew")]

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::resolver::{LoadOrderRule, ModId};
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModdeDb};

const GAME: &str = "stardew-valley";

struct Sandbox {
    fx: Fixture,
    mods_dir: PathBuf,
}

/// Seed `store/<mod_id>/<rel_path>` files (content = mod id) and a `main`
/// profile listing `mods` as `(mod_id, enabled)` in load order.
async fn sandbox(
    files: &[(&str, &str)],
    mods: &[(&str, bool)],
    rules: Vec<LoadOrderRule>,
    hidden: &[(&str, &str)],
) -> Sandbox {
    let fx = Fixture::new();
    let install = fx.root().join("install");
    std::fs::create_dir_all(&install).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install.clone());
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    std::fs::create_dir_all(fx.home().join(".config/StardewValley/Saves")).unwrap();

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
                .map(|(mod_id, enabled)| EnabledMod {
                    mod_id: (*mod_id).to_string(),
                    enabled: *enabled,
                    ..Default::default()
                })
                .collect(),
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: rules.into_iter().collect(),
            load_order_lock: None,
        })
        .await
        .unwrap();
    for (mod_id, rel_path) in hidden {
        db.hide_file(id, &ModId::from(*mod_id), rel_path)
            .await
            .unwrap();
    }
    Sandbox {
        fx,
        mods_dir: install.join("Mods"),
    }
}

fn deploy(fx: &Fixture) -> Output {
    fx.cmd()
        .args(["deploy", "--profile", "main", "--game", GAME])
        .output()
        .expect("spawn modde deploy")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "deploy failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Deployed relative paths and the content each one resolves to.
fn deployed(dir: &Path) -> Vec<(String, String)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().display().to_string();
                out.push((rel, std::fs::read_to_string(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    if dir.exists() {
        walk(dir, dir, &mut out);
    }
    out.sort();
    out
}

fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(a, b)| ((*a).to_string(), (*b).to_string()))
        .collect()
}

#[tokio::test]
async fn deploys_enabled_mods_idempotently() {
    let sb = sandbox(
        &[
            ("mod_a", "ModA/manifest.json"),
            ("mod_b", "ModB/manifest.json"),
            ("mod_off", "Off/manifest.json"),
        ],
        &[("mod_a", true), ("mod_b", true), ("mod_off", false)],
        Vec::new(),
        &[],
    )
    .await;

    let out = ok(&deploy(&sb.fx));
    assert!(out.contains("Load order: 2 enabled mods"), "{out}");
    assert!(out.contains("Total files: 2"), "{out}");
    let expected = pairs(&[
        ("ModA/manifest.json", "mod_a"),
        ("ModB/manifest.json", "mod_b"),
    ]);
    assert_eq!(deployed(&sb.mods_dir), expected);

    ok(&deploy(&sb.fx));
    assert_eq!(deployed(&sb.mods_dir), expected);
}

#[tokio::test]
async fn later_mods_win_conflicts_unless_rules_reorder_them() {
    let files = [
        ("mod_a", "Shared/config.json"),
        ("mod_b", "Shared/config.json"),
    ];
    let mods = [("mod_a", true), ("mod_b", true)];

    let sb = sandbox(&files, &mods, Vec::new(), &[]).await;
    let out = ok(&deploy(&sb.fx));
    assert!(out.contains("Conflicts detected (1 files)"), "{out}");
    assert_eq!(
        deployed(&sb.mods_dir),
        pairs(&[("Shared/config.json", "mod_b")])
    );

    let rule = LoadOrderRule::LoadAfter {
        mod_id: ModId::from("mod_a"),
        after: ModId::from("mod_b"),
    };
    let sb = sandbox(&files, &mods, vec![rule], &[]).await;
    ok(&deploy(&sb.fx));
    assert_eq!(
        deployed(&sb.mods_dir),
        pairs(&[("Shared/config.json", "mod_a")])
    );
}

#[tokio::test]
async fn hidden_files_are_not_deployed() {
    let sb = sandbox(
        &[
            ("mod_a", "ModA/manifest.json"),
            ("mod_a", "ModA/extra.json"),
        ],
        &[("mod_a", true)],
        Vec::new(),
        &[("mod_a", "ModA/extra.json")],
    )
    .await;
    ok(&deploy(&sb.fx));
    assert_eq!(
        deployed(&sb.mods_dir),
        pairs(&[("ModA/manifest.json", "mod_a")])
    );
}

#[tokio::test]
async fn deploy_requires_a_known_profile_and_install() {
    let sb = sandbox(&[], &[], Vec::new(), &[]).await;
    let missing = sb
        .fx
        .cmd()
        .args(["deploy", "--profile", "nope", "--game", GAME])
        .output()
        .unwrap();
    assert!(!missing.status.success());

    std::fs::remove_file(sb.fx.home().join(".config/modde/settings.toml")).unwrap();
    let no_install = deploy(&sb.fx);
    assert!(!no_install.status.success());
}

#[cfg(unix)]
fn verify(fx: &Fixture) -> String {
    ok(&fx
        .cmd()
        .args(["verify", "--profile", "main", "--game", GAME])
        .output()
        .expect("spawn modde verify"))
}

#[cfg(unix)]
#[tokio::test]
async fn verify_reports_missing_and_unreadable_store_files() {
    let clean = sandbox(
        &[("mod_a", "ModA/manifest.json"), ("mod_off", "Off/a.json")],
        &[("mod_a", true), ("mod_off", false)],
        Vec::new(),
        &[],
    )
    .await;
    let out = verify(&clean.fx);
    assert!(out.contains("Total files checked: 1"), "{out}");
    assert!(out.contains("All files OK."), "{out}");

    let broken = sandbox(
        &[
            ("mod_a", "ModA/manifest.json"),
            ("mod_a", "ModA/locked.json"),
        ],
        &[("mod_a", true), ("mod_gone", true)],
        Vec::new(),
        &[],
    )
    .await;
    let mod_dir = broken.fx.data_dir().join("store/mod_a/ModA");
    std::os::unix::fs::symlink("/nonexistent/target", mod_dir.join("dangling.json")).unwrap();
    std::fs::set_permissions(
        mod_dir.join("locked.json"),
        std::os::unix::fs::PermissionsExt::from_mode(0o000),
    )
    .unwrap();

    let out = verify(&broken.fx);
    assert!(out.contains("Missing mods/files:  2"), "{out}");
    assert!(out.contains("  - mod_gone\n"), "{out}");
    assert!(out.contains("dangling.json"), "{out}");
    assert!(out.contains("Mismatches/errors:   1"), "{out}");
    assert!(out.contains("locked.json (hash error:"), "{out}");
}
