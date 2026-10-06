#![allow(clippy::wildcard_imports)]
use super::*;

fn ready_wabbajack_report() -> modde_sources::wabbajack::readiness::WabbajackReadinessReport {
    modde_sources::wabbajack::readiness::WabbajackReadinessReport {
        manifest_path: "/tmp/list.wabbajack".to_string(),
        name: "Test List".to_string(),
        author: "tester".to_string(),
        version: "1.0".to_string(),
        game: "SkyrimSpecialEdition".to_string(),
        normalized_game: "skyrim-se".to_string(),
        profile_name: "test-list".to_string(),
        store_path: "/tmp/store".to_string(),
        archives: 0,
        directives: 0,
        archive_states: Default::default(),
        directive_types: Default::default(),
        archive_extensions: Default::default(),
        downloadable_archives: 0,
        store_present: 0,
        store_missing: Vec::new(),
        manual_downloads: Vec::new(),
        missing_nexus_archives: Vec::new(),
        game_file_sources: modde_sources::wabbajack::readiness::WabbajackGameFileSourceReport {
            total: 0,
            present: 0,
            missing: Vec::new(),
            mismatched: Vec::new(),
        },
        staging: modde_sources::wabbajack::readiness::WabbajackStagingReport {
            path: "/tmp/staging/test-list".to_string(),
            exists: false,
            compatible_layout: false,
            archive_batch_sentinels: 0,
            archive_batch_total: 0,
            create_bsa_sentinels: 0,
            create_bsa_total: 0,
            layout_action: "create".to_string(),
        },
        rar_enabled: true,
        nexus_required: false,
        nexus_available: true,
        hard_blockers: Vec::new(),
        warnings: Vec::new(),
        install_ready: true,
    }
}

fn wabbajack_catalog_entry(
    title: &str,
    game: &str,
    download_url: &str,
) -> modde_sources::wabbajack::catalog::WabbajackCatalogEntry {
    modde_sources::wabbajack::catalog::WabbajackCatalogEntry {
        title: title.to_string(),
        game: Some(game.to_string()),
        author: Some("tester".to_string()),
        version: Some("1.0".to_string()),
        tags: Vec::new(),
        image_url: None,
        readme_url: None,
        download_url: download_url.to_string(),
        repository_name: None,
        machine_url: Some("test-list".to_string()),
        discord_url: None,
        website_url: None,
        official: true,
        nsfw: false,
        force_down: false,
        size: Default::default(),
        source: modde_sources::wabbajack::catalog::CatalogEntrySource::Official,
    }
}

#[test]
fn wabbajack_e2e_sidebar_navigation_defaults_to_current_game() {
    let mut app = test_app();
    app.selected_game = Some("fallout4".to_string());
    let mut harness = AppUiHarness::new(app, Size::new(1100.0, 720.0));
    harness.rebuild();

    harness.click("Wabbajack");

    let View::WabbajackInstaller(state) = &harness.app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.game_filter.as_deref(), Some("fallout4"));
    assert!(!state.game_filter_user_edited);
}

#[test]
fn wabbajack_e2e_selecting_catalog_entry_prefills_install_target() {
    let mut app = test_app();
    let game_id = if cfg!(feature = "bethesda") {
        "skyrim-se"
    } else {
        "SkyrimSpecialEdition"
    };
    app.settings
        .set_game_path(&GameId::from(game_id), PathBuf::from("/games/skyrim"));
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        entries: vec![wabbajack_catalog_entry(
            "Legends of the Frost",
            "SkyrimSpecialEdition",
            "https://example.test/lotf.wabbajack",
        )],
        ..Default::default()
    });
    let mut harness = AppUiHarness::new(app, Size::new(1100.0, 720.0));
    harness.rebuild();

    harness.click(by_test_id("wabbajack.entry.0"));

    let View::WabbajackInstaller(state) = &harness.app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.selected_index, Some(0));
    assert_eq!(state.manual_source, "https://example.test/lotf.wabbajack");
    assert_eq!(state.hm_profile, "legends-of-the-frost");
    assert_eq!(state.hm_game, game_id);
    assert_eq!(state.hm_game_dir, "/games/skyrim");
}

#[test]
fn wabbajack_e2e_target_input_updates_state_and_rechecks_selected_file() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        ..Default::default()
    });
    let mut harness = AppUiHarness::new(app, Size::new(1100.0, 720.0));
    harness.rebuild();

    harness.click(by_test_id("wabbajack.input.hm_profile"));
    harness.typewrite("panel-profile");
    harness.click(by_test_id("wabbajack.input.hm_game_dir"));
    harness.typewrite("/custom/skyrim");

    let View::WabbajackInstaller(state) = &harness.app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.hm_profile, "panel-profile");
    assert_eq!(state.hm_game_dir, "/custom/skyrim");
    assert!(state.hm_game_dir_user_edited);
    assert!(state.readiness_loading);
    assert_eq!(state.status, "Checking Wabbajack readiness...");
}

#[test]
fn wabbajack_e2e_blocked_install_button_stays_inert_and_renders_blocker() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        ..Default::default()
    });
    let mut harness = AppUiHarness::new(app, Size::new(1100.0, 720.0));
    harness.rebuild();

    harness.find("Run a readiness check before installing.");
    harness.click_near_left(by_test_id("wabbajack.install"));

    let View::WabbajackInstaller(state) = &harness.app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert!(!state.installing);
    assert_eq!(state.install_phase, "");
}

#[test]
fn wabbajack_e2e_ready_install_control_renders_enabled_state() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        hm_profile: "panel-profile".to_string(),
        hm_game_dir: "/panel/game".to_string(),
        readiness: Some(ready_wabbajack_report()),
        ..Default::default()
    });
    let mut harness = AppUiHarness::new(app, Size::new(1100.0, 720.0));
    harness.rebuild();

    let View::WabbajackInstaller(state) = &harness.app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert!(state.can_install());
    harness.find(by_test_id("wabbajack.install"));
    harness.find("Ready to install.");
}

#[test]
fn wabbajack_e2e_progress_and_completion_render_through_app_view() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        installing: true,
        ..Default::default()
    });
    let mut harness = AppUiHarness::new(app, Size::new(1100.0, 720.0));
    harness.rebuild();

    harness.update_app(Message::WabbajackInstallEvent(
        WabbajackInstallEvent::Progress(
            modde_sources::wabbajack::installer::InstallProgress::Applying {
                directive_index: 3,
                total: 10,
            },
        ),
    ));
    harness.find("Applying: directive 4/10");
    harness.find("Applying directives: 4/10");

    harness.update_app(Message::WabbajackInstallEvent(
        WabbajackInstallEvent::Complete(Ok(WabbajackInstallUiSummary {
            status_message: "Installed 'Test List' to profile 'test-list' (2 mods)".to_string(),
        })),
    ));
    harness.find("Complete");
    harness.find("Installed 'Test List' to profile 'test-list' (2 mods)");
    let View::WabbajackInstaller(state) = &harness.app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert!(!state.installing);
    assert!((state.progress - 1.0).abs() <= f32::EPSILON);
}

#[test]
fn wabbajack_file_selection_starts_readiness_check() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState::default());

    let _ = app.update(Message::WabbajackFileSelected(PathBuf::from(
        "/tmp/list.wabbajack",
    )));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(
        state.file_path.as_deref(),
        Some(Path::new("/tmp/list.wabbajack"))
    );
    assert!(state.readiness_loading);
    assert_eq!(state.status, "Checking Wabbajack readiness...");
}

#[test]
fn wabbajack_target_changes_recheck_without_overwriting_manual_game_dir() {
    let mut app = test_app();
    app.settings
        .set_game_path(&GameId::from("skyrim-se"), PathBuf::from("/games/skyrim"));
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        hm_game: "skyrim-se".to_string(),
        hm_game_dir: "/custom/skyrim".to_string(),
        hm_game_dir_user_edited: true,
        ..Default::default()
    });

    let _ = app.update(Message::WabbajackHmGameChanged("skyrim-se".to_string()));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.hm_game_dir, "/custom/skyrim");
    assert!(state.hm_game_dir_user_edited);
    assert!(state.readiness_loading);
}

#[test]
fn wabbajack_start_install_requires_readiness_report() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        ..Default::default()
    });

    let task = app.update(Message::WabbajackStartInstall);

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    // No installer is emitted; the only task releases the mutation lease.
    assert_eq!(task.units(), 1);
    assert!(!state.installing);
    assert_eq!(state.status, "Run a readiness check before installing.");
}

#[test]
fn wabbajack_start_install_uses_panel_target_when_ready() {
    let mut app = test_app();
    app.active_profile = Some("active-profile".to_string());
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        hm_profile: "panel-profile".to_string(),
        hm_game_dir: "/panel/game".to_string(),
        readiness: Some(ready_wabbajack_report()),
        ..Default::default()
    });

    let task = app.update(Message::WabbajackStartInstall);

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    // Installer work plus deferred mutation-lease release.
    assert_eq!(task.units(), 2);
    assert!(state.installing);
    assert_eq!(state.install_phase, "Starting");
    assert_eq!(state.hm_profile, "panel-profile");
    assert_eq!(state.hm_game_dir, "/panel/game");
}

#[test]
fn wabbajack_live_progress_event_updates_state_and_log() {
    let mut app = test_app();
    app.active_view = View::WabbajackInstaller(WabbajackInstallerState::default());

    let _ = app.update(Message::WabbajackInstallEvent(
        WabbajackInstallEvent::Progress(
            modde_sources::wabbajack::installer::InstallProgress::Applying {
                directive_index: 3,
                total: 10,
            },
        ),
    ));

    let View::WabbajackInstaller(state) = &app.active_view else {
        panic!("expected Wabbajack installer view");
    };
    assert_eq!(state.install_phase, "Applying");
    assert_eq!(state.install_current_item, "directive 4/10");
    assert!((state.progress - 0.4).abs() < f32::EPSILON);
    assert!(
        state
            .log_lines
            .iter()
            .any(|line| line == "Applying directives: 4/10")
    );
}

#[test]
fn test_noop() {
    let mut app = test_app();
    let old = app.status_message.clone();
    let _ = app.update(Message::Noop);
    assert_eq!(app.status_message, old);
}

#[test]
fn nested_tool_setting_round_trips_dotted_path() {
    let mut settings = serde_json::json!({
        "ini_overrides": {
            "Menu.Scale": "1.0"
        }
    });

    set_nested_tool_setting(
        &mut settings,
        "ini_overrides.Menu.Scale",
        serde_json::json!(1.3),
    );

    assert_eq!(
        get_tool_setting_value(&settings, "ini_overrides.Menu.Scale"),
        Some(&serde_json::json!(1.3))
    );
    assert!(
        settings
            .get("ini_overrides")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|map| !map.contains_key("Menu.Scale"))
    );
}

#[test]
fn normalizes_legacy_boolean_and_numeric_tool_settings() {
    let settings = serde_json::json!({
        "enabled_flag": "yes",
        "amount": "1.25"
    });
    let specs = vec![
        modde_games::tools::ToolSettingSpec::bool("enabled_flag", "Enabled", ""),
        modde_games::tools::ToolSettingSpec::number("amount", "Amount", "", 0.0, 2.0, 0.05),
    ];

    let normalized = normalize_tool_settings_for_specs(&settings, &specs);

    assert_eq!(
        normalized.get("enabled_flag"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(normalized.get("amount"), Some(&serde_json::json!(1.25)));
}

#[test]
fn normalizes_nested_legacy_boolean_and_preserves_tri_state_auto() {
    let settings = serde_json::json!({
        "ini_overrides": {
            "FSR.Fsr4Update": "0",
            "Spoofing": {
                "Dxgi": "auto"
            }
        }
    });
    let specs = vec![
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.FSR.Fsr4Update",
            "FSR4 update",
            "",
        ),
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.Spoofing.Dxgi",
            "DXGI",
            "",
        ),
    ];

    let normalized = normalize_tool_settings_for_specs(&settings, &specs);

    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.FSR.Fsr4Update"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.Spoofing.Dxgi"),
        Some(&serde_json::json!("auto"))
    );
}
