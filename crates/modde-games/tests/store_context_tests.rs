#![cfg(feature = "stardew")]

use modde_core::library::{LaunchSettings, LibraryPreferences};
use modde_games::library::{LibraryGame, Store, context};

#[tokio::test]
async fn native_store_boundary_does_not_reuse_a_stale_proton_prefix() {
    let root = tempfile::tempdir().unwrap();
    modde_core::paths::set_data_dir(root.path().join("data"));
    modde_core::paths::set_config_dir(root.path().join("config"));
    let install = root.path().join("steamapps/common/Stardew Valley");
    let old_prefix = root.path().join("steamapps/compatdata/413150/pfx");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&old_prefix).unwrap();
    let game = LibraryGame::new(Store::Steam, "413150".into(), "Stardew Valley".into(), Some(install));
    let config = root.path().join("game-config");
    let mut settings = LaunchSettings { store_hook: true, ..Default::default() };
    settings.environment.insert("XDG_CONFIG_HOME".into(), config.to_string_lossy().into_owned());
    LibraryPreferences::update(|preferences| { preferences.launches.insert(game.id.clone(), settings.clone()); }).unwrap();
    let db = modde_core::ModdeDb::open_memory().await.unwrap();
    let inferred = context::for_installation(&game, std::slice::from_ref(&game), &db).await.unwrap();
    assert_eq!(inferred.prefix, Some(old_prefix));

    let before = LibraryPreferences::load().unwrap();
    let native = context::with_store_settings(&game, std::slice::from_ref(&game), &db, settings.clone(), None).await.unwrap();
    assert!(native.prefix.is_none());
    assert!(native.launch.prefix.is_none());
    assert_eq!(native.saves.directory, Some(config.join("StardewValley/Saves")));
    assert_ne!(native.saves.scope, inferred.saves.scope);

    let first_run = root.path().join("new-prefix");
    let wine = context::with_store_settings(&game, std::slice::from_ref(&game), &db, settings, Some(first_run.clone())).await.unwrap();
    assert_eq!(wine.prefix, Some(first_run.clone()));
    assert_eq!(wine.saves.directory, Some(first_run.join("drive_c/users/steamuser/AppData/Roaming/StardewValley/Saves")));
    assert_eq!(LibraryPreferences::load().unwrap(), before);

    #[cfg(feature = "cyberpunk")]
    {
        // Windows-only save discovery must not turn an explicit native boundary
        // back into a stale Steam prefix, including a recorded legacy binding.
        let install = root.path().join("steamapps/common/Cyberpunk");
        let prefix = root.path().join("steamapps/compatdata/1091500/pfx");
        let stale = prefix.join("drive_c/users/steamuser/Saved Games/CD Projekt Red/Cyberpunk 2077");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(&stale).unwrap();
        let game = LibraryGame::new(Store::Steam, "1091500".into(), "Cyberpunk 2077".into(), Some(install));
        LibraryPreferences::update(|preferences| {
            preferences.legacy_save_bindings.insert("cyberpunk2077".into(), modde_core::library::LegacySaveBinding {
                installation: game.id.clone(), save_directory: Some(stale),
            });
        }).unwrap();
        let native = context::with_store_settings(&game, std::slice::from_ref(&game), &db,
            LaunchSettings { store_hook: true, ..Default::default() }, None).await.unwrap();
        assert!(native.prefix.is_none());
        assert!(native.saves.directory.is_none());
    }
}
