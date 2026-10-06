//! Integration tests for `AppSettings` + `ProfileManager` interaction.
//!
//! These mirror the exact initialization flow used by the UI's `Modde::new()`:
//! 1. Load settings from disk
//! 2. List profiles from the database
//! 3. Auto-detect `selected_game` from profile if unset
//! 4. Auto-detect `game_path` if missing
//! 5. Persist settings

use std::path::PathBuf;

use modde_core::GameId;
use modde_core::db::ModdeDb;
use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::settings::AppSettings;
use smallvec::smallvec;

fn make_profile(name: &str, game_id: &str) -> Profile {
    Profile {
        id: None,
        name: name.to_string(),
        game_id: GameId::from(game_id),
        source: ProfileSource::Manual,
        mods: Vec::new(),
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    }
}

// ---------------------------------------------------------------------------
// Settings round-trip via file
// ---------------------------------------------------------------------------

#[test]
fn settings_round_trip_preserves_all_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");

    let mut original = AppSettings {
        nexus_api_key: "abc123".into(),
        ..AppSettings::default()
    };
    original.set_game_path(
        &GameId::from("cyberpunk2077"),
        PathBuf::from("/games/cp2077"),
    );
    original.set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/games/skyrim"));
    original.selected_game = Some("cyberpunk2077".into());
    original.theme = "Nord".into();
    original.download_dir = Some(PathBuf::from("/dl"));

    original.save_to(&path);
    let loaded = AppSettings::load_from(&path);

    assert_eq!(loaded.nexus_api_key, "abc123");
    assert_eq!(loaded.selected_game.as_deref(), Some("cyberpunk2077"));
    assert_eq!(loaded.theme, "Nord");
    assert_eq!(loaded.download_dir, Some(PathBuf::from("/dl")));
    assert_eq!(
        loaded.game_path(&GameId::from("cyberpunk2077")),
        Some(&PathBuf::from("/games/cp2077"))
    );
    assert_eq!(
        loaded.game_path(&GameId::from("skyrim-se")),
        Some(&PathBuf::from("/games/skyrim"))
    );
    assert!(loaded.game_path(&GameId::from("fallout4")).is_none());
}

#[test]
fn settings_load_missing_file_gives_defaults() {
    let loaded = AppSettings::load_from(&PathBuf::from("/nonexistent/path/settings.toml"));
    assert_eq!(loaded.nexus_api_key, "");
    assert!(loaded.game_paths.is_empty());
    assert!(loaded.selected_game.is_none());
}

#[test]
fn settings_load_partial_toml_fills_defaults() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");
    std::fs::write(&path, "nexus_api_key = \"mykey\"\n").unwrap();

    let loaded = AppSettings::load_from(&path);
    assert_eq!(loaded.nexus_api_key, "mykey");
    assert!(loaded.game_paths.is_empty());
    assert!(loaded.selected_game.is_none());
}

#[test]
fn settings_load_ignores_unknown_fields() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");
    std::fs::write(&path, "nexus_api_key = \"k\"\nfuture_field = 42\n").unwrap();

    let loaded = AppSettings::load_from(&path);
    assert_eq!(loaded.nexus_api_key, "k");
}

// ---------------------------------------------------------------------------
// game_path / set_game_path
// ---------------------------------------------------------------------------

#[test]
fn set_game_path_adds_new_entry() {
    let mut s = AppSettings::default();
    s.set_game_path(&GameId::from("cyberpunk2077"), PathBuf::from("/a"));
    assert_eq!(s.game_paths.len(), 1);
    assert_eq!(
        s.game_path(&GameId::from("cyberpunk2077")),
        Some(&PathBuf::from("/a"))
    );
}

#[test]
fn set_game_path_updates_existing() {
    let mut s = AppSettings::default();
    s.set_game_path(&GameId::from("cyberpunk2077"), PathBuf::from("/old"));
    s.set_game_path(&GameId::from("cyberpunk2077"), PathBuf::from("/new"));
    assert_eq!(
        s.game_paths.len(),
        1,
        "should update in-place, not add duplicate"
    );
    assert_eq!(
        s.game_path(&GameId::from("cyberpunk2077")),
        Some(&PathBuf::from("/new"))
    );
}

#[test]
fn set_game_path_independent_games() {
    let mut s = AppSettings::default();
    s.set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/skyrim"));
    s.set_game_path(&GameId::from("cyberpunk2077"), PathBuf::from("/cp2077"));
    assert_eq!(s.game_paths.len(), 2);
    assert_eq!(
        s.game_path(&GameId::from("skyrim-se")),
        Some(&PathBuf::from("/skyrim"))
    );
    assert_eq!(
        s.game_path(&GameId::from("cyberpunk2077")),
        Some(&PathBuf::from("/cp2077"))
    );
}

// ---------------------------------------------------------------------------
// UI init flow: profile listing -> auto-detect game
// ---------------------------------------------------------------------------

/// Simulates the exact logic in `Modde::new()` for selecting a game from profiles.
async fn simulate_ui_init(
    pm: &ProfileManager,
    settings: &mut AppSettings,
) -> (Vec<modde_core::ProfileSummary>, Option<String>) {
    let profiles = pm.list().await.unwrap_or_default();
    let mut selected_game = settings.selected_game.clone();

    // Auto-detect: if no game selected but profiles exist, pick first profile's game
    if selected_game.is_none()
        && let Some(first) = profiles.first()
    {
        selected_game = Some(first.game_id.to_string());
        settings.selected_game = Some(first.game_id.to_string());
    }

    (profiles, selected_game)
}

#[tokio::test]
async fn ui_init_no_profiles_no_settings_gives_nothing() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let mut settings = AppSettings::default();

    let (profiles, selected) = simulate_ui_init(&pm, &mut settings).await;

    assert_eq!(profiles, [] as [modde_core::ProfileSummary; 0]);
    assert!(selected.is_none());
    assert!(settings.selected_game.is_none());
}

#[tokio::test]
async fn ui_init_profile_exists_auto_selects_game() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    pm.create(&make_profile("3077", "cyberpunk2077"))
        .await
        .unwrap();

    let mut settings = AppSettings::default();
    let (profiles, selected) = simulate_ui_init(&pm, &mut settings).await;

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "3077");
    assert_eq!(selected, Some("cyberpunk2077".to_string()));
    assert_eq!(settings.selected_game, Some("cyberpunk2077".to_string()));
}

#[tokio::test]
async fn ui_init_settings_already_has_game_does_not_override() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    pm.create(&make_profile("3077", "cyberpunk2077"))
        .await
        .unwrap();
    pm.create(&make_profile("skyrim", "skyrim-se"))
        .await
        .unwrap();

    let mut settings = AppSettings {
        selected_game: Some("skyrim-se".into()),
        ..AppSettings::default()
    };

    let (profiles, selected) = simulate_ui_init(&pm, &mut settings).await;

    assert_eq!(profiles.len(), 2);
    // Should keep the existing selection, not override with first profile's game
    assert_eq!(selected, Some("skyrim-se".to_string()));
}

#[tokio::test]
async fn ui_init_multiple_profiles_picks_first() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    // DB returns profiles ordered by game_id, name
    pm.create(&make_profile("beta", "skyrim-se")).await.unwrap();
    pm.create(&make_profile("alpha", "cyberpunk2077"))
        .await
        .unwrap();

    let mut settings = AppSettings::default();
    let (profiles, selected) = simulate_ui_init(&pm, &mut settings).await;

    assert_eq!(profiles.len(), 2);
    // First profile in the list determines the game
    let first_game = &profiles[0].game_id;
    assert_eq!(selected.as_deref(), Some(first_game.as_str()));
}

// ---------------------------------------------------------------------------
// CLI install flow: settings persistence after profile creation
// ---------------------------------------------------------------------------

/// Simulates the settings persistence that happens after `modde install`.
fn simulate_cli_install_settings(
    settings_path: &std::path::Path,
    game_id: &GameId,
    game_dir: Option<PathBuf>,
) {
    let mut settings = AppSettings::load_from(settings_path);
    if let Some(gd) = game_dir {
        settings.set_game_path(game_id, gd);
    }
    settings.selected_game = Some(game_id.to_string());
    settings.save_to(settings_path);
}

#[test]
fn cli_install_persists_game_path_and_selected_game() {
    let tmp = tempfile::tempdir().unwrap();
    let settings_path = tmp.path().join("settings.toml");

    // Simulate CLI install with game dir
    simulate_cli_install_settings(
        &settings_path,
        &GameId::from("cyberpunk2077"),
        Some(PathBuf::from("/games/cp2077")),
    );

    // Now UI loads the settings
    let loaded = AppSettings::load_from(&settings_path);
    assert_eq!(loaded.selected_game.as_deref(), Some("cyberpunk2077"));
    assert_eq!(
        loaded.game_path(&GameId::from("cyberpunk2077")),
        Some(&PathBuf::from("/games/cp2077"))
    );
}

#[tokio::test]
async fn cli_install_then_ui_sees_profile_and_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let settings_path = tmp.path().join("settings.toml");

    // 1. CLI creates profile
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    pm.create(&make_profile("3077", "cyberpunk2077"))
        .await
        .unwrap();

    // 2. CLI persists settings
    simulate_cli_install_settings(
        &settings_path,
        &GameId::from("cyberpunk2077"),
        Some(PathBuf::from("/games/cp2077")),
    );

    // 3. UI loads everything
    let mut settings = AppSettings::load_from(&settings_path);
    let (profiles, selected) = simulate_ui_init(&pm, &mut settings).await;

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].name, "3077");
    assert_eq!(selected, Some("cyberpunk2077".to_string()));
    assert_eq!(
        settings.game_path(&GameId::from("cyberpunk2077")),
        Some(&PathBuf::from("/games/cp2077"))
    );
}

#[test]
fn cli_install_without_game_dir_still_sets_selected_game() {
    let tmp = tempfile::tempdir().unwrap();
    let settings_path = tmp.path().join("settings.toml");

    simulate_cli_install_settings(&settings_path, &GameId::from("cyberpunk2077"), None);

    let loaded = AppSettings::load_from(&settings_path);
    assert_eq!(loaded.selected_game.as_deref(), Some("cyberpunk2077"));
    assert!(loaded.game_path(&GameId::from("cyberpunk2077")).is_none());
}

// ---------------------------------------------------------------------------
// Settings idempotency
// ---------------------------------------------------------------------------

#[test]
fn double_save_produces_identical_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");

    let mut s = AppSettings::default();
    s.set_game_path(
        &GameId::from("cyberpunk2077"),
        PathBuf::from("/games/cp2077"),
    );
    s.selected_game = Some("cyberpunk2077".into());

    s.save_to(&path);
    let content1 = std::fs::read_to_string(&path).unwrap();

    s.save_to(&path);
    let content2 = std::fs::read_to_string(&path).unwrap();

    assert_eq!(content1, content2);
}

#[test]
fn save_load_save_produces_identical_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("settings.toml");

    let mut s = AppSettings::default();
    s.set_game_path(
        &GameId::from("cyberpunk2077"),
        PathBuf::from("/games/cp2077"),
    );
    s.selected_game = Some("cyberpunk2077".into());
    s.nexus_api_key = "key123".into();

    s.save_to(&path);
    let content1 = std::fs::read_to_string(&path).unwrap();

    let loaded = AppSettings::load_from(&path);
    loaded.save_to(&path);
    let content2 = std::fs::read_to_string(&path).unwrap();

    assert_eq!(content1, content2);
}
