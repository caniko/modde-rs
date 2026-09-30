use super::*;
use crate::views::library::{LaunchField, LibraryLoadResult};
use modde_core::library::{LaunchSettings, LibraryPreferences};

#[test]
fn manage_mods_keeps_the_selected_installation_while_another_client_owns_mutations() {
    let _guard = db_lock();
    let mut app = test_app();
    app.active_view = View::Library;
    app.selected_game = Some("fallout4".into());
    let root = modde_core::paths::modde_data_dir();
    app.settings.set_game_path(
        &GameId::from("skyrim-se"),
        root.join("original-installation"),
    );
    app.library.entries = crate::views::library::build_game_entries(vec![
        crate::views::library::LibraryGameInstall {
            game_id: "skyrim-se".into(),
            display_name: "Skyrim".into(),
            install_path: root.join("another-installation"),
            source_label: "Local".into(),
        },
    ]);
    app.library.selected_id = Some(app.library.entries[0].id.clone());
    let lease = modde_core::library::mutation_lock().unwrap();

    let _ = app.update(Message::LibraryManageGame("skyrim-se".into()));

    assert!(matches!(app.active_view, View::Library));
    assert_eq!(app.selected_game.as_deref(), Some("fallout4"));
    assert_eq!(
        app.settings
            .game_path(&GameId::from("skyrim-se"))
            .map(PathBuf::as_path),
        Some(root.join("original-installation").as_path())
    );
    assert!(app.status_message.contains("resource is busy"));
    drop(lease);
}

#[test]
fn late_picker_cannot_overwrite_a_newer_edit_or_a_reselected_game() {
    let mut app = test_app();
    let _ = app.update(Message::LibrarySelectEntry("installation".into()));
    let revision = app.library.draft_revision;
    let _ = app.update(Message::LibraryLaunchFieldChanged(
        LaunchField::Executable,
        "/new/game".into(),
    ));
    let _ = app.update(Message::LibraryPathPicked {
        id: "installation".into(),
        revision,
        field: LaunchField::Executable,
        path: Some("/old/game".into()),
    });
    assert_eq!(
        app.library
            .draft
            .as_ref()
            .unwrap()
            .settings()
            .unwrap()
            .executable,
        Some("/new/game".into())
    );

    let revision = app.library.draft_revision;
    let _ = app.update(Message::LibrarySelectEntry("other".into()));
    let _ = app.update(Message::LibrarySelectEntry("installation".into()));
    let _ = app.update(Message::LibraryPathPicked {
        id: "installation".into(),
        revision,
        field: LaunchField::Executable,
        path: Some("/old/game".into()),
    });
    assert!(
        app.library
            .draft
            .as_ref()
            .unwrap()
            .settings()
            .unwrap()
            .executable
            .is_none()
    );
}

#[test]
fn import_result_keeps_newer_draft_changes() {
    let mut app = test_app();
    let _ = app.update(Message::LibrarySelectEntry("installation".into()));
    let revision = app.library.draft_revision;
    let _ = app.update(Message::LibrarySandboxChanged(true));
    let _ = app.update(Message::LibraryLaunchImported {
        id: "installation".into(),
        revision,
        result: Ok(Some(LaunchSettings::default())),
    });
    assert!(
        app.library
            .draft
            .as_ref()
            .unwrap()
            .settings()
            .unwrap()
            .sandbox
            .enabled
    );

    let revision = app.library.draft_revision;
    let _ = app.update(Message::LibraryLaunchImported {
        id: "installation".into(),
        revision,
        result: Ok(Some(LaunchSettings::default())),
    });
    assert!(
        !app.library
            .draft
            .as_ref()
            .unwrap()
            .settings()
            .unwrap()
            .sandbox
            .enabled
    );
}

#[test]
fn catalogue_result_cannot_replace_newer_session_polling_state() {
    let mut app = test_app();
    let session_revision = app.library.session_revision;
    let pending = serde_json::from_value(serde_json::json!({
        "installation": "installation", "name": "Game", "scope": "installation",
        "game_id": null, "profile": null, "save_directory": null, "capture": false, "phase": "running"
    })).unwrap();
    let _ = app.update(Message::LibrarySessionLoaded {
        revision: session_revision,
        result: Ok(Some(pending)),
    });
    let _ = app.update(Message::LibraryLoaded {
        generation: app.library.generation,
        session_revision,
        result: Ok(LibraryLoadResult {
            entries: Vec::new(),
            manager_config: None,
            manager_error: None,
            notices: Vec::new(),
            preferences: LibraryPreferences::default(),
            pending: None,
        }),
    });
    assert!(app.library.pending.is_some());
}

#[test]
fn switching_to_library_invalidates_an_older_session_poll() {
    let _guard = db_lock();
    let mut app = test_app();
    let revision = app.library.session_revision;
    let stale = serde_json::from_value(serde_json::json!({
        "installation": "old-installation", "name": "Old game", "scope": "old-installation",
        "game_id": null, "profile": null, "save_directory": null, "capture": false, "phase": "running"
    })).unwrap();
    // Navigation reads the current disk state synchronously. A prior poll may
    // arrive afterwards with a session that has already completed.
    let _ = app.update(Message::SwitchView(View::Library));
    assert!(app.library.pending.is_none());
    let _ = app.update(Message::LibrarySessionLoaded {
        revision,
        result: Ok(Some(stale)),
    });
    assert!(app.library.pending.is_none());
}
