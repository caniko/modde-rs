#![allow(clippy::wildcard_imports)]
use super::*;

#[test]
fn executable_environment_parser_accepts_key_value_lines() {
    let parsed = parse_executable_environment("WINESYNC=1\nDXVK_LOG_LEVEL=none").unwrap();
    assert_eq!(parsed.get("WINESYNC").map(String::as_str), Some("1"));
    assert_eq!(
        parsed.get("DXVK_LOG_LEVEL").map(String::as_str),
        Some("none")
    );
}

#[test]
fn executable_environment_parser_rejects_invalid_line() {
    let err = parse_executable_environment("WINESYNC").unwrap_err();
    assert!(err.contains("KEY=VALUE"));
}

#[test]
fn test_initial_state() {
    let app = test_app();
    assert!(matches!(app.active_view, View::ModList));
    assert!(app.active_profile.is_none());
    assert_eq!(app.status_message, "Ready");
    assert_eq!(app.theme_name, "Dark");
    assert!(app.fomod_installer.is_none());
    assert_eq!(app.experiment_depth, 0);
    assert!(matches!(
        app.diagnostics_state,
        crate::views::diagnostics::DiagnosticsState::Idle
    ));
}

#[test]
fn render_path_sources_do_not_call_block_on() {
    let sources = [
        (
            "app.rs",
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/app.rs")),
        ),
        (
            "app/view.rs",
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/view.rs")),
        ),
        (
            "app/model.rs",
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/model.rs")),
        ),
        (
            "app/update.rs",
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/update.rs")),
        ),
        (
            "app/profile_ops.rs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/app/profile_ops.rs"
            )),
        ),
        (
            "app/tool_ops.rs",
            include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/app/tool_ops.rs")),
        ),
        (
            "app/tool_settings.rs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/src/app/tool_settings.rs"
            )),
        ),
    ];
    let allowed_blocking_helpers = [
        ("app.rs", "load_hidden_files_blocking"),
        ("app.rs", "load_active_plugins_blocking"),
        ("app/model.rs", "reload_profile_blocking"),
        ("app/model.rs", "finish_pending_switch_blocking"),
        ("app/model.rs", "load_diagnostics_blocking"),
        ("app/update.rs", "deploy_profile_blocking"),
        ("app/tool_ops.rs", "load_tools_state_blocking"),
        ("app/tool_ops.rs", "build_tool_ui_entry_blocking"),
        ("app/tool_ops.rs", "apply_tool_for_game_blocking"),
        ("app/tool_ops.rs", "revert_tool_for_game_blocking"),
        ("app/tool_ops.rs", "deactivate_optiscaler_for_game_blocking"),
        ("app/tool_ops.rs", "run_saved_executable_for_game_blocking"),
        ("app/tool_ops.rs", "run_executable_row_blocking"),
        ("app/tool_settings.rs", "current_tool_config_blocking"),
        (
            "app/tool_settings.rs",
            "save_tool_setting_for_game_blocking",
        ),
        ("app/tool_settings.rs", "toggle_tool_for_game_blocking"),
        (
            "app/tool_settings.rs",
            "restore_tool_settings_for_game_blocking",
        ),
        ("app/tool_settings.rs", "adopt_optiscaler_for_game_blocking"),
        (
            "app/tool_settings.rs",
            "load_tool_config_or_default_blocking",
        ),
        (
            "app/tool_settings.rs",
            "save_tool_config_with_reason_blocking",
        ),
        ("app/tool_settings.rs", "generate_tool_configs_blocking"),
    ];

    for (path, source) in sources {
        let mut current_function: Option<&str> = None;
        for (index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            for prefix in [
                "pub(super) async fn ",
                "pub(crate) async fn ",
                "pub async fn ",
                "async fn ",
                "pub(super) fn ",
                "pub(crate) fn ",
                "pub fn ",
                "fn ",
            ] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    current_function = rest.split_once('(').map(|(name, _)| name);
                    break;
                }
            }
            if !line.contains("crate::app::block_on") {
                continue;
            }
            if current_function.is_some_and(|function| {
                allowed_blocking_helpers
                    .iter()
                    .any(|(allowed_path, allowed_function)| {
                        *allowed_path == path && *allowed_function == function
                    })
            }) {
                continue;
            }
            if path == "app/update.rs"
                && (line.contains("modde_core::db::ModdeDb::open()")
                    || line.contains("ProfileManager::with_db(db.clone()).list()"))
            {
                continue;
            }
            panic!(
                "{path}:{} calls crate::app::block_on on a render-path source line: {line}",
                index + 1
            );
        }
    }
}

#[test]
fn test_title() {
    let app = test_app();
    assert_eq!(app.title(), "modde");
}

#[test]
fn test_switch_view_settings() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchView(View::Settings));
    assert!(matches!(app.active_view, View::Settings));
}

#[test]
fn test_switch_view_saves() {
    let mut app = test_app();
    let _ = app.update(Message::SwitchView(View::Saves));
    assert!(matches!(app.active_view, View::Saves));
}

#[test]
fn switch_view_tools_starts_async_load() {
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.entries = vec![test_tool_ui_entry("stale")];

    let _ = app.update(Message::SwitchView(View::Tools));

    assert!(matches!(app.active_view, View::Tools));
    assert!(app.tool_state.loading);
    assert_eq!(app.tool_state.load_generation, 1);
    assert_eq!(app.tool_state.entries[0].tool_id, "stale");
    assert_eq!(app.status_message, "Loading tools...");
}

#[test]
fn refresh_tools_starts_async_load_without_clearing_entries() {
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.entries = vec![test_tool_ui_entry("stale")];

    let _ = app.update(Message::RefreshTools);

    assert!(app.tool_state.loading);
    assert_eq!(app.tool_state.load_generation, 1);
    assert_eq!(app.tool_state.entries[0].tool_id, "stale");
    assert_eq!(app.status_message, "Loading tools...");
}

#[test]
fn tools_loaded_success_replaces_entries_and_clears_loading() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 7;
    app.tool_state.entries = vec![test_tool_ui_entry("old")];

    let _ = app.update(Message::ToolsLoaded {
        generation: 7,
        result: Ok(test_tool_load_snapshot(vec![test_tool_ui_entry("reshade")])),
    });

    assert!(!app.tool_state.loading);
    assert!(app.tool_state.load_error.is_none());
    assert_eq!(app.tool_state.entries.len(), 1);
    assert_eq!(app.tool_state.entries[0].tool_id, "reshade");
    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("reshade"));
    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("proton.selected_version"),
        Some(&vec!["latest".to_string()])
    );
    assert_eq!(app.status_message, "Loaded 1 tool(s)");
}

#[test]
fn tools_loaded_failure_preserves_entries_and_records_error() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 3;
    app.tool_state.entries = vec![test_tool_ui_entry("old")];

    let _ = app.update(Message::ToolsLoaded {
        generation: 3,
        result: Err("scan failed".to_string()),
    });

    assert!(!app.tool_state.loading);
    assert_eq!(app.tool_state.entries[0].tool_id, "old");
    assert_eq!(app.tool_state.load_error.as_deref(), Some("scan failed"));
    assert_eq!(app.status_message, "Failed to load tools: scan failed");
}

#[test]
fn stale_tools_loaded_result_is_ignored() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 2;
    app.tool_state.entries = vec![test_tool_ui_entry("old")];

    let _ = app.update(Message::ToolsLoaded {
        generation: 1,
        result: Ok(test_tool_load_snapshot(vec![test_tool_ui_entry("reshade")])),
    });

    assert!(app.tool_state.loading);
    assert_eq!(app.tool_state.entries[0].tool_id, "old");
    assert!(app.tool_state.load_error.is_none());
    assert_eq!(app.status_message, "Ready");
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn tool_toggle_write_persists_and_reload_reflects_committed_value() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let task = app.update(Message::ToggleTool {
        tool_id: "mangohud".to_string(),
        enabled: true,
    });

    // The write is followed by a task that releases its mutation lease.
    assert_eq!(task.units(), 2);
    assert!(modde_core::library::mutation_lock().is_err());
    assert_eq!(app.tool_state.active_tool_id.as_deref(), Some("mangohud"));
    assert_eq!(app.status_message, "Enabling MangoHud...");
    drop(task);
    assert!(modde_core::library::mutation_lock().is_ok());

    let result = crate::app::block_on(crate::app::tool_settings::toggle_tool_for_game(
        app.db.clone(),
        "skyrim-se".to_string(),
        "mangohud".to_string(),
        true,
        app.current_tool_game_context(),
    ));
    let reload_task = app.update(Message::ToolSettingWritten {
        tool_id: "mangohud".to_string(),
        result,
    });

    assert_eq!(reload_task.units(), 1);
    assert_eq!(app.tool_state.load_generation, 1);
    let db = test_db();
    let row = crate::app::block_on(db.load_tool_config(&GameId::from("skyrim-se"), "mangohud"))
        .expect("load tool config")
        .expect("tool config exists");
    assert!(row.enabled);

    let request = app.tool_load_request().expect("tool load request");
    let snapshot = crate::app::block_on(crate::app::tool_ops::load_tools_state(
        app.db.clone(),
        request,
    ))
    .expect("load tools");
    let generation = app.tool_state.load_generation;
    let _ = app.update(Message::ToolsLoaded {
        generation,
        result: Ok(snapshot),
    });
    let mangohud = app
        .tool_state
        .entries
        .iter()
        .find(|entry| entry.tool_id == "mangohud")
        .expect("mangohud entry");
    assert!(mangohud.enabled);
}

#[test]
fn stale_tool_toggle_reload_is_ignored() {
    let mut app = test_app();
    let mut committed = test_tool_ui_entry("mangohud");
    committed.enabled = true;
    app.tool_state.entries = vec![committed];
    app.tool_state.loading = true;
    app.tool_state.load_generation = 2;

    let mut stale = test_tool_ui_entry("mangohud");
    stale.enabled = false;
    let _ = app.update(Message::ToolsLoaded {
        generation: 1,
        result: Ok(test_tool_load_snapshot(vec![stale])),
    });

    assert!(app.tool_state.loading);
    assert!(app.tool_state.entries[0].enabled);
}

#[test]
fn tools_loaded_success_preserves_pending_optiscaler_feedback() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 7;
    app.pending_tools_load_status_message = Some("Reset OptiScaler config overrides".to_string());

    let _ = app.update(Message::ToolsLoaded {
        generation: 7,
        result: Ok(test_tool_load_snapshot(vec![test_tool_ui_entry(
            "optiscaler",
        )])),
    });

    assert!(!app.tool_state.loading);
    assert_eq!(app.status_message, "Reset OptiScaler config overrides");
    assert!(app.pending_tools_load_status_message.is_none());
}

#[test]
fn tools_loaded_failure_clears_pending_optiscaler_feedback() {
    let mut app = test_app();
    app.tool_state.loading = true;
    app.tool_state.load_generation = 7;
    app.pending_tools_load_status_message = Some("Adopted OptiScaler (3 file(s))".to_string());

    let _ = app.update(Message::ToolsLoaded {
        generation: 7,
        result: Err("scan failed".to_string()),
    });

    assert_eq!(app.status_message, "Failed to load tools: scan failed");
    assert!(app.pending_tools_load_status_message.is_none());
}

#[test]
fn button_hover_start_creates_pending_state() {
    let mut app = test_app();

    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    assert_eq!(
        app.button_hover_toast.pending,
        Some(ButtonHoverToast {
            id: 11,
            description: "Create a profile.",
        })
    );
    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn matching_button_hover_elapsed_makes_toast_visible() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    let _ = app.update(Message::ButtonHoverElapsed { id: 11 });

    assert_eq!(
        app.button_hover_toast.visible,
        app.button_hover_toast.pending
    );
}

#[test]
fn stale_button_hover_elapsed_is_ignored() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    let _ = app.update(Message::ButtonHoverElapsed { id: 10 });

    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn button_hover_exit_before_delay_suppresses_toast() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });

    let _ = app.update(Message::ButtonHoverEnded { id: 11 });
    let _ = app.update(Message::ButtonHoverElapsed { id: 11 });

    assert!(app.button_hover_toast.pending.is_none());
    assert!(app.button_hover_toast.visible.is_none());
}

#[test]
fn button_hover_exit_after_display_clears_toast() {
    let mut app = test_app();
    let _ = app.update(Message::ButtonHoverStarted {
        id: 11,
        description: "Create a profile.",
    });
    let _ = app.update(Message::ButtonHoverElapsed { id: 11 });

    let _ = app.update(Message::ButtonHoverEnded { id: 11 });

    assert!(app.button_hover_toast.pending.is_none());
    assert!(app.button_hover_toast.visible.is_none());
}
