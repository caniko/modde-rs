//! E2E-style semantic selector tests for high-value modde UI flows.
//!
//! This is the local proving ground for Playwright-like iced testing. Until
//! iced exposes real semantic metadata, these tests use stable widget IDs with
//! the same public shape we want to upstream as `test_id` selectors.

use iced_test::selector;
use iced_test::simulator::simulator;
use modde_ui::app::{Message, SettingsState, ToolReleaseSupport, ToolState, ToolUiEntry};
use modde_ui::semantics;
use std::path::PathBuf;

fn by_test_id(test_id: &str) -> impl iced_test::Selector<Output = iced_test::selector::Target> {
    selector::id(semantics::widget_id(test_id.to_string()))
}

fn settings_state() -> SettingsState {
    SettingsState {
        nexus_api_key_draft: String::new(),
        nexus_api_key_visible: false,
        nexus_api_key_source: None,
        nexus_config_key_exists: false,
        game_install_paths: Vec::new(),
        download_dir: None,
        effective_download_dir: PathBuf::new(),
        has_stock_snapshot: false,
        theme_name: "Dark".to_string(),
        nexus_status: None,
        game_path_draft: String::new(),
        download_dir_draft: String::new(),
        current_game_id: None,
    }
}

#[test]
fn settings_semantic_ids_drive_api_key_flow() {
    let mut ui = simulator(modde_ui::views::settings::view(settings_state()));

    ui.click(by_test_id("settings.nexus_api_key"))
        .expect("API key input should be selectable by test id");
    ui.typewrite("abc123");
    ui.click(by_test_id("settings.nexus_api_key.toggle_visibility"))
        .expect("visibility toggle should be selectable by test id");
    ui.click(by_test_id("settings.nexus_api_key.validate"))
        .expect("validate button should be selectable by test id");

    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::SetNexusApiKeyDraft(value) if value == "abc123")),
        "typing should emit SetNexusApiKeyDraft(\"abc123\"), got: {messages:?}",
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ToggleNexusApiKeyVisibility)),
        "toggle should emit ToggleNexusApiKeyVisibility, got: {messages:?}",
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ValidateNexusKey)),
        "validate should emit ValidateNexusKey, got: {messages:?}",
    );
}

#[test]
fn settings_duplicate_browse_labels_are_addressable_by_test_id() {
    let mut game_path_ui = simulator(modde_ui::views::settings::view(settings_state()));
    game_path_ui
        .click(by_test_id("settings.game_path.browse"))
        .expect("game path Browse should be selectable by test id");
    let messages: Vec<_> = game_path_ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::BrowseGamePath)),
        "game path browse should emit BrowseGamePath, got: {messages:?}",
    );

    let mut download_dir_ui = simulator(modde_ui::views::settings::view(settings_state()));
    download_dir_ui
        .click(by_test_id("settings.download_dir.browse"))
        .expect("download dir Browse should be selectable by test id");
    let messages: Vec<_> = download_dir_ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::BrowseDownloadDir)),
        "download dir browse should emit BrowseDownloadDir, got: {messages:?}",
    );
}

#[test]
fn tools_semantic_ids_drive_optiscaler_settings_and_apply() {
    let state = optiscaler_tools_state();
    let mut ui = simulator(modde_ui::views::tools::view(&state));

    ui.click(by_test_id(
        "tools.optiscaler.setting.ini_overrides__FSR__Fsr4Update.on",
    ))
    .expect("tri-state setting should be selectable by test id");
    ui.click(by_test_id("tools.optiscaler.apply"))
        .expect("apply button should be selectable by test id");

    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::UpdateToolSetting { tool_id, key, value }
                if tool_id == "optiscaler"
                    && key == "ini_overrides.FSR.Fsr4Update"
                    && value == &serde_json::json!(true)
        )),
        "tri-state click should emit typed OptiScaler setting update, got: {messages:?}",
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "optiscaler")),
        "apply should emit ApplyTool(optiscaler), got: {messages:?}",
    );
}

fn optiscaler_tools_state() -> ToolState {
    ToolState {
        active_tool_id: Some("optiscaler".to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        loading: false,
        load_error: None,
        load_generation: 0,
        show_advanced_settings: true,
        active_operations: std::collections::HashSet::new(),
        tool_option_catalog: std::collections::HashMap::from([
            (
                "optiscaler.release_tag".to_string(),
                vec!["v0.9.1".to_string()],
            ),
            (
                "optiscaler.release_asset".to_string(),
                vec!["Optiscaler_0.9.1-final.7z".to_string()],
            ),
        ]),
        optiscaler_releases: Vec::new(),
        optiscaler_releases_loading: false,
        proton_versions_loading: false,
        executables: Vec::new(),
        executables_loading: false,
        executables_load_error: None,
        executables_load_generation: 0,
        executable_draft: Default::default(),
        executable_editor_open: false,
        executable_error: None,
        active_executable_operations: std::collections::HashSet::new(),
        entries: vec![ToolUiEntry {
            tool_id: "optiscaler".to_string(),
            display_name: "OptiScaler".to_string(),
            description: "Upscaling injection".to_string(),
            category: "Graphics".to_string(),
            available: true,
            availability_text: "available".to_string(),
            enabled: true,
            settings: serde_json::json!({
                "release_tag": "v0.9.1",
                "release_asset": "Optiscaler_0.9.1-final.7z",
                "ini_overrides": {
                    "FSR": {
                        "Fsr4Update": "auto"
                    }
                }
            }),
            setting_specs: vec![modde_games::tools::ToolSettingSpec::tri_state_bool(
                "ini_overrides.FSR.Fsr4Update",
                "FSR4 update",
                "FSR4 update.",
            )],
            generated_config_path: None,
            applied_files: vec!["dxgi.dll".to_string()],
            has_file_patching: true,
            release_support: ToolReleaseSupport::from_supports_releases(true),
            status_message: None,
            env_preview: Vec::new(),
            dll_overrides: Vec::new(),
            wrapper_preview: Vec::new(),
            derived_facts: Vec::new(),
            optiscaler_state: None,
            optiscaler_latest_backup: None,
            optiscaler_detected_files: 0,
            apply_pending: true,
            apply_missing_inputs: Vec::new(),
            setting_history: Vec::new(),
            config_checklist: Vec::new(),
            dirty_keys: std::collections::HashSet::new(),
        }],
    }
}
