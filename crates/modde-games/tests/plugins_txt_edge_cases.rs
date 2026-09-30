#![cfg(feature = "bethesda")]

use std::path::Path;

use modde_games::bethesda::plugins_txt::{
    PluginEntry, read_plugins_txt_from, write_plugins_txt_to,
};
use tempfile::TempDir;

// ── Reading edge cases ──────────────────────────────────────────────

#[test]
fn test_read_only_comments() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");
    std::fs::write(&path, "# comment 1\n# comment 2\n# comment 3\n").unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn test_read_only_empty_lines() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");
    std::fs::write(&path, "\n\n\n\n").unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert!(entries.is_empty());
}

#[test]
fn test_read_plugin_name_with_spaces() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");
    std::fs::write(&path, "*My Cool Mod.esp\n").unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "My Cool Mod.esp");
    assert!(entries[0].enabled);
}

#[test]
fn test_read_many_plugins() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");

    let mut content = String::new();
    for i in 0..200 {
        if i % 2 == 0 {
            content.push_str(&format!("*Plugin{i}.esp\n"));
        } else {
            content.push_str(&format!("Plugin{i}.esp\n"));
        }
    }
    std::fs::write(&path, &content).unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert_eq!(entries.len(), 200);
    assert!(entries[0].enabled);
    assert!(!entries[1].enabled);
}

#[test]
fn test_read_comment_between_plugins() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");
    std::fs::write(
        &path,
        "*Skyrim.esm\n# Master files above, mods below\n*Mod.esp\n",
    )
    .unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "Skyrim.esm");
    assert_eq!(entries[1].name, "Mod.esp");
}

#[test]
fn test_read_star_only_line() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");
    // A line with just "*" should be treated as an enabled plugin with empty name
    std::fs::write(&path, "*\n").unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "");
    assert!(entries[0].enabled);
}

#[test]
fn test_read_various_extensions() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");
    std::fs::write(&path, "*Master.esm\n*Plugin.esp\nLight.esl\n").unwrap();

    let entries = read_plugins_txt_from(&path).unwrap();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "Master.esm");
    assert_eq!(entries[1].name, "Plugin.esp");
    assert_eq!(entries[2].name, "Light.esl");
}

// ── Writing edge cases ──────────────────────────────────────────────

#[test]
fn test_write_and_read_large_list() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");

    let entries: Vec<PluginEntry> = (0..500)
        .map(|i| PluginEntry {
            name: format!("Plugin{i}.esp"),
            enabled: i % 3 != 0,
        })
        .collect();

    write_plugins_txt_to(&path, &entries).unwrap();
    let read_back = read_plugins_txt_from(&path).unwrap();
    assert_eq!(read_back.len(), 500);

    for (original, read) in entries.iter().zip(read_back.iter()) {
        assert_eq!(original.name, read.name);
        assert_eq!(original.enabled, read.enabled);
    }
}

#[test]
fn test_write_overwrite_existing() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");

    // Write initial
    let entries1 = vec![PluginEntry {
        name: "Old.esp".to_string(),
        enabled: true,
    }];
    write_plugins_txt_to(&path, &entries1).unwrap();

    // Overwrite
    let entries2 = vec![PluginEntry {
        name: "New.esp".to_string(),
        enabled: false,
    }];
    write_plugins_txt_to(&path, &entries2).unwrap();

    let read_back = read_plugins_txt_from(&path).unwrap();
    assert_eq!(read_back.len(), 1);
    assert_eq!(read_back[0].name, "New.esp");
    assert!(!read_back[0].enabled);
}

#[test]
fn test_write_header_not_read_as_plugin() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");

    let entries = vec![PluginEntry {
        name: "Mod.esp".to_string(),
        enabled: true,
    }];
    write_plugins_txt_to(&path, &entries).unwrap();

    // Read back and verify header comment is not parsed as a plugin
    let read_back = read_plugins_txt_from(&path).unwrap();
    assert_eq!(read_back.len(), 1);
    assert_eq!(read_back[0].name, "Mod.esp");
}

#[test]
fn test_read_nonexistent_returns_error() {
    let result =
        read_plugins_txt_from(Path::new("/tmp/nonexistent_modde_plugins_test/plugins.txt"));
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("failed to read"));
}

// ── Roundtrip consistency ───────────────────────────────────────────

#[test]
fn test_roundtrip_preserves_order() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("plugins.txt");

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
            name: "Dawnguard.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "HearthFires.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "Dragonborn.esm".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "Unofficial Skyrim.esp".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "SkyUI_SE.esp".to_string(),
            enabled: true,
        },
        PluginEntry {
            name: "DisabledMod.esp".to_string(),
            enabled: false,
        },
    ];

    write_plugins_txt_to(&path, &entries).unwrap();
    let read_back = read_plugins_txt_from(&path).unwrap();

    assert_eq!(entries.len(), read_back.len());
    for (i, (orig, read)) in entries.iter().zip(read_back.iter()).enumerate() {
        assert_eq!(orig.name, read.name, "mismatch at index {i}");
        assert_eq!(orig.enabled, read.enabled, "enabled mismatch at index {i}");
    }
}
