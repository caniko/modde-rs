#![cfg(feature = "bethesda")]

use modde_games::bethesda;
use modde_games::{
    GamePlugin, SUPPORTED_GAME_IDS, resolve_game_plugin, resolve_save_dependency_analyzer,
    resolve_save_tracker,
};

#[test]
fn starfield_constant_has_correct_game_id() {
    assert_eq!(bethesda::STARFIELD.game_id(), "starfield");
}

#[test]
fn resolve_game_plugin_returns_some_for_starfield() {
    assert!(
        resolve_game_plugin("starfield").is_some(),
        "resolve_game_plugin should return Some for starfield"
    );
}

#[test]
fn starfield_in_supported_game_ids() {
    assert!(
        SUPPORTED_GAME_IDS.contains(&"starfield"),
        "SUPPORTED_GAME_IDS should contain starfield"
    );
}

#[test]
fn starfield_save_tracker_is_exposed() {
    assert!(
        resolve_save_tracker("starfield").is_some(),
        "Starfield save tracking should resolve once .sfs tracking is wired"
    );
    assert!(bethesda::STARFIELD.supports_save_profiles());
}

#[test]
fn starfield_save_dependency_analyzer_is_explicitly_unavailable_until_verified() {
    assert!(
        resolve_save_dependency_analyzer("starfield").is_none(),
        "Starfield must remain without a save dependency analyzer until a verified .sfs magic/header fixture or format source exists; update docs and this test when STARFIELD_SAVE_ANALYZER is real"
    );
}

#[test]
fn starfield_save_tracker_detects_sfs_files_by_category() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Autosave1.sfs"), b"").unwrap();
    std::fs::write(tmp.path().join("Quicksave.sfs"), b"").unwrap();
    std::fs::write(tmp.path().join("Exitsave0.sfs"), b"").unwrap();
    std::fs::write(tmp.path().join("Save42_Custom.sfs"), b"").unwrap();
    std::fs::write(tmp.path().join("Save42_Custom.bak"), b"").unwrap();

    let saves = bethesda::saves::STARFIELD_SAVE_TRACKER
        .detect_saves(tmp.path())
        .unwrap();
    let mut categories: Vec<_> = saves.iter().map(|save| save.category.as_ref()).collect();
    categories.sort_unstable();

    assert_eq!(saves.len(), 4);
    assert_eq!(categories, vec!["auto", "exit", "manual", "quick"]);
    assert!(
        saves
            .iter()
            .any(|save| save.label.as_deref() == Some("Save42_Custom"))
    );
}
