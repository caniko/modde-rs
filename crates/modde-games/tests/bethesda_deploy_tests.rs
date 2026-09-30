#![cfg(feature = "bethesda")]

//! Comprehensive tests for Bethesda game plugin deployment,
//! INI patching edge cases, and plugins.txt parsing.

use std::path::PathBuf;

use modde_games::bethesda::plugins_txt::{PluginEntry, format_plugins_txt, parse_plugins_txt};
use modde_games::bethesda::{FALLOUT4, FALLOUT76, SKYRIM_AE, SKYRIM_SE};
use modde_games::traits::GamePlugin;
use tempfile::TempDir;

// ── GamePlugin trait tests ─────────────────────────────────────────

#[test]
fn test_all_bethesda_games_have_unique_ids() {
    let games: Vec<Box<dyn GamePlugin>> = vec![
        Box::new(SKYRIM_SE),
        Box::new(SKYRIM_AE),
        Box::new(FALLOUT4),
        Box::new(FALLOUT76),
    ];

    let ids: Vec<&str> = games.iter().map(|g| g.game_id()).collect();
    let unique: std::collections::HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "game IDs must be unique");
}

#[test]
fn test_bethesda_mod_directory_is_data() {
    let tmp = TempDir::new().unwrap();
    let games: Vec<Box<dyn GamePlugin>> = vec![
        Box::new(SKYRIM_SE),
        Box::new(SKYRIM_AE),
        Box::new(FALLOUT4),
        Box::new(FALLOUT76),
    ];

    for game in &games {
        let mod_dir = game.mod_directory(tmp.path());
        assert!(
            mod_dir.ends_with("Data"),
            "{} mod_dir should end with 'Data', got {:?}",
            game.game_id(),
            mod_dir
        );
    }
}

#[test]
fn test_deploy_creates_symlinks_for_flat_structure() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("game/Data");

    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("plugin.esp"), "plugin data").unwrap();
    std::fs::write(staging.join("textures.bsa"), "bsa data").unwrap();

    SKYRIM_SE.deploy(&staging, &target).unwrap();

    assert!(
        target
            .join("plugin.esp")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        target
            .join("textures.bsa")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn test_deploy_creates_nested_symlinks() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("game/Data");

    std::fs::create_dir_all(staging.join("textures/landscape")).unwrap();
    std::fs::write(staging.join("textures/landscape/dirt.dds"), "texture").unwrap();
    std::fs::create_dir_all(staging.join("meshes/architecture")).unwrap();
    std::fs::write(staging.join("meshes/architecture/wall.nif"), "mesh").unwrap();

    SKYRIM_SE.deploy(&staging, &target).unwrap();

    let t1 = target.join("textures/landscape/dirt.dds");
    let t2 = target.join("meshes/architecture/wall.nif");
    assert!(t1.symlink_metadata().unwrap().file_type().is_symlink());
    assert!(t2.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read_to_string(&t1).unwrap(), "texture");
    assert_eq!(std::fs::read_to_string(&t2).unwrap(), "mesh");
}

#[test]
fn test_deploy_empty_staging() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("game/Data");

    std::fs::create_dir_all(&staging).unwrap();

    SKYRIM_SE.deploy(&staging, &target).unwrap();
    // Target should exist but be empty (or may not exist if no symlinks created)
}

#[test]
fn test_deploy_overwrites_existing_symlinks() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    let target = tmp.path().join("game/Data");

    // First deploy
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("mod.esp"), "v1").unwrap();
    SKYRIM_SE.deploy(&staging, &target).unwrap();

    // Replace source content
    std::fs::write(staging.join("mod.esp"), "v2").unwrap();

    // Second deploy should overwrite
    SKYRIM_SE.deploy(&staging, &target).unwrap();

    assert_eq!(
        std::fs::read_to_string(target.join("mod.esp")).unwrap(),
        "v2"
    );
}

#[test]
fn test_post_deploy_is_noop_for_bethesda() {
    let tmp = TempDir::new().unwrap();
    // post_deploy should succeed without doing anything
    SKYRIM_SE.post_deploy(tmp.path()).unwrap();
    FALLOUT4.post_deploy(tmp.path()).unwrap();
}

// ── plugins.txt parsing ────────────────────────────────────────────

#[test]
fn test_parse_empty() {
    let result = parse_plugins_txt("");
    assert!(result.is_empty());
}

#[test]
fn test_parse_comments_only() {
    let content = "# This is a header\n# Another comment\n";
    let result = parse_plugins_txt(content);
    assert!(result.is_empty());
}

#[test]
fn test_parse_enabled_plugins() {
    let content = "*Skyrim.esm\n*Update.esm\n*Dawnguard.esm\n";
    let result = parse_plugins_txt(content);
    assert_eq!(result.len(), 3);
    assert!(result.iter().all(|p| p.enabled));
    assert_eq!(result[0].name, "Skyrim.esm");
}

#[test]
fn test_parse_disabled_plugins() {
    let content = "DisabledMod.esp\nAnotherDisabled.esp\n";
    let result = parse_plugins_txt(content);
    assert_eq!(result.len(), 2);
    assert!(result.iter().all(|p| !p.enabled));
}

#[test]
fn test_parse_mixed_enabled_disabled() {
    let content = "# Header\n*Skyrim.esm\nOptionalMod.esp\n*USSEP.esp\n\n";
    let result = parse_plugins_txt(content);
    assert_eq!(result.len(), 3);
    assert!(result[0].enabled);
    assert!(!result[1].enabled);
    assert!(result[2].enabled);
}

#[test]
fn test_parse_plugin_names_with_spaces() {
    let content = "*My Cool Mod.esp\nAnother Mod With Spaces.esp\n";
    let result = parse_plugins_txt(content);
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].name, "My Cool Mod.esp");
    assert!(result[0].enabled);
    assert_eq!(result[1].name, "Another Mod With Spaces.esp");
}

#[test]
fn test_parse_various_extensions() {
    let content = "*Plugin.esm\n*Plugin.esp\n*Plugin.esl\n";
    let result = parse_plugins_txt(content);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_parse_empty_lines_skipped() {
    let content = "\n\n*Skyrim.esm\n\n\n*Update.esm\n\n";
    let result = parse_plugins_txt(content);
    assert_eq!(result.len(), 2);
}

// ── plugins.txt formatting ─────────────────────────────────────────

#[test]
fn test_format_empty() {
    let result = format_plugins_txt(&[]);
    assert!(result.starts_with("# This file is generated by modde"));
}

#[test]
fn test_format_enabled_plugins() {
    let entries = vec![
        PluginEntry {
            name: "Skyrim.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "USSEP.esp".to_string(),
            enabled: true,
        },
    ];

    let formatted = format_plugins_txt(&entries);
    assert!(formatted.contains("*Skyrim.esm"));
    assert!(formatted.contains("*USSEP.esp"));
}

#[test]
fn test_format_disabled_plugins() {
    let entries = vec![PluginEntry {
        name: "Optional.esp".to_string(),
        enabled: false,
    }];

    let formatted = format_plugins_txt(&entries);
    assert!(formatted.contains("Optional.esp"));
    assert!(!formatted.contains("*Optional.esp"));
}

#[test]
fn test_format_parse_roundtrip() {
    let entries = vec![
        PluginEntry {
            name: "Skyrim.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "Update.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "Optional.esp".to_string(),
            enabled: false,
        },
        PluginEntry {
            name: "Dawnguard.esm".to_string(),
            enabled: true,
        },
    ];

    let formatted = format_plugins_txt(&entries);
    let parsed = parse_plugins_txt(&formatted);
    assert_eq!(parsed.len(), 4);
    for (orig, parsed) in entries.iter().zip(parsed.iter()) {
        assert_eq!(orig.name, parsed.name);
        assert_eq!(orig.enabled, parsed.enabled);
    }
}

#[test]
fn test_format_parse_roundtrip_large() {
    let entries: Vec<PluginEntry> = (0..300)
        .map(|i| PluginEntry {
            name: format!("Plugin_{i:04}.esp"),
            enabled: i % 3 != 0,
        })
        .collect();

    let formatted = format_plugins_txt(&entries);
    let parsed = parse_plugins_txt(&formatted);
    assert_eq!(parsed.len(), 300);
    for (orig, parsed) in entries.iter().zip(parsed.iter()) {
        assert_eq!(orig.name, parsed.name, "name mismatch at some index");
        assert_eq!(
            orig.enabled, parsed.enabled,
            "enabled mismatch for {}",
            orig.name
        );
    }
}

// ── plugins.txt file I/O ───────────────────────────────────────────

#[test]
fn test_write_and_read_plugins_txt_file() {
    use modde_games::bethesda::plugins_txt::{read_plugins_txt_from, write_plugins_txt_to};

    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("plugins.txt");

    let entries = vec![
        PluginEntry {
            name: "Skyrim.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "DisabledMod.esp".to_string(),
            enabled: false,
        },
    ];

    write_plugins_txt_to(&path, &entries).unwrap();
    let loaded = read_plugins_txt_from(&path).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].name, "Skyrim.esm");
    assert!(loaded[0].enabled);
    assert!(!loaded[1].enabled);
}

#[test]
fn test_write_overwrites_existing() {
    use modde_games::bethesda::plugins_txt::{read_plugins_txt_from, write_plugins_txt_to};

    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("plugins.txt");

    write_plugins_txt_to(
        &path,
        &[PluginEntry {
            name: "Old.esp".to_string(),
            enabled: true,
        }],
    )
    .unwrap();

    write_plugins_txt_to(
        &path,
        &[PluginEntry {
            name: "New.esp".to_string(),
            enabled: true,
        }],
    )
    .unwrap();

    let loaded = read_plugins_txt_from(&path).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].name, "New.esp");
}

#[test]
fn test_read_nonexistent_returns_error() {
    use modde_games::bethesda::plugins_txt::read_plugins_txt_from;

    let result = read_plugins_txt_from(&PathBuf::from("/nonexistent/plugins.txt"));
    assert!(result.is_err());
}
