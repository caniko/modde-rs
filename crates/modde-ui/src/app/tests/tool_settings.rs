#![allow(clippy::wildcard_imports)]
use super::*;

#[test]
fn labeled_select_setting_preserves_raw_value() {
    let settings = serde_json::json!({
        "ini_overrides": {
            "fakenvapi": {
                "force_reflex": "2"
            },
            "Menu": {
                "ShortcutKey": "INSERT"
            },
            "FSR": {
                "UpscalerIndex": "0",
                "FGIndex": "1"
            }
        }
    });
    let specs = vec![
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.fakenvapi.force_reflex",
            "Force Reflex",
            "",
            &[
                ("0", "0 - Follow in-game setting"),
                ("1", "1 - Force disable"),
                ("2", "2 - Force enable"),
            ],
        ),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.Menu.ShortcutKey",
            "Menu shortcut",
            "",
            &[("auto", "Auto"), ("INSERT", "Insert")],
        ),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.UpscalerIndex",
            "FSR upscaler backend",
            "",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.2"),
                ("1", "1 - FSR 3.1.5"),
                ("2", "2 - FSR 2.3.4"),
            ],
        ),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.FGIndex",
            "FSR frame generation backend",
            "",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.0"),
                ("1", "1 - FSR 3.1.6"),
            ],
        ),
    ];

    let normalized = normalize_tool_settings_for_specs(&settings, &specs);

    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.fakenvapi.force_reflex"),
        Some(&serde_json::json!("2"))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.Menu.ShortcutKey"),
        Some(&serde_json::json!("INSERT"))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.FSR.UpscalerIndex"),
        Some(&serde_json::json!("0"))
    );
    assert_eq!(
        get_tool_setting_value(&normalized, "ini_overrides.FSR.FGIndex"),
        Some(&serde_json::json!("1"))
    );
}

#[test]
fn tool_apply_signature_ignores_internal_apply_state() {
    let settings = serde_json::json!({
        "source_mode": "local_dir",
        "_game_id": "stellar-blade",
        "_last_applied_settings": { "old": true },
        "managed_manifest": [{ "path": "dxgi.dll" }]
    });

    assert_eq!(
        tool_apply_signature(&settings),
        serde_json::json!({ "source_mode": "local_dir" })
    );
    assert!(tool_apply_is_pending(
        &modde_games::tools::ToolConfig {
            tool_id: "optiscaler".to_string(),
            enabled: true,
            settings,
        },
        &["dxgi.dll".to_string()]
    ));
}

#[test]
#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn apply_tool_marks_tool_busy_immediately() {
    let _guard = db_lock();
    reset_isolated_db();
    let temp = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.settings
        .set_game_path(&GameId::from("skyrim-se"), temp.path().to_path_buf());

    let _ = app.update(Message::ApplyTool("optiscaler".to_string()));

    assert!(app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.status_message, "Applying OptiScaler...");
}

#[test]
fn optiscaler_activate_marks_tool_busy_immediately() {
    let _guard = db_lock();
    reset_isolated_db();
    let temp = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.settings
        .set_game_path(&GameId::from("skyrim-se"), temp.path().to_path_buf());

    let _ = app.update(Message::ActivateOptiScaler);

    assert!(app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.status_message, "Activating OptiScaler...");
}

#[test]
fn optiscaler_deactivate_marks_tool_busy_immediately() {
    let _guard = db_lock();
    reset_isolated_db();
    let temp = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.settings
        .set_game_path(&GameId::from("skyrim-se"), temp.path().to_path_buf());

    let _ = app.update(Message::DeactivateOptiScaler);

    assert!(app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.status_message, "Deactivating OptiScaler...");
}

#[test]
fn tool_apply_failure_clears_busy_state() {
    let mut app = test_app();
    app.tool_state
        .active_operations
        .insert("optiscaler".to_string());

    let _ = app.update(Message::ToolApplied {
        tool_id: "optiscaler".to_string(),
        result: Err("validation failed".to_string()),
    });

    assert!(!app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(
        app.status_message,
        "Failed to apply tool: validation failed"
    );
}

#[test]
fn tool_apply_success_clears_busy_state_and_preserves_active_tab() {
    let mut app = test_app();
    app.tool_state
        .active_operations
        .insert("optiscaler".to_string());

    let _ = app.update(Message::ToolApplied {
        tool_id: "optiscaler".to_string(),
        result: Ok(ToolApplyResult {
            display_name: "OptiScaler".to_string(),
            applied_file_count: 2,
            validation_message: Some("validated managed install".to_string()),
        }),
    });

    assert!(!app.tool_state.is_tool_busy("optiscaler"));
    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("optiscaler"));
    assert_eq!(
        app.status_message,
        "Applied OptiScaler (2 file(s)); validated managed install"
    );
}

#[test]
fn tool_settings_restore_success_preserves_active_tab() {
    let mut app = test_app();

    let _ = app.update(Message::ToolSettingsRestored {
        tool_id: "mangohud".to_string(),
        result: Ok("Restored MangoHud settings version".to_string()),
    });

    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("mangohud"));
    assert_eq!(app.status_message, "Restored MangoHud settings version");
}

#[test]
fn optiscaler_apply_validation_accepts_core_files() {
    let temp = tempfile::tempdir().expect("game dir");
    std::fs::write(temp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    std::fs::write(temp.path().join("OptiScaler.ini"), b"[FSR]\n").expect("ini");
    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    let applied = modde_games::tools::AppliedFiles {
        files: vec![PathBuf::from("dxgi.dll"), PathBuf::from("OptiScaler.ini")],
    };

    validate_optiscaler_apply("test-game", temp.path(), &config, &applied)
        .expect("validation should pass");
}

#[test]
fn optiscaler_apply_validation_rejects_missing_proxy() {
    let temp = tempfile::tempdir().expect("game dir");
    std::fs::write(temp.path().join("OptiScaler.ini"), b"[FSR]\n").expect("ini");
    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    let applied = modde_games::tools::AppliedFiles {
        files: vec![PathBuf::from("OptiScaler.ini")],
    };

    let err = validate_optiscaler_apply("test-game", temp.path(), &config, &applied)
        .expect_err("missing proxy should fail");
    assert!(err.contains("missing configured proxy DLL dxgi.dll"));
}

#[test]
fn optiscaler_apply_validation_rejects_missing_ini() {
    let temp = tempfile::tempdir().expect("game dir");
    std::fs::write(temp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    let applied = modde_games::tools::AppliedFiles {
        files: vec![PathBuf::from("dxgi.dll")],
    };

    let err = validate_optiscaler_apply("test-game", temp.path(), &config, &applied)
        .expect_err("missing ini should fail");
    assert!(err.contains("missing OptiScaler.ini"));
}

#[test]
#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn optiscaler_apply_preserves_stellar_blade_selected_release() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let tag = "official:v0.9.11";
    let asset = "OptiScaler_0.9.11.7z";

    let cache_dir = modde_games::tools::optiscaler::cached_release_dir(tag);
    std::fs::create_dir_all(&cache_dir).expect("create OptiScaler cache dir");
    std::fs::write(cache_dir.join("OptiScaler.dll"), b"dll").expect("cached dll");
    std::fs::write(cache_dir.join("OptiScaler.ini"), b"[FSR]\n").expect("cached ini");
    let optipatcher = modde_games::tools::optiscaler::cached_optipatcher_asi();
    std::fs::create_dir_all(optipatcher.parent().expect("optipatcher cache parent"))
        .expect("create OptiPatcher cache dir");
    std::fs::write(&optipatcher, b"asi").expect("cached OptiPatcher");

    let mut config = modde_games::tools::ToolConfig::new("optiscaler");
    config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
    config.set("source_mode", serde_json::json!("github_release"));
    config.set("release_tag", serde_json::json!(tag));
    config.set("release_asset", serde_json::json!(asset));
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    config.set("copy_companion_files", serde_json::json!(true));
    config.set("enable_optipatcher", serde_json::json!(false));
    let db = test_db();
    crate::app::block_on(db.save_tool_config(
        &GameId::from("stellar-blade"),
        "optiscaler",
        false,
        &serde_json::to_string(&config.settings).expect("settings json"),
    ))
    .expect("save selected OptiScaler release");

    let context = modde_games::tools::ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(game_dir.path().to_path_buf()),
        None,
    );
    let result = tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(crate::app::tool_ops::apply_tool_for_game(
            db.clone(),
            "stellar-blade".to_string(),
            game_dir.path().to_path_buf(),
            "optiscaler".to_string(),
            Some(context),
        ))
        .expect("apply OptiScaler");

    assert_eq!(result.applied_file_count, 2);
    let row =
        crate::app::block_on(db.load_tool_config(&GameId::from("stellar-blade"), "optiscaler"))
            .expect("load tool config")
            .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["optiscaler_profile"], "community-dxgi");
    assert_eq!(saved["source_mode"], "github_release");
    assert_eq!(saved["release_tag"], tag);
    assert_eq!(saved["release_asset"], asset);
    assert_eq!(saved["proxy_dll"], "dxgi.dll");
    assert_eq!(saved["enable_optipatcher"], false);
}

#[test]
fn unavailable_tool_toggle_leaves_settings_untouched() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("test-game".to_string());
    // This tool ID is absent under every supported feature combination.
    let tool_id = "fixture-unregistered-tool";
    let task = app.update(Message::ToggleTool {
        tool_id: tool_id.into(),
        enabled: true,
    });
    assert_eq!(app.status_message, format!("Unknown tool: {tool_id}"));
    assert!(app.tool_state.active_tool_id.is_none());
    assert!(!app.tool_state.is_tool_busy(tool_id));
    drop(task);
    assert!(modde_core::library::mutation_lock().is_ok());
    assert!(
        crate::app::block_on(app.db.load_tool_config(&GameId::from("test-game"), tool_id))
            .expect("read isolated tool settings")
            .is_none()
    );
}

#[test]
fn test_shortcut_action_to_message_maps_deploy() {
    assert!(matches!(
        shortcut_action_to_message("deploy"),
        Some(Message::Deploy)
    ));
}

#[test]
fn test_shortcut_action_to_message_drops_unmapped() {
    assert!(shortcut_action_to_message("refresh").is_none());
    assert!(shortcut_action_to_message("nonexistent").is_none());
}

#[test]
fn test_shortcut_action_to_message_maps_dismiss_modal() {
    assert!(matches!(
        shortcut_action_to_message("dismiss_modal"),
        Some(Message::CancelNewProfileDialog)
    ));
}

#[test]
fn shortcuts_layer_maps_uncaptured_escape() {
    let app = test_app();
    let mut ui = iced_test::simulator::simulator(app.view());

    ui.tap_key(iced::keyboard::Key::Named(
        iced::keyboard::key::Named::Escape,
    ));

    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::CancelNewProfileDialog)),
        "uncaptured Escape should emit CancelNewProfileDialog, got: {messages:?}",
    );
}

#[test]
fn test_fomod_cancel() {
    let mut app = test_app();
    app.fomod_installer = Some(FOMODWizardState::new());
    app.active_view = View::FOMODWizard(FOMODWizardState::new());
    let _ = app.update(Message::FOMODCancel);
    assert!(matches!(app.active_view, View::ModList));
    assert!(app.status_message.contains("cancelled"));
}

#[test]
fn test_fomod_back() {
    let mut app = test_app();
    app.fomod_wizard_pos = 2;
    let _ = app.update(Message::FOMODBack);
    assert_eq!(app.fomod_wizard_pos, 1);
}

#[test]
fn test_reset_fomod() {
    let mut app = test_app();
    app.fomod_installer = Some(FOMODWizardState::new());
    app.fomod_source_dir = Some(PathBuf::from("/src"));
    app.fomod_wizard_pos = 1;
    app.fomod_can_undo = true;
    app.reset_fomod();
    assert!(app.fomod_installer.is_none());
    assert_eq!(app.fomod_wizard_pos, 0);
    assert!(!app.fomod_can_undo);
}

// ── UI handler lock refusal tests (DB-isolated) ──────────────
//
// These exercise the DB-touching branches of `Message::ReorderMod`
// / `LockMod` / `UnlockMod`.
//
// DB isolation is a process-wide `OnceLock<TempDir>` via
// `modde_core::paths::set_data_dir` — the same pattern used by
// `crates/modde-core/tests/load_order_lock_tests.rs`. All tests in
// this module share one tempdir; collision avoidance is by unique
// profile names (so there's no mutable-shared-state risk). See
// `/home/can/.claude/plans/greedy-shimmying-pine.md` for the full
// plan.
