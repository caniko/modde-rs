#![cfg(all(feature = "bethesda", feature = "cyberpunk"))]

use std::path::Path;

use modde_games::bethesda::{FALLOUT4, FALLOUT76, SKYRIM_AE, SKYRIM_SE, STARFIELD};
use modde_games::cyberpunk::CYBERPUNK2077;
use modde_games::traits::GamePlugin;
use tempfile::TempDir;

// ── GamePlugin trait implementation tests ───────────────────────────

#[test]
fn test_skyrim_se_game_id() {
    let game = SKYRIM_SE;
    assert_eq!(game.game_id(), "skyrim-se");
}

#[test]
fn test_skyrim_se_display_name() {
    let game = SKYRIM_SE;
    assert!(game.display_name().contains("Skyrim"));
    assert!(game.display_name().contains("Special Edition"));
}

#[test]
fn test_skyrim_se_mod_directory() {
    let game = SKYRIM_SE;
    let install = Path::new("/game/Skyrim Special Edition");
    assert_eq!(game.mod_directory(install), install.join("Data"));
}

#[test]
fn test_skyrim_ae_game_id() {
    let game = SKYRIM_AE;
    assert_eq!(game.game_id(), "skyrim-ae");
}

#[test]
fn test_skyrim_ae_display_name() {
    let game = SKYRIM_AE;
    assert!(game.display_name().contains("Anniversary"));
}

#[test]
fn test_skyrim_ae_mod_directory() {
    let game = SKYRIM_AE;
    let install = Path::new("/game/Skyrim");
    assert_eq!(game.mod_directory(install), install.join("Data"));
}

#[test]
fn test_fallout4_game_id() {
    let game = FALLOUT4;
    assert_eq!(game.game_id(), "fallout4");
}

#[test]
fn test_fallout4_display_name() {
    let game = FALLOUT4;
    assert!(game.display_name().contains("Fallout 4"));
}

#[test]
fn test_fallout4_mod_directory() {
    let game = FALLOUT4;
    let install = Path::new("/game/Fallout 4");
    assert_eq!(game.mod_directory(install), install.join("Data"));
}

#[test]
fn test_fallout76_game_id() {
    let game = FALLOUT76;
    assert_eq!(game.game_id(), "fallout76");
}

#[test]
fn test_fallout76_display_name() {
    let game = FALLOUT76;
    assert!(game.display_name().contains("Fallout 76"));
}

#[test]
fn test_fallout76_mod_directory() {
    let game = FALLOUT76;
    let install = Path::new("/game/FALLOUT76");
    assert_eq!(game.mod_directory(install), install.join("Data"));
}

// ── All Bethesda games use Data directory ───────────────────────────

#[test]
fn test_all_bethesda_games_use_data_dir() {
    let games: Vec<Box<dyn GamePlugin>> = vec![
        Box::new(SKYRIM_SE),
        Box::new(SKYRIM_AE),
        Box::new(FALLOUT4),
        Box::new(FALLOUT76),
    ];

    let install = Path::new("/game");
    for game in &games {
        let mod_dir = game.mod_directory(install);
        assert_eq!(
            mod_dir.file_name().unwrap().to_str().unwrap(),
            "Data",
            "{} should use Data directory",
            game.game_id()
        );
    }
}

// ── Game ID uniqueness ──────────────────────────────────────────────

#[test]
fn test_all_game_ids_unique() {
    let games: Vec<Box<dyn GamePlugin>> = vec![
        Box::new(SKYRIM_SE),
        Box::new(SKYRIM_AE),
        Box::new(FALLOUT4),
        Box::new(FALLOUT76),
    ];

    let ids: Vec<&str> = games.iter().map(|g| g.game_id()).collect();
    let mut deduped = ids.clone();
    deduped.sort_unstable();
    deduped.dedup();
    assert_eq!(ids.len(), deduped.len(), "game IDs should be unique");
}

struct MinimalGame;

impl GamePlugin for MinimalGame {
    fn game_id(&self) -> &'static str {
        "minimal"
    }

    fn display_name(&self) -> &'static str {
        "Minimal"
    }

    fn mod_directory(&self, install: &Path) -> std::path::PathBuf {
        install.join("mods")
    }
}

#[test]
fn test_save_profiles_default_disabled() {
    assert!(!MinimalGame.supports_save_profiles());
}

#[test]
fn test_existing_save_profile_games_opt_in() {
    assert!(SKYRIM_SE.supports_save_profiles());
    assert!(SKYRIM_AE.supports_save_profiles());
    assert!(FALLOUT4.supports_save_profiles());
    assert!(FALLOUT76.supports_save_profiles());
    assert!(CYBERPUNK2077.supports_save_profiles());
    assert!(STARFIELD.supports_save_profiles());
}

// ── Deploy symlinks ─────────────────────────────────────────────────

#[test]
fn test_deploy_creates_symlinks_in_target() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    // Create files in staging
    std::fs::write(staging.join("mod.esp"), "plugin data").unwrap();
    std::fs::create_dir_all(staging.join("textures")).unwrap();
    std::fs::write(staging.join("textures/sky.dds"), "texture").unwrap();

    let target = tmp.path().join("Data");
    let game = SKYRIM_SE;
    game.deploy(&staging, &target).unwrap();

    // Verify symlinks exist
    assert!(
        target
            .join("mod.esp")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        target
            .join("textures/sky.dds")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // Verify content accessible
    assert_eq!(
        std::fs::read_to_string(target.join("mod.esp")).unwrap(),
        "plugin data"
    );
    assert_eq!(
        std::fs::read_to_string(target.join("textures/sky.dds")).unwrap(),
        "texture"
    );
}

#[test]
fn test_deploy_creates_target_if_missing() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("test.esp"), "data").unwrap();

    let target = tmp.path().join("nonexistent/Data");
    let game = SKYRIM_SE;
    game.deploy(&staging, &target).unwrap();

    assert!(target.exists());
    assert!(
        target
            .join("test.esp")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn test_deploy_overwrites_existing_files() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("mod.esp"), "new version").unwrap();

    let target = tmp.path().join("Data");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("mod.esp"), "old version").unwrap();

    let game = SKYRIM_SE;
    game.deploy(&staging, &target).unwrap();

    // Should now be a symlink pointing to staging
    assert!(
        target
            .join("mod.esp")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        std::fs::read_to_string(target.join("mod.esp")).unwrap(),
        "new version"
    );
}

#[test]
fn test_deploy_empty_staging() {
    let tmp = TempDir::new().unwrap();
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();

    let target = tmp.path().join("Data");
    let game = SKYRIM_SE;
    game.deploy(&staging, &target).unwrap();

    assert!(target.exists());
}

#[test]
fn test_post_deploy_succeeds() {
    let tmp = TempDir::new().unwrap();
    let games: Vec<Box<dyn GamePlugin>> = vec![
        Box::new(SKYRIM_SE),
        Box::new(SKYRIM_AE),
        Box::new(FALLOUT4),
        Box::new(FALLOUT76),
    ];

    for game in &games {
        assert!(
            game.post_deploy(tmp.path()).is_ok(),
            "{} post_deploy should succeed",
            game.game_id()
        );
    }
}
