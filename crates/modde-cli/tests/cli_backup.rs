//! End-to-end tests for `modde backup`: mod snapshots and plugin load order.

mod common;

use std::process::Output;

use common::Fixture;

fn backup(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("backup")
        .args(args)
        .output()
        .expect("spawn modde backup")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "backup failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    assert!(!output.status.success(), "backup unexpectedly succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn mod_backups_restore_the_latest_snapshot() {
    let fx = Fixture::new();
    let file = fx.data_dir().join("store/mod_a/data/config.ini");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "original").unwrap();

    assert!(ok(&backup(&fx, &["create", "mod_a"])).contains("Backup created: "));
    std::fs::write(&file, "edited").unwrap();
    let restored = ok(&backup(&fx, &["restore", "mod_a"]));
    assert!(
        restored.contains("Restored mod 'mod_a' from backup: "),
        "{restored}"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "original");
    assert!(ok(&backup(&fx, &["list", "mod_a"])).contains("Backups for 'mod_a':"));

    assert!(ok(&backup(&fx, &["list", "ghost"])).contains("No backups found for mod 'ghost'."));
    assert!(err(&backup(&fx, &["create", "ghost"])).contains("mod directory not found"));
    assert!(err(&backup(&fx, &["restore", "ghost"])).contains("failed to restore backup"));
}

#[cfg(feature = "bethesda")]
#[test]
fn plugin_order_round_trips_through_plugins_txt() {
    let fx = Fixture::new();
    let plugins_txt = fx.home().join(
        ".local/share/Steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt",
    );
    std::fs::create_dir_all(plugins_txt.parent().unwrap()).unwrap();
    for name in ["main", "other"] {
        ok(&fx
            .cmd()
            .args(["profile", "create", name, "--game", "skyrim-se"])
            .output()
            .unwrap());
    }
    let args = |action: &'static str, profile: &'static str| {
        [action, "--profile", profile, "--game", "skyrim-se"]
    };

    assert!(
        err(&backup(&fx, &args("plugins", "main"))).contains("no real plugin order found"),
        "a profile without any plugin order has nothing to back up"
    );
    assert!(
        err(&backup(&fx, &args("restore-plugins", "main")))
            .contains("failed to restore plugin order")
    );

    // The first backup seeds the profile from the game's plugins.txt.
    std::fs::write(&plugins_txt, "# order\nListed.esp\nOther.esp\n").unwrap();
    assert!(
        ok(&backup(&fx, &args("plugins", "main"))).contains("Plugin order backed up (2 plugins)")
    );
    std::fs::remove_file(&plugins_txt).unwrap();
    let restored = ok(&backup(&fx, &args("restore-plugins", "main")));
    assert!(
        restored.contains("  1: Listed.esp (disabled)\n  2: Other.esp (disabled)"),
        "{restored}"
    );
    let written = std::fs::read_to_string(&plugins_txt).unwrap();
    assert!(written.ends_with("Listed.esp\nOther.esp\n"), "{written}");

    // An enabled plugin that is not staged fails record validation, so the
    // native plugins.txt is left alone.
    std::fs::write(&plugins_txt, "*Listed.esp\n").unwrap();
    ok(&backup(&fx, &args("plugins", "other")));
    let refused = err(&backup(&fx, &args("restore-plugins", "other")));
    assert!(
        refused
            .contains("refusing to write native plugin order because validation found 1 issue(s)"),
        "{refused}"
    );
    assert!(
        refused.contains("[ERROR] Listed.esp: active plugin file is missing"),
        "{refused}"
    );
    assert_eq!(
        std::fs::read_to_string(&plugins_txt).unwrap(),
        "*Listed.esp\n"
    );
}
