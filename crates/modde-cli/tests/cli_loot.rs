//! End-to-end tests for `modde loot` against a cached masterlist, the
//! Skyrim SE `plugins.txt` and plugin headers in a fake install.

#![cfg(feature = "bethesda")]

mod common;

use std::path::Path;
use std::process::Output;

use common::Fixture;
use modde_core::GameId;
use modde_core::settings::AppSettings;

const GAME: &str = "skyrim-se";

fn loot(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("loot")
        .args(args)
        .output()
        .expect("spawn modde loot")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "loot failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn plugins_txt(fx: &Fixture, content: &str) {
    let path = fx.home().join(
        ".local/share/Steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt",
    );
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

/// A TES4 header carrying only `HEDR` (form `version`) and `MAST` records.
fn plugin(path: &Path, version: f32, masters: &[&str]) {
    let mut data = Vec::new();
    data.extend_from_slice(b"HEDR");
    data.extend_from_slice(&12u16.to_le_bytes());
    data.extend_from_slice(&version.to_le_bytes());
    data.extend_from_slice(&[0; 8]);
    for master in masters {
        data.extend_from_slice(b"MAST");
        data.extend_from_slice(&u16::try_from(master.len() + 1).unwrap().to_le_bytes());
        data.extend_from_slice(master.as_bytes());
        data.push(0);
    }
    let mut bytes = b"TES4".to_vec();
    bytes.extend_from_slice(&u32::try_from(data.len()).unwrap().to_le_bytes());
    bytes.extend_from_slice(&[0; 16]);
    bytes.extend_from_slice(&data);
    std::fs::write(path, bytes).unwrap();
}

#[test]
fn sort_applies_masterlist_rules_to_active_plugins() {
    let fx = Fixture::new();
    let missing = ok(&loot(&fx, &["sort", "--game", GAME]));
    assert!(missing.contains("Masterlist not cached."), "{missing}");
    assert!(
        missing.contains("loot/skyrimse/master/masterlist.yaml"),
        "{missing}"
    );
    let unsupported = loot(&fx, &["sort", "--game", "stardew-valley"]);
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("no LOOT masterlist available"));

    let cache = fx.home().join(".local/share/loot/skyrim-se.yaml");
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    std::fs::write(
        &cache,
        r#"
plugins:
  - name: "SkyUI_SE.esp"
    after: ["USSEP.esp"]
  - name: "Early.esp"
    requires: ["USSEP.esp"]
  - name: "BadMod.esp"
    incompatible:
      - name: "ConflictMod.esp"
"#,
    )
    .unwrap();
    let empty = ok(&loot(&fx, &["sort", "--game", GAME]));
    assert!(
        empty.contains("Loaded LOOT masterlist: 3 plugin entries"),
        "{empty}"
    );
    assert!(empty.contains("No active plugins found"), "{empty}");

    plugins_txt(
        &fx,
        "*USSEP.esp\n*SkyUI_SE.esp\n*Early.esp\n*BadMod.esp\n*ConflictMod.esp\nOff.esp\n",
    );
    let sorted = ok(&loot(&fx, &["sort", "--game", GAME])).to_lowercase();
    assert!(sorted.contains("generated 3 load order rules"), "{sorted}");
    assert!(
        sorted.contains("skyui_se.esp loads after ussep.esp"),
        "{sorted}"
    );
    assert!(
        sorted.contains("early.esp loads after ussep.esp"),
        "{sorted}"
    );
    assert!(
        sorted.contains("incompatible: badmod.esp <-> conflictmod.esp"),
        "{sorted}"
    );
}

#[test]
fn validate_flags_form_43_plugins_and_missing_masters() {
    let fx = Fixture::new();
    let install = fx.root().join("skyrim");
    let data = install.join("Data");
    std::fs::create_dir_all(&data).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install);
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    plugin(&data.join("Skyrim.esm"), 1.71, &[]);
    plugin(&data.join("Old.esp"), 0.94, &["Skyrim.esm"]);
    plugin(
        &data.join("Needy.esp"),
        1.71,
        &["Skyrim.esm", "Missing.esm"],
    );

    let none = ok(&loot(&fx, &["validate", "--game", GAME]));
    assert!(none.contains("No active plugins found"), "{none}");

    plugins_txt(&fx, "*Skyrim.esm\n*Old.esp\n*Needy.esp\n");
    let out = ok(&loot(&fx, &["validate", "--game", GAME]));
    assert!(out.contains("2 warning(s) found"), "{out}");
    assert!(out.contains("[FORM43] Old.esp (v0.94)"), "{out}");
    assert!(
        out.contains("[MISSING] Needy.esp requires 'Missing.esm' which is not loaded"),
        "{out}"
    );

    plugins_txt(&fx, "*Skyrim.esm\n");
    assert!(ok(&loot(&fx, &["validate", "--game", GAME])).contains("All 1 plugins are valid."));
}

#[test]
fn validate_requires_a_known_install() {
    let fx = Fixture::new();
    let output = loot(&fx, &["validate", "--game", GAME]);
    assert!(!output.status.success());
}
