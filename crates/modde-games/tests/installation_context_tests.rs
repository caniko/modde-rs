#![cfg(feature = "stardew")]

use modde_core::library::{LaunchSettings, LibraryPreferences};
use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::save::SaveManager;
use modde_core::{GameId, ModdeDb};
use modde_games::library::{LibraryGame, Store, context};

#[tokio::test]
async fn adopted_scope_survives_legacy_state_and_duplicate_installs_require_separate_saves() {
    let root = tempfile::tempdir().unwrap();
    modde_core::paths::set_data_dir(root.path().join("data"));
    modde_core::paths::set_config_dir(root.path().join("config"));
    let install = root.path().join("game-a");
    let other_install = root.path().join("game-b");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&other_install).unwrap();
    let game = LibraryGame::new(
        Store::Local,
        "stardew-valley".into(),
        "Stardew Valley".into(),
        Some(install),
    );
    let other = LibraryGame::new(
        Store::Local,
        "stardew-valley".into(),
        "Stardew Valley".into(),
        Some(other_install),
    );
    // Isolate game XDG independently from the modde control-plane config root.
    LibraryPreferences::update(|preferences| {
        for entry in [&game, &other] {
            preferences
                .launches
                .entry(entry.id.clone())
                .or_default()
                .environment
                .insert(
                    "XDG_CONFIG_HOME".into(),
                    root.path()
                        .join("game-config")
                        .to_string_lossy()
                        .into_owned(),
                );
        }
    })
    .unwrap();
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let game_id = GameId::from("stardew-valley");
    let profile = Profile {
        id: None,
        name: "adopted".into(),
        game_id: game_id.clone(),
        source: ProfileSource::Manual,
        mods: Vec::new(),
        overrides: root.path().join("overrides"),
        load_order_rules: Default::default(),
        load_order_lock: None,
    };
    let profile_id = pm.create(&profile).await.unwrap();
    let first = context::for_installation(&game, std::slice::from_ref(&game), pm.db())
        .await
        .unwrap();
    let saves = first.saves.directory.as_ref().unwrap();
    std::fs::create_dir_all(saves).unwrap();
    std::fs::write(saves.join("slot.sav"), "scoped-save").unwrap();
    SaveManager::new(pm.db())
        .adopt(&first.saves.scope, "adopted", saves)
        .unwrap();
    pm.db()
        .set_active_profile(&first.saves.scope, profile_id)
        .await
        .unwrap();

    // A legacy active slot appearing later must not redirect an adopted vault.
    pm.db()
        .set_active_profile(&game_id, profile_id)
        .await
        .unwrap();
    let again = context::for_installation(&game, std::slice::from_ref(&game), pm.db())
        .await
        .unwrap();
    assert_eq!(again.saves.scope, first.saves.scope);
    assert_ne!(again.saves.scope, game_id);
    assert!(
        !LibraryPreferences::load()
            .unwrap()
            .legacy_save_bindings
            .contains_key("stardew-valley")
    );

    let games = [game.clone(), other.clone()];
    assert!(
        context::for_installation(&game, &games, pm.db())
            .await
            .is_err()
    );
    let other_saves = root.path().join("other-saves");
    LibraryPreferences::update(|preferences| {
        preferences.launches.insert(
            other.id.clone(),
            LaunchSettings {
                save_directory: Some(other_saves.clone()),
                ..Default::default()
            },
        );
    })
    .unwrap();
    let selected = context::for_installation(&game, &games, pm.db())
        .await
        .unwrap();
    let second = context::for_installation(&other, &games, pm.db())
        .await
        .unwrap();
    assert_eq!(selected.saves.scope, first.saves.scope);
    assert_ne!(selected.saves.scope, second.saves.scope);
    assert!(pm.active(&second.saves.scope).await.unwrap().is_none());

    pm.create(&Profile {
        name: "candidate".into(),
        ..profile.clone()
    })
    .await
    .unwrap();
    std::fs::write(saves.join("slot.sav"), "before experiment").unwrap();
    // A real parking failure occurs after the durable snapshot marker. The
    // failed action must block retry until recovery restores the original state.
    std::fs::write(
        saves.join(".modde"),
        "cannot create a parking directory here",
    )
    .unwrap();
    assert!(selected.try_profile(&pm, "candidate", None).await.is_err());
    let pending = modde_core::library::PendingSession::load()
        .unwrap()
        .unwrap();
    assert_eq!(
        pending.phase,
        modde_core::library::SessionPhase::SwitchingSaves
    );
    assert!(selected.try_profile(&pm, "candidate", None).await.is_err());
    std::fs::remove_file(saves.join(".modde")).unwrap();
    pending.restore_preparation(&pm).await.unwrap();
    modde_core::library::PendingSession::clear().unwrap();
    assert_eq!(
        pm.db()
            .experiment_depth(&selected.saves.scope)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        std::fs::read_to_string(saves.join("slot.sav")).unwrap(),
        "before experiment"
    );
    selected.try_profile(&pm, "candidate", None).await.unwrap();
    assert_eq!(
        pm.db()
            .experiment_depth(&selected.saves.scope)
            .await
            .unwrap(),
        1
    );
    assert!(
        modde_core::library::PendingSession::load()
            .unwrap()
            .is_none()
    );
    std::fs::write(saves.join("slot.sav"), "candidate save").unwrap();
    selected.rollback_profile(&pm, None).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(saves.join("slot.sav")).unwrap(),
        "before experiment"
    );
    assert_eq!(
        pm.db()
            .experiment_depth(&selected.saves.scope)
            .await
            .unwrap(),
        0
    );
    assert!(
        modde_core::library::PendingSession::load()
            .unwrap()
            .is_none()
    );
    let snapshot = SaveManager::history(&selected.saves.scope, "adopted", 1).unwrap()[0]
        .id
        .clone();
    std::fs::write(saves.join("slot.sav"), "after snapshot").unwrap();
    selected
        .restore_saves(&pm, "adopted", &snapshot, None)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(saves.join("slot.sav")).unwrap(),
        "before experiment"
    );
    assert!(
        modde_core::library::PendingSession::load()
            .unwrap()
            .is_none()
    );

    // Changing a destination is a new scope, even on the same installation.
    LibraryPreferences::update(|preferences| {
        preferences.launches.insert(
            game.id.clone(),
            LaunchSettings {
                save_directory: Some(root.path().join("moved-saves")),
                ..Default::default()
            },
        );
    })
    .unwrap();
    let moved = context::for_installation(&game, &games, pm.db())
        .await
        .unwrap();
    assert_ne!(moved.saves.scope, first.saves.scope);
    assert!(pm.active(&moved.saves.scope).await.unwrap().is_none());
    assert!(
        !SaveManager::history(&first.saves.scope, "adopted", 1)
            .unwrap()
            .is_empty()
    );

    let config = root.path().join("native-config");
    let mut native = LaunchSettings {
        executable: Some(game.install_path.as_ref().unwrap().join("native-game")),
        ..Default::default()
    };
    native.environment.insert(
        "XDG_CONFIG_HOME".into(),
        config.to_string_lossy().into_owned(),
    );
    let mut preferences = LibraryPreferences::load().unwrap();
    preferences.launches.insert(game.id.clone(), native.clone());
    let plain = context::effective_settings(&game, &games, &preferences).unwrap();
    native.sandbox.enabled = true;
    preferences.launches.insert(game.id.clone(), native);
    let sandboxed = context::effective_settings(&game, &games, &preferences).unwrap();
    assert_eq!(
        plain.save_directory,
        Some(config.join("StardewValley/Saves"))
    );
    assert_eq!(sandboxed.save_directory, plain.save_directory);
    let before = LibraryPreferences::load().unwrap();
    let transient = context::with_launch_settings(&game, &games, pm.db(), plain)
        .await
        .unwrap();
    assert_eq!(
        transient.saves.directory,
        Some(config.join("StardewValley/Saves"))
    );
    assert_ne!(transient.saves.scope, moved.saves.scope);
    assert_eq!(
        LibraryPreferences::load().unwrap(),
        before,
        "one-run overrides must not overwrite saved settings"
    );
}
