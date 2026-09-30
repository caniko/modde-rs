#![cfg(feature = "cyberpunk")]

use modde_games::GamePlugin;
use modde_games::HotDeploySupport;
use modde_games::cyberpunk::Cyberpunk2077;
use modde_games::cyberpunk::manifest::RedModManifest;

// ── Cyberpunk2077 game_id and display_name ──────────────────────────

#[test]
fn test_cyberpunk_game_id() {
    let game = Cyberpunk2077;
    assert_eq!(game.game_id(), "cyberpunk2077");
}

#[test]
fn test_cyberpunk_display_name() {
    let game = Cyberpunk2077;
    assert_eq!(game.display_name(), "Cyberpunk 2077");
}

// ── mod_directory returns "mods" subdirectory ───────────────────────

#[test]
fn test_cyberpunk_mod_directory() {
    let game = Cyberpunk2077;
    let install = std::path::Path::new("/fake/game/install");
    let mod_dir = game.mod_directory(install);
    assert_eq!(mod_dir, install.join("mods"));
}

#[test]
fn test_cyberpunk_hot_deploy_is_experimental() {
    let game = Cyberpunk2077;
    let capability = game.hot_deploy_capability();
    assert_eq!(capability.support, HotDeploySupport::Experimental);
    assert!(capability.cosmetic_only);
}

// ── RedModManifest::parse with valid JSON ────────────────────────────

#[test]
fn test_redmod_manifest_parse_full() {
    let json = r#"{
        "name": "TestMod",
        "version": "1.2.3",
        "custom_sounds": [
            {
                "name": "ambient_rain",
                "type": "ambient",
                "file": "sounds/rain.wav"
            }
        ],
        "scripts": [
            {
                "name": "main_script",
                "path": "scripts/main.reds"
            }
        ]
    }"#;

    let manifest = RedModManifest::parse(json).unwrap();
    assert_eq!(manifest.name, "TestMod");
    assert_eq!(manifest.version.as_deref(), Some("1.2.3"));
    assert_eq!(manifest.custom_sounds.len(), 1);
    assert_eq!(manifest.custom_sounds[0].name, "ambient_rain");
    assert_eq!(
        manifest.custom_sounds[0].sound_type.as_deref(),
        Some("ambient")
    );
    assert_eq!(manifest.custom_sounds[0].file, "sounds/rain.wav");
    assert_eq!(manifest.scripts.len(), 1);
    assert_eq!(manifest.scripts[0].name, "main_script");
    assert_eq!(
        manifest.scripts[0].path.as_deref(),
        Some("scripts/main.reds")
    );
}

// ── RedModManifest::parse with minimal JSON (no optional fields) ─────

#[test]
fn test_redmod_manifest_parse_minimal() {
    let json = r#"{"name": "MinimalMod"}"#;
    let manifest = RedModManifest::parse(json).unwrap();
    assert_eq!(manifest.name, "MinimalMod");
    assert!(manifest.version.is_none());
    assert!(manifest.custom_sounds.is_empty());
    assert!(manifest.scripts.is_empty());
}

#[test]
fn test_redmod_manifest_parse_name_only_with_version() {
    let json = r#"{"name": "VersionedMod", "version": "0.1.0"}"#;
    let manifest = RedModManifest::parse(json).unwrap();
    assert_eq!(manifest.name, "VersionedMod");
    assert_eq!(manifest.version.as_deref(), Some("0.1.0"));
    assert!(manifest.custom_sounds.is_empty());
    assert!(manifest.scripts.is_empty());
}

// ── RedModManifest::parse with custom sounds ─────────────────────────

#[test]
fn test_redmod_manifest_parse_multiple_custom_sounds() {
    let json = r#"{
        "name": "SoundMod",
        "custom_sounds": [
            {"name": "footstep_wood", "type": "sfx", "file": "sounds/wood.wav"},
            {"name": "footstep_metal", "type": "sfx", "file": "sounds/metal.wav"},
            {"name": "ambient_wind", "file": "sounds/wind.wav"}
        ]
    }"#;

    let manifest = RedModManifest::parse(json).unwrap();
    assert_eq!(manifest.custom_sounds.len(), 3);
    assert_eq!(manifest.custom_sounds[0].name, "footstep_wood");
    assert_eq!(manifest.custom_sounds[1].name, "footstep_metal");
    assert_eq!(manifest.custom_sounds[2].name, "ambient_wind");
    // Third sound has no type
    assert!(manifest.custom_sounds[2].sound_type.is_none());
}

// ── RedModManifest::parse with scripts ───────────────────────────────

#[test]
fn test_redmod_manifest_parse_multiple_scripts() {
    let json = r#"{
        "name": "ScriptMod",
        "scripts": [
            {"name": "init", "path": "scripts/init.reds"},
            {"name": "cleanup"}
        ]
    }"#;

    let manifest = RedModManifest::parse(json).unwrap();
    assert_eq!(manifest.scripts.len(), 2);
    assert_eq!(manifest.scripts[0].name, "init");
    assert_eq!(
        manifest.scripts[0].path.as_deref(),
        Some("scripts/init.reds")
    );
    assert_eq!(manifest.scripts[1].name, "cleanup");
    assert!(manifest.scripts[1].path.is_none());
}

// ── RedModManifest::parse with invalid JSON ──────────────────────────

#[test]
fn test_redmod_manifest_parse_invalid_json() {
    let result = RedModManifest::parse("not valid json");
    assert!(result.is_err());
}

#[test]
fn test_redmod_manifest_parse_missing_name() {
    // "name" is required
    let json = r#"{"version": "1.0"}"#;
    let result = RedModManifest::parse(json);
    assert!(result.is_err());
}

#[test]
fn test_redmod_manifest_parse_empty_object() {
    let result = RedModManifest::parse("{}");
    assert!(result.is_err());
}

#[test]
fn test_redmod_manifest_parse_empty_string() {
    let result = RedModManifest::parse("");
    assert!(result.is_err());
}

// ── Cyberpunk deploy creates symlinks ───────────────────────────────

#[test]
fn test_cyberpunk_deploy_creates_symlinks() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    // Create a mod directory in staging
    let mod_dir = staging.path().join("mymod");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(mod_dir.join("info.json"), r#"{"name":"mymod"}"#).unwrap();

    let game = Cyberpunk2077;
    game.deploy(staging.path(), target.path()).unwrap();

    // The mod directory should be symlinked
    let deployed = target.path().join("mymod");
    assert!(deployed.exists());
    assert!(
        deployed
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn test_cyberpunk_deploy_replaces_existing_symlink() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    // Create mod in staging
    let mod_dir = staging.path().join("testmod");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(mod_dir.join("data.bin"), b"mod data").unwrap();

    let game = Cyberpunk2077;

    // Deploy once
    game.deploy(staging.path(), target.path()).unwrap();
    assert!(target.path().join("testmod").exists());

    // Deploy again -- should succeed (replacing existing symlink)
    game.deploy(staging.path(), target.path()).unwrap();
    assert!(target.path().join("testmod").exists());
}

#[test]
fn test_cyberpunk_deploy_empty_staging() {
    let staging = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();

    let game = Cyberpunk2077;
    // Empty staging -- should succeed with nothing to deploy
    game.deploy(staging.path(), target.path()).unwrap();
}

#[test]
fn test_cyberpunk_deploy_creates_target_dir() {
    let staging = tempfile::tempdir().unwrap();
    let target_base = tempfile::tempdir().unwrap();
    let target = target_base.path().join("new_dir");

    let mod_dir = staging.path().join("amod");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(mod_dir.join("file.txt"), b"data").unwrap();

    let game = Cyberpunk2077;
    game.deploy(staging.path(), &target).unwrap();
    assert!(target.exists());
    assert!(target.join("amod").exists());
}

#[test]
fn test_cyberpunk_hot_deploy_patches_staging_and_refreshes_root_symlink() {
    let install = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();

    let source_file = source.path().join("body.archive");
    std::fs::write(&source_file, b"archive").unwrap();

    let stale_root = install.path().join("mods/archive");
    std::fs::create_dir_all(&stale_root).unwrap();
    std::fs::write(stale_root.join("stale.txt"), b"stale").unwrap();

    let patch = modde_core::hot_deploy::HotDeployPatch {
        changes: vec![modde_core::hot_deploy::HotDeployChange {
            rel_path: "archive/pc/mod/body.archive".to_string(),
            before: None,
            after: Some(source_file),
        }],
    };

    let game = Cyberpunk2077;
    game.apply_hot_deploy_patch(&patch, staging.path(), install.path())
        .unwrap();

    let staged = staging.path().join("archive/pc/mod/body.archive");
    assert!(staged.symlink_metadata().unwrap().file_type().is_symlink());

    let deployed_root = install.path().join("mods/archive");
    assert!(
        deployed_root
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read(deployed_root.join("pc/mod/body.archive")).unwrap(),
        b"archive"
    );
}
