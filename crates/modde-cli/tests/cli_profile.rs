//! End-to-end tests for `modde profile` against a fake Stardew Valley
//! install, whose saves live under the fixture's `XDG_CONFIG_HOME`.

#![cfg(feature = "stardew")]

mod common;

use std::path::PathBuf;
use std::process::Output;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModdeDb};

const GAME: &str = "stardew-valley";

/// A fixture with a configured install, profiles `farm` and `ranch`, and
/// one live save file (returned) containing `farm save`.
fn sandbox() -> (Fixture, PathBuf) {
    let fx = Fixture::new();
    let install = fx.root().join("install");
    std::fs::create_dir_all(&install).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install);
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));

    let save = fx
        .home()
        .join(".config/StardewValley/Saves/Farm_123/Farm_123");
    std::fs::create_dir_all(save.parent().unwrap()).unwrap();
    std::fs::write(&save, "farm save").unwrap();

    ok(&profile(&fx, &["create", "farm"]));
    ok(&profile(&fx, &["create", "ranch"]));
    (fx, save)
}

fn modde(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd().args(args).output().expect("spawn modde")
}

fn profile(fx: &Fixture, args: &[&str]) -> Output {
    let mut full = vec!["profile"];
    full.extend_from_slice(args);
    full.extend_from_slice(&["--game", GAME]);
    modde(fx, &full)
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

#[test]
fn fork_lock_and_delete_profiles() {
    let (fx, _save) = sandbox();

    let forked = ok(&profile(&fx, &["fork", "farm", "farm-copy"]));
    assert!(
        forked.contains("Forked profile 'farm' -> 'farm-copy'"),
        "{forked}"
    );
    let list = ok(&modde(&fx, &["profile", "list"]));
    for name in ["farm ", "farm-copy ", "ranch "] {
        assert!(list.contains(name), "{name} missing:\n{list}");
    }

    assert!(ok(&profile(&fx, &["lock-info", "farm"])).contains("is not locked"));
    assert!(
        ok(&profile(&fx, &["lock", "farm", "--note", "curated"]))
            .contains("Locked profile 'farm' (manual: curated)")
    );
    assert!(ok(&profile(&fx, &["lock-info", "farm"])).contains("Reason:    manual (curated)"));
    assert!(ok(&profile(&fx, &["unlock", "farm"])).contains("Unlocked profile 'farm'"));

    assert!(ok(&profile(&fx, &["delete", "farm-copy"])).contains("Deleted profile: farm-copy"));
    assert!(!ok(&modde(&fx, &["profile", "list"])).contains("farm-copy"));
    assert!(!profile(&fx, &["delete", "farm-copy"]).status.success());
}

#[test]
fn switch_requires_adopted_saves_then_isolates_them() {
    let (fx, save) = sandbox();

    assert!(ok(&profile(&fx, &["active"])).contains("No active profile"));
    // The refusal currently exits 0; only its guidance and effect are pinned.
    let refused = profile(&fx, &["switch", "ranch"]);
    let guidance = String::from_utf8_lossy(&refused.stdout);
    assert!(guidance.contains("modde save adopt"), "{guidance}");
    assert!(ok(&profile(&fx, &["active"])).contains("No active profile"));

    ok(&modde(
        &fx,
        &["save", "adopt", "--game", GAME, "--profile", "farm"],
    ));
    assert!(ok(&profile(&fx, &["active"])).contains("Active profile: farm"));

    assert!(ok(&profile(&fx, &["switch", "ranch"])).contains("Switched to profile: ranch"));
    std::fs::write(&save, "ranch save").unwrap();
    ok(&profile(&fx, &["switch", "farm"]));
    assert_eq!(std::fs::read_to_string(&save).unwrap(), "farm save");
    ok(&profile(&fx, &["switch", "ranch"]));
    assert_eq!(std::fs::read_to_string(&save).unwrap(), "ranch save");
}

#[test]
fn experiments_roll_back_or_commit() {
    let (fx, _save) = sandbox();
    ok(&modde(
        &fx,
        &["save", "adopt", "--game", GAME, "--profile", "farm"],
    ));

    assert!(err(&profile(&fx, &["commit"])).contains("not in experiment mode"));

    assert!(ok(&profile(&fx, &["try", "ranch"])).contains("stack depth: 1"));
    let active = ok(&profile(&fx, &["active"]));
    assert!(active.contains("Active profile: ranch"), "{active}");
    assert!(active.contains("Experiment depth: 1"), "{active}");
    assert!(ok(&profile(&fx, &["rollback"])).contains("Rolled back to profile: farm"));

    ok(&profile(&fx, &["try", "ranch"]));
    assert!(ok(&profile(&fx, &["commit"])).contains("Experiment accepted"));
    let active = ok(&profile(&fx, &["active"]));
    assert!(active.contains("Active profile: ranch"), "{active}");
    assert!(!active.contains("Experiment depth"), "{active}");
}

#[tokio::test]
async fn mod_pins_and_dedup_suspects() {
    let (fx, _save) = sandbox();
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let mods = ["nexus_stardew_1_1", "cet/leftover"]
        .into_iter()
        .map(|id| EnabledMod {
            mod_id: id.to_string(),
            enabled: true,
            ..Default::default()
        })
        .collect();
    db.create_profile(&Profile {
        id: None,
        name: "modded".to_string(),
        game_id: GameId::from(GAME),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: None,
    })
    .await
    .unwrap();
    drop(db);

    ok(&profile(&fx, &["lock-mod", "modded", "nexus_stardew_1_1"]));
    assert!(ok(&profile(&fx, &["lock-info", "modded"])).contains("nexus_stardew_1_1"));
    ok(&profile(
        &fx,
        &["unlock-mod", "modded", "nexus_stardew_1_1"],
    ));
    assert!(err(&profile(&fx, &["lock-mod", "modded", "nope"])).contains("mod 'nope' not found"));

    let unlocked = ok(&profile(&fx, &["dedup", "modded"]));
    assert!(unlocked.contains("2 mods"), "{unlocked}");

    ok(&profile(&fx, &["lock", "modded"]));
    let suspects = ok(&profile(&fx, &["dedup", "modded"]));
    assert!(suspects.contains("cet/leftover"), "{suspects}");
}
