#![allow(clippy::wildcard_imports)]
use super::*;

#[test]
fn tools_hover_toast_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let before = scroll_tools_panel_down(&mut harness);

    harness.update_app(Message::ButtonHoverStarted {
        id: 99,
        description: "Clear OptiScaler INI overrides so the selected release defaults are used.",
    });
    harness.update_app(Message::ButtonHoverElapsed { id: 99 });
    harness.find("Clear OptiScaler INI overrides so the selected release defaults are used.");

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn tools_actual_button_hover_toast_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let (target, before) = scroll_until_visible(&mut harness, "tools.optiscaler.reset_config");
    let center = target
        .visible_bounds()
        .expect("reset config should be visible after scrolling")
        .center();

    harness.move_cursor_to(center);
    let pending = harness
        .app
        .button_hover_toast
        .pending
        .expect("hovering the described button should start hover toast state");
    assert_eq!(pending.description, "No INI overrides to reset.");

    harness.update_app(Message::ButtonHoverElapsed { id: pending.id });
    harness.find("No INI overrides to reset.");

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn tools_described_button_click_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let before = scroll_tools_panel_down(&mut harness);

    harness.click(by_test_id("tools.optiscaler.apply"));

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn tools_optiscaler_state_button_click_does_not_reset_scroll_position() {
    let mut harness = AppUiHarness::new(scroll_heavy_tools_app(), Size::new(900.0, 420.0));
    harness.rebuild();
    let (_, before) = scroll_until_visible(&mut harness, "tools.optiscaler.reset_config");

    harness.click(by_test_id("tools.optiscaler.reset_config"));

    assert_scroll_y_unchanged(&mut harness, before);
}

#[test]
fn test_switch_view_saves_unsupported_game_does_not_load_save_state() {
    let mut app = test_app();
    app.selected_game = Some("not-a-game".to_string());
    app.save_snapshots = vec![SaveSnapshot {
        id: "abc123".to_string(),
        message: "capture".to_string(),
        timestamp: 0,
        file_count: 1,
        fingerprint: None,
        profile_name: None,
        character_name: None,
        save_label: None,
        category: None,
    }];
    app.current_fingerprint = Some(modde_core::save::SaveFingerprint::empty());

    let _ = app.update(Message::SwitchView(View::Saves));

    assert!(matches!(app.active_view, View::Saves));
    assert!(app.save_snapshots.is_empty());
    assert!(app.current_fingerprint.is_none());
    assert_eq!(
        app.status_message,
        "Save profiles are not supported for this game"
    );
}

#[test]
fn test_switch_view_diagnostics_requires_profile() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchView(View::Diagnostics));
    assert!(matches!(app.active_view, View::Diagnostics));
    assert!(matches!(
        app.diagnostics_state,
        crate::views::diagnostics::DiagnosticsState::Error(_)
    ));
}

#[test]
fn diagnostics_task_completion_sets_complete_state() {
    let mut app = test_app();
    app.loaded_profile = Some(profile_for_game("diagnostics-profile", "skyrim-se", vec![]));

    let task = app.update(Message::RunDiagnostics);

    assert_eq!(task.units(), 1);
    assert_eq!(app.diagnostics_generation, 1);
    assert!(matches!(
        app.diagnostics_state,
        crate::views::diagnostics::DiagnosticsState::Running
    ));

    let profile = app.loaded_profile.clone().expect("profile loaded");
    let result = crate::app::block_on(super::model::load_diagnostics(app.db.clone(), profile));
    let generation = app.diagnostics_generation;
    let _ = app.update(Message::DiagnosticsComputed { generation, result });

    assert!(matches!(
        app.diagnostics_state,
        crate::views::diagnostics::DiagnosticsState::Complete(_)
    ));
    assert!(app.status_message.starts_with("Diagnostics complete:"));
}

#[test]
fn test_switch_profile() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchProfile("test-profile".to_string()));
    assert_eq!(app.active_profile.as_deref(), Some("test-profile"));
    assert_eq!(app.status_message, "Profile switched");
}

#[test]
fn browse_nexus_defaults_to_loaded_profile_before_selected_game() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());
    app.loaded_profile = Some(profile_for_game("browse-profile", "skyrim-se", vec![]));

    app.sync_browse_game_to_current(true);

    assert_eq!(
        app.browse_nexus.selected_game_id.as_deref(),
        Some("skyrim-se")
    );
}

#[test]
fn browse_nexus_game_change_clears_results_and_starts_load() {
    let mut app = test_app();
    app.browse_nexus.selected_game_id = Some("skyrim-se".to_string());
    app.browse_nexus.mods = vec![modde_sources::nexus::graphql::GqlModTile {
        mod_id: 1.into(),
        name: "SkyUI".to_string(),
        summary: None,
        version: None,
        author: None,
        picture_url: None,
        thumbnail_url: None,
        endorsements: None,
        downloads: None,
        uploaded_at: None,
        game_domain: Some("skyrimspecialedition".to_string()),
    }];
    app.browse_nexus.error = Some("stale".to_string());
    app.browse_nexus.install_status = Some("stale install".to_string());

    let _ = app.update(Message::BrowseGameChanged(Some("fallout4".to_string())));

    assert_eq!(
        app.browse_nexus.selected_game_id.as_deref(),
        Some("fallout4")
    );
    assert!(app.browse_nexus.mods.is_empty());
    assert!(app.browse_nexus.collections.is_empty());
    assert!(app.browse_nexus.error.is_none());
    assert!(app.browse_nexus.install_status.is_none());
    assert!(app.browse_nexus.loading);
}

#[test]
fn browse_nexus_excludes_games_without_verified_numeric_id() {
    assert_eq!(
        Modde::nexus_domain_for_game("baldurs-gate3"),
        None,
        "new games with unverified Nexus numeric IDs should not be browse targets yet"
    );
    assert_eq!(
        Modde::nexus_domain_for_game("skyrim-se").as_deref(),
        Some("skyrimspecialedition")
    );
}

#[test]
fn wabbajack_defaults_to_current_game_on_enter() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());

    let _ = app.update(Message::SwitchView(View::WabbajackInstaller(
        WabbajackInstallerState::default(),
    )));

    let View::WabbajackInstaller(state) = app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.game_filter.as_deref(), Some("fallout4"));
    assert!(!state.game_filter_user_edited);
}

#[test]
fn wabbajack_keeps_user_selected_all_games_on_enter() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());
    let state = WabbajackInstallerState {
        game_filter: None,
        game_filter_user_edited: true,
        ..Default::default()
    };

    let _ = app.update(Message::SwitchView(View::WabbajackInstaller(state)));

    let View::WabbajackInstaller(state) = app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert!(state.game_filter.is_none());
    assert!(state.game_filter_user_edited);
}

#[test]
fn test_filter_changed() {
    let mut app = test_app();
    let _ = app.update(Message::FilterChanged("skyui".to_string()));
    assert_eq!(app.mod_filter, "skyui");
}

#[test]
fn test_select_mod() {
    let mut app = test_app();
    let _ = app.update(Message::SelectMod(3));
    assert_eq!(app.selected_mod_index, Some(3));
}

#[test]
fn test_deploy_complete_ok() {
    let mut app = test_app();
    let _ = app.update(Message::DeployComplete(Ok("Deployed 5 mods".to_string())));
    assert!(app.status_message.contains("Deployed"));
}

#[test]
fn test_deploy_complete_err() {
    let mut app = test_app();
    let _ = app.update(Message::DeployComplete(Err("game not found".to_string())));
    assert!(app.status_message.contains("Deploy failed"));
}

#[test]
fn test_set_nexus_api_key_draft() {
    let mut app = test_app();
    let _ = app.update(Message::SetNexusApiKeyDraft("abc123".to_string()));
    assert_eq!(app.nexus_api_key_draft, "abc123");
    assert!(app.settings.nexus_api_key.is_empty());
}

#[test]
fn test_toggle_nexus_api_key_visibility() {
    let mut app = test_app();
    assert!(!app.nexus_api_key_visible);
    let _ = app.update(Message::ToggleNexusApiKeyVisibility);
    assert!(app.nexus_api_key_visible);
}

#[test]
fn test_set_theme() {
    let mut app = test_app();
    let _ = app.update(Message::SetTheme("Nord".to_string()));
    assert_eq!(app.theme_name, "Nord");
    assert_eq!(app.settings.theme, "Nord");
}

#[test]
fn test_theme_returns_correct_variant() {
    let mut app = test_app();
    assert_eq!(app.theme(), Theme::Dark);
    app.theme_name = "Light".to_string();
    assert_eq!(app.theme(), Theme::Light);
    app.theme_name = "Nord".to_string();
    assert_eq!(app.theme(), Theme::Nord);
    app.theme_name = "Dracula".to_string();
    assert_eq!(app.theme(), Theme::Dracula);
    app.theme_name = "Gruvbox Dark".to_string();
    assert_eq!(app.theme(), Theme::GruvboxDark);
    app.theme_name = "Catppuccin Mocha".to_string();
    assert_eq!(app.theme(), Theme::CatppuccinMocha);
}

#[test]
fn test_new_profile_name_changed() {
    let mut app = test_app();
    let _ = app.update(Message::NewProfileNameChanged("my-profile".to_string()));
    assert_eq!(app.new_profile_name, "my-profile");
}

#[test]
fn test_open_new_profile_dialog() {
    let mut app = test_app();
    let _ = app.update(Message::OpenNewProfileDialog);
    assert!(app.new_profile_dialog_open);
}

#[test]
fn test_cancel_new_profile_dialog() {
    let mut app = test_app();
    app.new_profile_dialog_open = true;
    app.new_profile_name = "draft".to_string();

    let _ = app.update(Message::CancelNewProfileDialog);

    assert!(!app.new_profile_dialog_open);
    assert!(app.new_profile_name.is_empty());
}

#[test]
fn test_submit_new_profile_rejects_blank_name() {
    let mut app = test_app();
    app.new_profile_dialog_open = true;
    app.selected_game = Some("skyrim-se".to_string());
    app.new_profile_name = "   ".to_string();

    let _ = app.update(Message::SubmitNewProfileDialog);

    assert!(app.new_profile_dialog_open);
    assert!(app.status_message.contains("Profile name is required"));
}

#[test]
fn test_submit_new_profile_creates_profile_and_closes_dialog() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.new_profile_dialog_open = true;
    app.selected_game = Some("skyrim-se".to_string());
    app.new_profile_name = "  ui-profile-create  ".to_string();

    let _ = app.update(Message::SubmitNewProfileDialog);
    complete_create_profile_write(&mut app, "ui-profile-create", "skyrim-se");

    assert!(!app.new_profile_dialog_open);
    assert!(app.new_profile_name.is_empty());
    assert_eq!(app.active_profile.as_deref(), Some("ui-profile-create"));
    assert_eq!(app.status_message, "Profile created");
}

#[test]
fn stale_profile_write_done_is_ignored() {
    let mut app = test_app();
    app.context_generation = 2;
    app.active_profile = Some("current".to_string());

    let _ = app.update(Message::ProfileWriteDone {
        generation: 1,
        kind: ProfileWriteKind::Create {
            name: "stale".to_string(),
            game_id: "skyrim-se".to_string(),
        },
        result: Ok(ProfileWriteOutcome {
            status_message: Some("Profile created".to_string()),
            reload: true,
        }),
    });

    assert_eq!(app.active_profile.as_deref(), Some("current"));
    assert_eq!(app.status_message, "Ready");
}

#[test]
#[cfg(feature = "cyberpunk")]
fn experiment_try_then_commit_write_completion_updates_state() {
    let _guard = db_lock();
    reset_isolated_db();
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    let install = tempfile::tempdir().expect("installation");
    let saves = tempfile::tempdir().expect("saves");
    crate::app::block_on(pm.create(&profile_for_game(
        "experiment-profile",
        "cyberpunk2077",
        Vec::new(),
    )))
    .expect("seed experiment profile");
    let loaded =
        crate::app::block_on(pm.load("experiment-profile", Some(&GameId::from("cyberpunk2077"))))
            .expect("load experiment profile");

    let mut app = test_app();
    app.settings
        .set_game_path(&GameId::from("cyberpunk2077"), install.path().into());
    let game = modde_games::library::catalogue(&app.settings)
        .unwrap()
        .games
        .into_iter()
        .find(|game| game.install_path.as_deref() == Some(install.path()))
        .unwrap();
    modde_core::library::LibraryPreferences::update(|prefs| {
        prefs.launches.insert(
            game.id,
            modde_core::library::LaunchSettings {
                save_directory: Some(saves.path().into()),
                ..Default::default()
            },
        );
    })
    .unwrap();
    let context = crate::app::block_on(modde_games::library::context::for_game(
        &app.settings,
        "cyberpunk2077",
        pm.db(),
    ))
    .unwrap();
    crate::app::block_on(pm.activate_scoped("experiment-profile", &context.saves, None)).unwrap();
    drop(pm);
    app.selected_game = Some("cyberpunk2077".to_string());
    app.active_profile = Some("experiment-profile".to_string());
    app.loaded_profile = Some(loaded);

    let task = app.update(Message::TryProfile);
    assert_eq!(task.units(), 2); // operation and lease-release completion
    drop(task); // completion is driven explicitly by the fixture below
    complete_experiment_write(
        &mut app,
        ExperimentWriteKind::Try,
        Some("experiment-profile"),
        "cyberpunk2077",
    );
    assert_eq!(app.experiment_depth, 1);
    assert_eq!(app.status_message, "Experiment started (depth 1)");

    let task = app.update(Message::CommitExperiment);
    assert_eq!(task.units(), 2);
    drop(task);
    complete_experiment_write(&mut app, ExperimentWriteKind::Commit, None, "cyberpunk2077");
    assert_eq!(app.experiment_depth, 0);
    assert_eq!(app.status_message, "Experiment committed");
}

#[test]
fn test_select_game() {
    let mut app = test_app();
    let _ = app.update(Message::SelectGame("missing-game".to_string()));
    assert_eq!(app.selected_game, Some("missing-game".to_string()));
    assert!(app.game_path_dialog_open);
    assert_eq!(
        app.pending_game_path_game_id.as_deref(),
        Some("missing-game")
    );
}

#[test]
fn wabbajack_select_entry_prefills_profiled_game_dir() {
    let mut app = test_app();
    app.settings
        .set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/games/skyrim"));
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        entries: vec![modde_sources::wabbajack::catalog::WabbajackCatalogEntry {
            title: "Legends of the Frost".to_string(),
            game: Some("SkyrimSpecialEdition".to_string()),
            author: None,
            version: None,
            tags: Vec::new(),
            image_url: None,
            readme_url: None,
            download_url: "https://example/lotf.wabbajack".to_string(),
            repository_name: None,
            machine_url: None,
            discord_url: None,
            website_url: None,
            official: true,
            nsfw: false,
            force_down: false,
            size: Default::default(),
            source: modde_sources::wabbajack::catalog::CatalogEntrySource::Official,
        }],
        ..Default::default()
    });

    let _ = app.update(Message::WabbajackSelectEntry(0));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.hm_profile, "legends-of-the-frost");
    assert_eq!(state.hm_game, "skyrim-se");
    assert_eq!(state.hm_game_dir, "/games/skyrim");
    assert!(!state.hm_game_dir_user_edited);
}

#[test]
fn wabbajack_game_dir_manual_edit_is_not_overwritten() {
    let mut app = test_app();
    app.settings
        .set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/games/skyrim"));
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState::default());

    let _ = app.update(Message::WabbajackHmGameDirChanged(
        "/custom/skyrim".to_string(),
    ));
    let _ = app.update(Message::WabbajackHmGameChanged("skyrim-se".to_string()));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.hm_game_dir, "/custom/skyrim");
    assert!(state.hm_game_dir_user_edited);
}
