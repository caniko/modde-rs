//! End-to-end tests for `modde save` with a fake Stardew Valley install,
//! whose saves live under the fixture's `XDG_CONFIG_HOME`.

#![cfg(feature = "stardew")]

mod common;

use std::path::PathBuf;
use std::process::Output;

use common::Fixture;
use modde_core::GameId;
use modde_core::settings::AppSettings;

const GAME: &str = "stardew-valley";
const PROFILE: &str = "farm";

/// A fixture with a configured install, an active `farm` profile and one
/// live save, `Farm_123/Farm_123` containing `contents`.
fn sandbox(contents: &str) -> (Fixture, PathBuf) {
    let fx = Fixture::new();
    let install = fx.root().join("install");
    std::fs::create_dir_all(&install).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install);
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));

    let save = fx.home().join(".config/StardewValley/Saves/Farm_123");
    std::fs::create_dir_all(&save).unwrap();
    std::fs::write(save.join("Farm_123"), contents).unwrap();

    ok(&modde(&fx, &["profile", "create", PROFILE, "--game", GAME]));
    (fx, save)
}

fn modde(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd().args(args).output().expect("spawn modde")
}

fn save(fx: &Fixture, args: &[&str]) -> Output {
    let mut full = vec!["save"];
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

/// Commit ids from `save history`, newest first.
fn history(fx: &Fixture) -> Vec<String> {
    ok(&save(fx, &["history", "--profile", PROFILE]))
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next().map(str::to_string))
        .collect()
}

#[test]
fn scan_assign_list_and_unassign() {
    let (fx, live) = sandbox("day 1");
    let live_arg = live.display().to_string();

    let scan = ok(&save(&fx, &["scan"]));
    assert!(scan.contains(&live_arg), "{scan}");

    assert!(ok(&save(&fx, &["list", "--profile", PROFILE])).contains("No saves assigned"));
    let assigned = ok(&save(
        &fx,
        &["assign", "--profile", PROFILE, "--label", "main", &live_arg],
    ));
    assert!(assigned.contains("Assigned save"), "{assigned}");
    let list = ok(&save(&fx, &["list", "--profile", PROFILE]));
    assert!(list.contains("(label: main"), "{list}");

    assert!(ok(&modde(&fx, &["save", "unassign", &live_arg])).contains("Unassigned save"));
    assert!(ok(&save(&fx, &["list", "--profile", PROFILE])).contains("No saves assigned"));
}

#[test]
fn adopt_capture_and_restore_round_trip() {
    let (fx, live) = sandbox("day 1");
    let file = live.join("Farm_123");

    let adopted = ok(&save(&fx, &["adopt", "--profile", PROFILE]));
    assert!(adopted.contains("Adopted 1 save files"), "{adopted}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "day 1");
    let day_one = history(&fx)[0].clone();

    assert!(ok(&save(&fx, &["capture", "--profile", PROFILE])).contains("No saves to capture"));

    std::fs::write(&file, "day 2").unwrap();
    let captured = ok(&save(&fx, &["capture", "--profile", PROFILE]));
    assert!(captured.contains("Captured 1 save file(s)"), "{captured}");
    assert_eq!(history(&fx).len(), 3, "init, adopt and day-2 snapshots");

    let restored = ok(&save(&fx, &["restore", "--profile", PROFILE, &day_one]));
    assert!(restored.contains("Restored 1 save file(s)"), "{restored}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "day 1");
}

#[test]
fn auto_capture_uses_the_active_profile() {
    let (fx, live) = sandbox("day 1");
    ok(&save(&fx, &["adopt", "--profile", PROFILE]));
    std::fs::write(live.join("Farm_123"), "day 2").unwrap();

    let auto = ok(&save(&fx, &["auto-capture"]));
    assert!(auto.contains('1'), "{auto}");
    assert_eq!(history(&fx).len(), 3);
}

#[test]
fn rejects_unresolved_installs_missing_save_dirs_and_unknown_snapshots() {
    let (fx, _live) = sandbox("day 1");

    let unconfigured = err(&modde(&fx, &["save", "scan", "--game", "skyrim-se"]));
    assert!(
        unconfigured.contains("configure its game path"),
        "{unconfigured}"
    );

    let settings_path = fx.home().join(".config/modde/settings.toml");
    let mut settings = AppSettings::load_from(&settings_path);
    let skyrim = fx.root().join("skyrim");
    std::fs::create_dir_all(&skyrim).unwrap();
    settings.set_game_path(&GameId::from("skyrim-se"), skyrim);
    settings.save_to(&settings_path);
    let no_save_dir = err(&modde(&fx, &["save", "scan", "--game", "skyrim-se"]));
    // Built-in games all support save profiles; Skyrim simply has no save
    // directory inside the isolated home.
    assert!(
        no_save_dir.contains("configure this installation's save_directory"),
        "{no_save_dir}"
    );

    ok(&save(&fx, &["adopt", "--profile", PROFILE]));
    let missing = err(&save(&fx, &["restore", "--profile", PROFILE, "deadbeef"]));
    assert!(missing.contains("deadbeef"), "{missing}");
}
