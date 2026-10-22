//! Simulator-based UI tests for modde views.
//!
//! These tests use `iced_test::simulator` to render views headlessly and
//! interact with them (find widgets by text, click buttons, type into inputs).
//! They complement the pure state/logic tests in `app.rs` by verifying that
//! the rendered widget tree actually contains expected elements and that
//! click/input interactions produce the right `Message` variants.

use iced_test::core::Point;
use iced_test::selector;
use iced_test::simulator::{click, simulator};
use modde_ui::app::{
    ExecutableUiEntry, Message, SettingsState, ToolReleaseSupport, ToolState, ToolUiEntry,
    WabbajackInstallerState,
};
use modde_ui::semantics;
use modde_ui::views::data_tab::DataTabState;
use modde_ui::views::diagnostics::{
    DiagnosticEntry, DiagnosticSeverity, DiagnosticsReport, DiagnosticsState, IntegritySummary,
};
use modde_ui::views::downloads::{DownloadState, DownloadTask};
use std::path::PathBuf;

fn by_test_id(test_id: &str) -> impl iced_test::Selector<Output = iced_test::selector::Target> {
    selector::id(semantics::widget_id(test_id.to_string()))
}

// ─── Settings View ────────────────────────────────────────────

fn default_settings_state() -> SettingsState {
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
fn settings_shows_sections() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.find("Settings").expect("title");
    ui.find("Nexus Mods API Key").expect("API key section");
    ui.find("Game Install Path").expect("game path section");
    ui.find("Download Directory").expect("download dir section");
    ui.find("Stock Game Snapshot")
        .expect("stock snapshot section");
    ui.find("Theme").expect("theme section");
}

#[test]
fn settings_hides_nexus_key_by_default_and_can_toggle() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.find("Show").expect("should show reveal button");
    ui.click("Show").expect("should click 'Show'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ToggleNexusApiKeyVisibility)),
        "should emit ToggleNexusApiKeyVisibility, got: {messages:?}",
    );
}

#[test]
fn settings_click_replace_and_remove_emit_messages() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Replace").expect("should click 'Replace'");
    ui.click("Remove modde config")
        .expect("should click 'Remove modde config'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ReplaceNexusApiKey)),
        "should emit ReplaceNexusApiKey, got: {messages:?}",
    );
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::RequestConfirm(confirm)
                if matches!(confirm.on_confirm, Message::RemoveNexusConfigKey)
        )),
        "should ask to confirm RemoveNexusConfigKey, got: {messages:?}",
    );
}

#[test]
fn settings_shows_no_snapshot_status() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.find("No snapshot created")
        .expect("should show 'No snapshot created'");
}

#[test]
fn settings_shows_snapshot_exists() {
    let state = SettingsState {
        has_stock_snapshot: true,
        ..default_settings_state()
    };
    let mut ui = simulator(modde_ui::views::settings::view(state));
    ui.find("Snapshot exists")
        .expect("should show 'Snapshot exists'");
}

#[test]
fn settings_click_validate_emits_message() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Validate").expect("should click 'Validate'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ValidateNexusKey)),
        "should emit ValidateNexusKey, got: {messages:?}",
    );
}

#[test]
fn settings_click_create_snapshot_emits_message() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Create Snapshot")
        .expect("should click 'Create Snapshot'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::CreateStockSnapshot)),
        "should emit CreateStockSnapshot, got: {messages:?}",
    );
}

#[test]
fn settings_click_verify_snapshot_emits_message() {
    let mut ui = simulator(modde_ui::views::settings::view(default_settings_state()));
    ui.click("Verify Snapshot")
        .expect("should click 'Verify Snapshot'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::VerifyStockSnapshot)),
        "should emit VerifyStockSnapshot, got: {messages:?}",
    );
}

#[test]
fn wabbajack_shows_generated_hm_snippet_preview() {
    let state = WabbajackInstallerState {
        hm_snippet: "programs.modde.profiles.lotf = {\n  game = \"skyrim-se\";\n};\n".to_string(),
        ..Default::default()
    };
    let manifest = None;
    let available_games = vec![("skyrim-se".to_string(), "Skyrim SE".to_string())];
    let mut ui = simulator(modde_ui::views::wabbajack::view(
        &state,
        &manifest,
        &available_games,
        Some("skyrim-se"),
    ));

    ui.find("programs.modde.profiles.lotf = {\n  game = \"skyrim-se\";\n};\n")
        .expect("should show generated snippet preview");
}

fn wabbajack_readiness_report(
    install_ready: bool,
    manual_downloads: Vec<modde_sources::wabbajack::readiness::WabbajackManualArchive>,
) -> modde_sources::wabbajack::readiness::WabbajackReadinessReport {
    modde_sources::wabbajack::readiness::WabbajackReadinessReport {
        manifest_path: "/tmp/list.wabbajack".to_string(),
        name: "Test List".to_string(),
        author: "tester".to_string(),
        version: "1.0".to_string(),
        game: "SkyrimSpecialEdition".to_string(),
        normalized_game: "skyrim-se".to_string(),
        profile_name: "test-list".to_string(),
        store_path: "/tmp/store".to_string(),
        archives: 1,
        directives: 2,
        archive_states: Default::default(),
        directive_types: Default::default(),
        archive_extensions: Default::default(),
        downloadable_archives: 1,
        store_present: 0,
        store_missing: Vec::new(),
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
        manual_downloads,
        install_ready,
    }
}

#[test]
fn wabbajack_installer_renders_guided_sections() {
    let state = WabbajackInstallerState::default();
    let manifest = None;
    let available_games = vec![("skyrim-se".to_string(), "Skyrim SE".to_string())];
    let mut ui = simulator(modde_ui::views::wabbajack::view(
        &state,
        &manifest,
        &available_games,
        Some("skyrim-se"),
    ));

    ui.find("Wabbajack Installer").expect("title");
    ui.find("Install Target").expect("target section");
    ui.find("Readiness").expect("readiness section");
    ui.find("Missing Manual Archives")
        .expect("manual archive section");
    ui.find("Install Progress").expect("progress section");
}

#[test]
fn wabbajack_ready_install_button_emits_start() {
    let state = WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        readiness: Some(wabbajack_readiness_report(true, Vec::new())),
        ..Default::default()
    };
    let manifest = None;
    let available_games = vec![("skyrim-se".to_string(), "Skyrim SE".to_string())];
    let mut ui = simulator(modde_ui::views::wabbajack::view(
        &state,
        &manifest,
        &available_games,
        Some("skyrim-se"),
    ));

    ui.find("Ready for validated staging/deploy")
        .expect("ready status");
    ui.click("Install").expect("install button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::WabbajackStartInstall)),
        "ready install should emit WabbajackStartInstall, got: {messages:?}",
    );
}

#[test]
fn wabbajack_missing_manual_archives_block_install_and_offer_actions() {
    let manual = modde_sources::wabbajack::readiness::WabbajackManualArchive {
        hash: "0000000000001234".to_string(),
        name: "manual.7z".to_string(),
        url: "https://example.test/manual.7z".to_string(),
        prompt: "download it".to_string(),
        store_path: "/tmp/store/0000000000001234.archive".to_string(),
    };
    let state = WabbajackInstallerState {
        file_path: Some(PathBuf::from("/tmp/list.wabbajack")),
        readiness: Some(wabbajack_readiness_report(false, vec![manual])),
        ..Default::default()
    };
    let manifest = None;
    let available_games = vec![("skyrim-se".to_string(), "Skyrim SE".to_string())];
    let mut ui = simulator(modde_ui::views::wabbajack::view(
        &state,
        &manifest,
        &available_games,
        Some("skyrim-se"),
    ));

    ui.find("manual.7z").expect("manual archive row");
    ui.click("Open URL").expect("open URL button");
    ui.click("Import archives").expect("import button");
    ui.click("Install")
        .expect("disabled install button remains visible");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| {
            matches!(m, Message::WabbajackOpenUrl(url) if url == "https://example.test/manual.7z")
        }),
        "manual archive should expose Open URL, got: {messages:?}",
    );
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::WabbajackImportArchives)),
        "manual archive should expose import action, got: {messages:?}",
    );
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::WabbajackStartInstall)),
        "install should not emit while manual archives are missing, got: {messages:?}",
    );
}

#[test]
fn wabbajack_live_progress_is_visible() {
    let state = WabbajackInstallerState {
        install_phase: "Applying".to_string(),
        install_current_item: "directive 4/10".to_string(),
        progress: 0.4,
        log_lines: vec!["Applying directives: 4/10".to_string()],
        ..Default::default()
    };
    let manifest = None;
    let available_games = vec![("skyrim-se".to_string(), "Skyrim SE".to_string())];
    let mut ui = simulator(modde_ui::views::wabbajack::view(
        &state,
        &manifest,
        &available_games,
        Some("skyrim-se"),
    ));

    ui.find("Applying: directive 4/10").expect("progress phase");
    ui.find("Applying directives: 4/10").expect("progress log");
}

// ─── Mod List View ────────────────────────────────────────────

/// Helper macro that declares filter state bindings at the caller's scope
/// and creates a `mut $ui` simulator. Use as:
///   `mod_list_simulator!(mods_expr => ui);`
macro_rules! mod_list_simulator {
    ($mods:expr => $ui:ident) => {
        let __collapsed = std::collections::HashSet::new();
        let __categories: Vec<(Option<i64>, String)> = vec![];
        let __criteria: Vec<modde_core::filter::FilterCriterion> = vec![];
        let __filter_keys: Vec<String> = vec![];
        let mut $ui = simulator(modde_ui::views::mod_list::view_filtered(
            $mods,
            &__filter_keys,
            "",
            None,
            modde_core::filter::FilterMode::default(),
            &__criteria,
            &__collapsed,
            &__categories,
            false,
            false,
        ));
    };
}

fn sample_mods() -> Vec<modde_core::profile::EnabledMod> {
    vec![
        modde_core::profile::EnabledMod {
            mod_id: "SkyUI".to_string(),
            enabled: true,
            version: Some("5.2".to_string()),
            fomod_config: None,
            ..Default::default()
        },
        modde_core::profile::EnabledMod {
            mod_id: "USSEP".to_string(),
            enabled: true,
            version: Some("4.2.8".to_string()),
            fomod_config: None,
            ..Default::default()
        },
        modde_core::profile::EnabledMod {
            mod_id: "EnhancedLights".to_string(),
            enabled: false,
            version: None,
            fomod_config: None,
            ..Default::default()
        },
    ]
}

#[test]
fn mod_list_empty_shows_placeholder() {
    let mods: Vec<modde_core::profile::EnabledMod> = vec![];
    mod_list_simulator!(&mods => ui);
    ui.find("No mods found. Click 'Add Mod' to get started.")
        .expect("should show empty placeholder");
}

#[test]
fn mod_list_shows_toolbar_buttons() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.find("Add Mod").expect("should show 'Add Mod' button");
    ui.find("Remove").expect("should show 'Remove' button");
    ui.find("Deploy").expect("should show 'Deploy' button");
}

#[test]
fn mod_list_shows_mod_names() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.find("SkyUI").expect("should show SkyUI");
    ui.find("USSEP").expect("should show USSEP");
    ui.find("EnhancedLights")
        .expect("should show EnhancedLights");
}

#[test]
fn mod_list_shows_mod_count() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.find("3 mod(s) shown").expect("should show mod count");
}

#[test]
fn mod_list_click_add_mod_emits_message() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.click("Add Mod").expect("should click 'Add Mod'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::AddMod)),
        "should emit AddMod, got: {messages:?}",
    );
}

#[test]
fn mod_list_click_deploy_emits_message() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.click("Deploy").expect("should click 'Deploy'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::Deploy)),
        "should emit Deploy, got: {messages:?}",
    );
}

#[test]
fn mod_list_click_mod_name_emits_select() {
    let mods = sample_mods();
    mod_list_simulator!(&mods => ui);
    ui.click("USSEP").expect("should click 'USSEP'");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(m, Message::SelectMod(1))),
        "should emit SelectMod(1), got: {messages:?}",
    );
}

// ─── Sidebar View ─────────────────────────────────────────────

macro_rules! sidebar_test {
    ($name:ident, profiles = $profiles:expr, active = $active:expr, depth = $depth:expr, |$ui:ident| $body:expr) => {
        #[test]
        fn $name() {
            let view = modde_ui::app::View::ModList;
            let collapsed_groups = std::collections::HashSet::new();
            let profiles = $profiles;
            let active = $active;
            let mut $ui = simulator(modde_ui::views::sidebar::view(
                &view,
                &collapsed_groups,
                &profiles,
                &active,
                $depth,
                true,
                None,
                None,
            ));
            $body
        }
    };
}

sidebar_test!(
    sidebar_shows_nav_items,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.find("Game").expect("group: Game");
        ui.find("Install").expect("group: Install");
        ui.find("General").expect("group: General");
        ui.find("Mod List").expect("nav: Mod List");
        ui.find("Saves").expect("nav: Saves");
        ui.find("Browse Nexus").expect("nav: Browse Nexus");
        ui.find("Collections").expect("nav: Collections");
        ui.find("Wabbajack").expect("nav: Wabbajack");
        ui.find("Downloads").expect("nav: Downloads");
        ui.find("Data Files").expect("nav: Data Files");
        ui.find("Diagnostics").expect("nav: Diagnostics");
        ui.find("Tools").expect("nav: Tools");
        ui.find("Executables").expect("nav: Executables");
        ui.find("Settings").expect("nav: Settings");
        assert!(ui.find("Maintenance").is_err(), "Maintenance group is gone");
        assert!(
            ui.find("Verify").is_err(),
            "Verify nav item is unified into Diagnostics"
        );
    }
);

#[test]
fn sidebar_default_collapsed_groups_hide_inactive_items() {
    let view = modde_ui::app::View::ModList;
    let collapsed_groups = std::collections::HashSet::from([modde_ui::app::SidebarGroup::General]);
    let profiles = Vec::new();
    let active = None;
    let mut ui = simulator(modde_ui::views::sidebar::view(
        &view,
        &collapsed_groups,
        &profiles,
        &active,
        0,
        true,
        None,
        None,
    ));

    ui.find("Mod List")
        .expect("active game item remains visible");
    ui.find("Diagnostics")
        .expect("Diagnostics remains visible in the Game group");
    assert!(
        ui.find("Settings").is_err(),
        "collapsed inactive General item should be hidden"
    );
}

#[test]
fn sidebar_collapsed_active_group_still_shows_active_view() {
    let view = modde_ui::app::View::Settings;
    let collapsed_groups = std::collections::HashSet::from([modde_ui::app::SidebarGroup::General]);
    let profiles = Vec::new();
    let active = None;
    let mut ui = simulator(modde_ui::views::sidebar::view(
        &view,
        &collapsed_groups,
        &profiles,
        &active,
        0,
        true,
        None,
        None,
    ));

    ui.find("Settings")
        .expect("active collapsed item remains visible");
}

#[test]
fn sidebar_group_toggle_emits_message() {
    let view = modde_ui::app::View::ModList;
    let collapsed_groups = std::collections::HashSet::new();
    let profiles = Vec::new();
    let active = None;
    let mut ui = simulator(modde_ui::views::sidebar::view(
        &view,
        &collapsed_groups,
        &profiles,
        &active,
        0,
        true,
        None,
        None,
    ));

    ui.click("Game").expect("should click Game group header");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::ToggleSidebarGroup(modde_ui::app::SidebarGroup::Game)
        )),
        "should emit ToggleSidebarGroup(Game), got: {messages:?}",
    );
}

#[test]
fn sidebar_hides_inactive_saves_when_save_profiles_are_unsupported() {
    let view = modde_ui::app::View::ModList;
    let collapsed_groups = std::collections::HashSet::new();
    let profiles = Vec::new();
    let active = None;
    let mut ui = simulator(modde_ui::views::sidebar::view(
        &view,
        &collapsed_groups,
        &profiles,
        &active,
        0,
        false,
        None,
        None,
    ));

    assert!(
        ui.find("Saves").is_err(),
        "inactive Saves item should be hidden for games without save profiles"
    );
}

#[test]
fn browse_nexus_tabs_keep_existing_messages() {
    let state = modde_ui::views::browse_nexus::NexusBrowseState {
        selected_game_id: Some("skyrim-se".to_string()),
        ..Default::default()
    };
    let games = vec![
        ("skyrim-se".to_string(), "Skyrim SE".to_string()),
        ("fallout4".to_string(), "Fallout 4".to_string()),
    ];
    let mut ui = simulator(modde_ui::views::browse_nexus::view(
        &state,
        &games,
        Some("skyrimspecialedition".to_string()),
    ));

    ui.click("Month").expect("should click Month tab");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::BrowseTabSwitched(modde_ui::views::browse_nexus::BrowseTab::Month)
        )),
        "should emit BrowseTabSwitched(Month), got: {messages:?}",
    );
}

#[test]
fn browse_nexus_with_supported_game_renders_browse_surface() {
    let state = modde_ui::views::browse_nexus::NexusBrowseState {
        selected_game_id: Some("fallout4".to_string()),
        ..Default::default()
    };
    let games = vec![
        ("skyrim-se".to_string(), "Skyrim SE".to_string()),
        ("fallout4".to_string(), "Fallout 4".to_string()),
        ("stellar-blade".to_string(), "Stellar Blade".to_string()),
    ];
    let mut ui = simulator(modde_ui::views::browse_nexus::view(
        &state,
        &games,
        Some("fallout4".to_string()),
    ));

    ui.find("Browse Nexus").expect("should show title");
    ui.find("Search Nexus mods & collections…")
        .expect("should show search input");
}

#[test]
fn wabbajack_tabs_keep_existing_messages() {
    let state = WabbajackInstallerState::default();
    let manifest = None;
    let available_games = Vec::new();
    let mut ui = simulator(modde_ui::views::wabbajack::view(
        &state,
        &manifest,
        &available_games,
        Some("skyrim-se"),
    ));

    ui.click("Manual").expect("should click Manual tab");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::WabbajackTabChanged(modde_ui::app::WabbajackTab::Manual)
        )),
        "should emit WabbajackTabChanged(Manual), got: {messages:?}",
    );
}

sidebar_test!(
    sidebar_click_executables_emits_switch_view,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.click("Executables").expect("should click 'Executables'");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::SwitchView(modde_ui::app::View::Executables))),
            "should emit SwitchView(Executables), got: {messages:?}",
        );
    }
);

sidebar_test!(
    sidebar_click_settings_emits_switch_view,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.click("Settings").expect("should click 'Settings'");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::SwitchView(modde_ui::app::View::Settings))),
            "should emit SwitchView(Settings), got: {messages:?}",
        );
    }
);

sidebar_test!(
    sidebar_click_downloads_emits_switch_view,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.click("Downloads").expect("should click 'Downloads'");
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::SwitchView(modde_ui::app::View::Downloads))),
            "should emit SwitchView(Downloads), got: {messages:?}",
        );
    }
);

sidebar_test!(
    sidebar_shows_new_profile_button,
    profiles = vec![],
    active = None,
    depth = 0,
    |ui| {
        ui.find("New").expect("should show 'New' button");
    }
);

fn test_profile_summary() -> modde_core::profile::ProfileSummary {
    modde_core::profile::ProfileSummary {
        id: 1,
        name: "test".to_string(),
        game_id: "skyrim-se".into(),
        mod_count: 0,
        source_type: "Manual".to_string(),
    }
}

sidebar_test!(
    sidebar_shows_try_profile_when_profile_active,
    profiles = vec![test_profile_summary()],
    active = Some("test".to_string()),
    depth = 0,
    |ui| {
        ui.find("Try Profile")
            .expect("should show 'Try Profile' when a profile is active");
    }
);

sidebar_test!(
    sidebar_experiment_mode_shows_rollback_commit,
    profiles = vec![test_profile_summary()],
    active = Some("test".to_string()),
    depth = 2,
    |ui| {
        ui.find("Rollback").expect("should show 'Rollback'");
        ui.find("Commit").expect("should show 'Commit'");
    }
);

// ─── Downloads / Data / Diagnostics / Tools Views ──────────────────

#[test]
fn downloads_empty_shows_connected_empty_state() {
    let mut ui = simulator(modde_ui::views::downloads::view(&[]));
    ui.find("Downloads").expect("should show title");
    ui.find("No downloads")
        .expect("should show empty downloads state");
}

#[test]
fn downloads_active_task_emits_pause_and_cancel_messages() {
    let tasks = vec![DownloadTask {
        id: 7,
        name: "SkyUI".to_string(),
        state: DownloadState::Active {
            bytes_downloaded: 512 * 1024,
            total_bytes: Some(1024 * 1024),
        },
    }];

    let mut pause_ui = simulator(modde_ui::views::downloads::view(&tasks));
    pause_ui.find("SkyUI").expect("should show download name");
    pause_ui
        .find("50% (512.0 KB downloaded)")
        .expect("should show progress");
    pause_ui.click("Pause").expect("should click Pause");
    let pause_messages: Vec<_> = pause_ui.into_messages().collect();
    assert!(
        pause_messages
            .iter()
            .any(|m| matches!(m, Message::PauseDownload(7))),
        "should emit PauseDownload(7), got: {pause_messages:?}",
    );

    let mut cancel_ui = simulator(modde_ui::views::downloads::view(&tasks));
    cancel_ui.click("Cancel").expect("should click Cancel");
    let cancel_messages: Vec<_> = cancel_ui.into_messages().collect();
    assert!(
        cancel_messages
            .iter()
            .any(|m| matches!(m, Message::CancelDownload(7))),
        "should emit CancelDownload(7), got: {cancel_messages:?}",
    );
}

#[test]
fn data_tab_shows_conflict_rows_and_count() {
    let state = DataTabState::default();
    let conflicts = vec![(
        "meshes/dragon.nif".to_string(),
        vec!["SkyUI".to_string(), "USSEP".to_string()],
    )];

    let mut ui = simulator(modde_ui::views::data_tab::view(&state, &conflicts));
    ui.find("Data Files").expect("should show title");
    ui.find("meshes/dragon.nif")
        .expect("should show conflict file");
    ui.find("SkyUI, USSEP")
        .expect("should show conflict providers");
    ui.find("1 file(s) shown").expect("should show file count");
}

#[test]
fn data_tab_reports_missing_store_mods_when_empty() {
    let state = DataTabState {
        missing_store_mod_count: 452,
        ..Default::default()
    };
    let conflicts = Vec::new();

    let mut ui = simulator(modde_ui::views::data_tab::view(&state, &conflicts));
    ui.find("452 enabled mod(s) are missing from the store; conflict data is incomplete.")
        .expect("should show incomplete conflict data message");
}

#[test]
fn diagnostics_idle_click_run_emits_message() {
    let mut ui = simulator(modde_ui::views::diagnostics::view(
        &DiagnosticsState::Idle,
        "",
    ));
    ui.find("Diagnostics").expect("should show title");
    ui.click("Run Diagnostics")
        .expect("should click Run Diagnostics");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::RunDiagnostics)),
        "should emit RunDiagnostics, got: {messages:?}",
    );
}

#[test]
fn diagnostics_complete_shows_summary_and_findings() {
    let state = DiagnosticsState::Complete(DiagnosticsReport {
        profile_name: "test-profile".to_string(),
        game_id: "skyrim-se".to_string(),
        entries: vec![
            DiagnosticEntry {
                severity: DiagnosticSeverity::Error,
                message: "Missing master: Update.esm".to_string(),
            },
            DiagnosticEntry {
                severity: DiagnosticSeverity::Warning,
                message: "Loose file conflict detected".to_string(),
            },
        ],
        integrity: IntegritySummary {
            ok_count: 42,
            broken_symlinks: vec![PathBuf::from("/game/broken_link.esp")],
        },
        crash_report: None,
    });

    let mut ui = simulator(modde_ui::views::diagnostics::view(&state, ""));
    ui.find("test-profile (skyrim-se) - 1 error(s), 1 warning(s), 0 info(s), 1 broken symlink(s)")
        .expect("should show diagnostics summary");
    ui.find("Integrity: 42 staged file(s) OK, 1 broken symlink(s)")
        .expect("should show integrity summary");
    ui.find("/game/broken_link.esp")
        .expect("should show broken symlink path");
    ui.find("Missing master: Update.esm")
        .expect("should show error finding");
    ui.find("Loose file conflict detected")
        .expect("should show warning finding");
}

#[test]
fn tools_view_shows_entries_and_actions() {
    let state = sample_tools_state("reshade");

    let mut refresh_ui = simulator(modde_ui::views::tools::view(&state));
    refresh_ui
        .find("Gaming Tools - Skyrim SE")
        .expect("should show title");
    refresh_ui.find("ReShade").expect("should show tool name");
    refresh_ui
        .find("Source directory")
        .expect("should show setting label");
    refresh_ui
        .find("DLL overrides: dxgi")
        .expect("should show launch preview");
    refresh_ui
        .find("2 file(s) applied to game directory")
        .expect("should show applied files");
    refresh_ui
        .find("Applied successfully")
        .expect("should show tool status");
    assert!(
        refresh_ui.find("Executables").is_err(),
        "executables should not render inside Tools view",
    );
    refresh_ui.click("Refresh").expect("should click Refresh");
    let refresh_messages: Vec<_> = refresh_ui.into_messages().collect();
    assert!(
        refresh_messages
            .iter()
            .any(|m| matches!(m, Message::RefreshTools)),
        "should emit RefreshTools, got: {refresh_messages:?}",
    );

    let mut apply_ui = simulator(modde_ui::views::tools::view(&state));
    apply_ui.click("Apply").expect("should click Apply");
    let apply_messages: Vec<_> = apply_ui.into_messages().collect();
    assert!(
        apply_messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "reshade")),
        "should emit ApplyTool(reshade), got: {apply_messages:?}",
    );

    let mut revert_ui = simulator(modde_ui::views::tools::view(&state));
    revert_ui.click("Revert").expect("should click Revert");
    let revert_messages: Vec<_> = revert_ui.into_messages().collect();
    assert!(
        revert_messages
            .iter()
            .any(|m| matches!(m, Message::RevertTool(tool_id) if tool_id == "reshade")),
        "should emit RevertTool(reshade), got: {revert_messages:?}",
    );
}

#[test]
fn tools_view_shows_loading_state_with_existing_entries() {
    let mut state = sample_tools_state("reshade");
    state.loading = true;

    let mut ui = simulator(modde_ui::views::tools::view(&state));

    ui.find("Loading tools...")
        .expect("loading status should render");
    ui.find("ReShade")
        .expect("existing tool entries should remain visible while loading");
}

#[test]
fn tools_actions_disable_while_loading() {
    let mut state = sample_tools_state("reshade");
    state.loading = true;

    let mut refresh_ui = simulator(modde_ui::views::tools::view(&state));
    refresh_ui
        .click("Refresh")
        .expect("refresh button remains visible");
    let messages: Vec<_> = refresh_ui.into_messages().collect();
    assert!(
        !messages.iter().any(|m| matches!(m, Message::RefreshTools)),
        "loading refresh should not emit RefreshTools, got: {messages:?}",
    );

    let mut apply_ui = simulator(modde_ui::views::tools::view(&state));
    apply_ui
        .click("Apply")
        .expect("loading apply button remains visible");
    let messages: Vec<_> = apply_ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "reshade")),
        "loading apply should not emit ApplyTool, got: {messages:?}",
    );

    let mut revert_ui = simulator(modde_ui::views::tools::view(&state));
    revert_ui
        .click("Revert")
        .expect("revert button remains visible");
    let messages: Vec<_> = revert_ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::RevertTool(tool_id) if tool_id == "reshade")),
        "loading revert should not emit RevertTool, got: {messages:?}",
    );
}

#[test]
fn tools_view_shows_registered_tool_tabs_including_proton() {
    let state = ToolState {
        active_tool_id: Some("mangohud".to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        loading: false,
        load_error: None,
        load_generation: 0,
        show_advanced_settings: false,
        active_operations: std::collections::HashSet::new(),
        tool_option_catalog: std::collections::HashMap::from([
            (
                "optiscaler.release_tag".to_string(),
                vec!["v1.0.0".to_string()],
            ),
            (
                "optiscaler.release_asset".to_string(),
                vec!["OptiScaler.zip".to_string()],
            ),
            (
                "proton.selected_version".to_string(),
                vec!["latest".to_string()],
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
        entries: vec![
            sample_tool_entry("mangohud", "MangoHud"),
            sample_tool_entry("vkbasalt", "vkBasalt"),
            sample_tool_entry("gamemode", "GameMode"),
            sample_tool_entry("reshade", "ReShade"),
            sample_tool_entry("optiscaler", "OptiScaler"),
            sample_tool_entry("proton", "Proton"),
        ],
    };

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("MangoHud").expect("MangoHud tab");
    ui.find("vkBasalt").expect("vkBasalt tab");
    ui.find("GameMode").expect("GameMode tab");
    ui.find("ReShade").expect("ReShade tab");
    ui.find("OptiScaler").expect("OptiScaler tab");
    ui.find("Proton").expect("Proton tab");

    ui.click("Proton").expect("should click Proton tab");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::SelectToolTab(tool_id) if tool_id == "proton")),
        "should emit SelectToolTab(proton), got: {messages:?}",
    );
}

#[test]
fn tools_optiscaler_scrollable_has_stable_test_id() {
    let mut state = sample_tools_state("optiscaler");
    state.game_dir_configured = true;
    state.entries = vec![ToolUiEntry {
        has_file_patching: true,
        optiscaler_state: Some("partially managed".to_string()),
        optiscaler_latest_backup: Some("/tmp/backup".to_string()),
        optiscaler_detected_files: 2,
        ..sample_tool_entry("optiscaler", "OptiScaler")
    }];

    let mut ui = simulator(modde_ui::views::tools::view(&state));

    ui.find(by_test_id("tools.optiscaler.scroll"))
        .expect("OptiScaler scrollable should have a stable test id");
    ui.find(by_test_id("tools.optiscaler.adopt"))
        .expect("Adopt button should have a stable test id");
    ui.find(by_test_id("tools.optiscaler.restore_backup"))
        .expect("Restore backup button should have a stable test id");
    ui.find(by_test_id("tools.optiscaler.reset_config"))
        .expect("Reset config button should have a stable test id");
}

#[test]
fn executables_view_renders_configured_executables() {
    let mut state = sample_tools_state("reshade");
    state.executables = vec![ExecutableUiEntry {
        name: "xEdit".to_string(),
        executable_path: "/tools/SSEEdit.exe".to_string(),
        arguments: "-IKnowWhatImDoing".to_string(),
        working_dir: "/games/skyrim".to_string(),
        environment: "WINESYNC=1".to_string(),
        wine_dll_overrides: "dinput8=n,b".to_string(),
        output_mod: "xedit-output".to_string(),
        enabled: true,
    }];

    let mut ui = simulator(modde_ui::views::executables::view(&state));
    ui.find("Executables - Skyrim SE")
        .expect("executables title");
    ui.find("Executables").expect("executables section");
    ui.find("xEdit").expect("saved executable name");
    ui.find("/tools/SSEEdit.exe")
        .expect("saved executable path");
    ui.find(
        "args: -IKnowWhatImDoing | cwd: /games/skyrim | dll: dinput8=n,b | output: xedit-output",
    )
    .expect("saved executable metadata");

    ui.click("Run").expect("run button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::RunExecutable(name) if name == "xEdit")),
        "should emit RunExecutable(xEdit), got: {messages:?}",
    );
}

#[test]
fn executables_view_empty_form_emits_save() {
    let state = sample_tools_state("reshade");
    let mut ui = simulator(modde_ui::views::executables::view(&state));

    ui.find("No executables configured")
        .expect("empty executable state");
    ui.click("Save").expect("save button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::SaveExecutable)),
        "should emit SaveExecutable, got: {messages:?}",
    );
}

#[test]
fn executables_view_refresh_emits_message() {
    let state = sample_tools_state("reshade");
    let mut ui = simulator(modde_ui::views::executables::view(&state));

    ui.click("Refresh").expect("refresh button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::RefreshExecutables)),
        "should emit RefreshExecutables, got: {messages:?}",
    );
}

#[test]
fn tools_optiscaler_release_buttons_emit_messages() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.tool_option_catalog.insert(
        "optiscaler.release_tag".to_string(),
        vec!["v0.7.7".to_string()],
    );
    state.tool_option_catalog.insert(
        "optiscaler.release_asset".to_string(),
        vec!["OptiScaler.zip".to_string()],
    );

    let mut refresh_ui = simulator(modde_ui::views::tools::view(&state));
    refresh_ui
        .click("Refresh releases")
        .expect("refresh releases button");
    let messages: Vec<_> = refresh_ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::RefreshOptiScalerReleases)),
        "should emit RefreshOptiScalerReleases, got: {messages:?}",
    );

    let mut install_ui = simulator(modde_ui::views::tools::view(&state));
    install_ui
        .click("Install selected release")
        .expect("install release button");
    let messages: Vec<_> = install_ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::InstallOptiScalerRelease)),
        "should emit InstallOptiScalerRelease, got: {messages:?}",
    );
}

#[test]
fn tools_optiscaler_release_buttons_disable_while_unready() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.optiscaler_releases_loading = true;
    state.tool_option_catalog.insert(
        "optiscaler.release_asset".to_string(),
        vec!["OptiScaler.zip".to_string()],
    );

    let mut refresh_ui = simulator(modde_ui::views::tools::view(&state));
    refresh_ui
        .click("Loading releases")
        .expect("loading releases button remains visible");
    let messages: Vec<_> = refresh_ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::RefreshOptiScalerReleases)),
        "loading releases button should not emit refresh, got: {messages:?}",
    );

    let mut install_ui = simulator(modde_ui::views::tools::view(&state));
    install_ui
        .click("Install selected release")
        .expect("install release button remains visible");
    let messages: Vec<_> = install_ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::InstallOptiScalerRelease)),
        "install button should not emit while releases are loading, got: {messages:?}",
    );
}

#[test]
fn tools_optiscaler_install_release_disables_without_assets() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state
        .tool_option_catalog
        .insert("optiscaler.release_asset".to_string(), Vec::new());

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.click("Install selected release")
        .expect("install release button remains visible");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::InstallOptiScalerRelease)),
        "install button should not emit without assets, got: {messages:?}",
    );
}

#[test]
fn tools_apply_and_revert_buttons_disable_while_busy() {
    let mut state = sample_tools_state("reshade");
    state.active_operations.insert("reshade".to_string());

    let mut apply_ui = simulator(modde_ui::views::tools::view(&state));
    apply_ui
        .click("Applying")
        .expect("busy apply button remains visible");
    let messages: Vec<_> = apply_ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "reshade")),
        "busy apply should not emit ApplyTool, got: {messages:?}",
    );

    let mut revert_ui = simulator(modde_ui::views::tools::view(&state));
    revert_ui
        .click("Revert")
        .expect("busy revert button remains visible");
    let messages: Vec<_> = revert_ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::RevertTool(tool_id) if tool_id == "reshade")),
        "busy revert should not emit RevertTool, got: {messages:?}",
    );
}

#[test]
fn tools_apply_button_disables_when_current_settings_are_applied() {
    let mut state = sample_tools_state("reshade");
    state.entries[0].apply_pending = false;

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.click("Up to date")
        .expect("already-applied apply button remains visible");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "reshade")),
        "already-applied settings should not emit ApplyTool, got: {messages:?}",
    );
}

#[test]
fn tools_apply_button_disables_when_preview_has_missing_inputs() {
    let mut state = sample_tools_state("reshade");
    state.entries[0].apply_pending = false;
    state.entries[0]
        .apply_missing_inputs
        .push("source DLL missing".to_string());

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.click("Apply")
        .expect("missing-input apply button remains visible");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::ApplyTool(tool_id) if tool_id == "reshade")),
        "missing inputs should not emit ApplyTool, got: {messages:?}",
    );
}

#[test]
fn tools_optiscaler_release_asset_selector_is_rendered() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.tool_option_catalog.insert(
        "optiscaler.release_tag".to_string(),
        vec!["v0.9.1".to_string()],
    );
    state.tool_option_catalog.insert(
        "optiscaler.release_asset".to_string(),
        vec!["Optiscaler_0.9.1-final.7z".to_string()],
    );
    state.entries[0].settings = serde_json::json!({
        "source_mode": "github_release",
        "release_tag": "v0.9.1",
        "release_asset": "Optiscaler_0.9.1-final.7z",
    });

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("OptiScaler Release")
        .expect("release panel title should render");
    ui.find("Source")
        .expect("source selector label should render");
    ui.find("Release tag")
        .expect("release tag selector label should render");
    ui.find("Release asset")
        .expect("release asset selector label should render");
    ui.find("Refresh releases")
        .expect("refresh release action should render in panel");
    ui.find("Install selected release")
        .expect("install release action should render in panel");
}

#[test]
fn tools_optiscaler_local_source_renders_in_release_panel() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.entries[0].setting_specs.push(
        modde_games::tools::ToolSettingSpec::path(
            "local_source_dir",
            "Local source directory",
            "Directory containing OptiScaler.dll and companion files.",
        )
        .section("Source"),
    );
    state.entries[0].settings = serde_json::json!({
        "source_mode": "local_dir",
        "local_source_dir": "/tmp/optiscaler",
        "release_tag": "v0.9.1",
        "release_asset": "Optiscaler_0.9.1-final.7z",
    });

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("OptiScaler Release")
        .expect("release panel title should render");
    ui.find("Source")
        .expect("source selector label should render");
    ui.find("Local source directory")
        .expect("local source control should render in release panel");
    assert!(
        ui.find("Release tag").is_err(),
        "release tag should not render in local directory mode",
    );
    assert!(
        ui.find("Release asset").is_err(),
        "release asset should not render in local directory mode",
    );
}

#[test]
fn tools_optiscaler_advanced_settings_are_hidden_by_default() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.entries[0].setting_specs.push(
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.FSR.Fsr4Update",
            "FSR4 update",
            "FSR4 update.",
        )
        .section("Advanced")
        .advanced(),
    );

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("Show advanced")
        .expect("advanced toggle should render");
    assert!(
        ui.find("FSR4 update").is_err(),
        "advanced OptiScaler setting should be hidden by default",
    );

    ui.click("Show advanced").expect("advanced toggle");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::ToggleToolAdvancedSettings)),
        "advanced toggle should emit ToggleToolAdvancedSettings, got: {messages:?}",
    );
}

#[test]
fn tools_optiscaler_basic_settings_are_visible_by_default() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.entries[0].setting_specs.extend([
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.Spoofing.Dxgi",
            "Spoof DLSS / DXGI",
            "OptiScaler Dxgi override.",
        )
        .section("Basic"),
        modde_games::tools::ToolSettingSpec::labeled_select(
            "ini_overrides.FSR.UpscalerIndex",
            "FSR upscaler backend",
            "OptiScaler [FSR] UpscalerIndex override.",
            &[
                ("auto", "Auto"),
                ("0", "0 - FSR 4.0.2"),
                ("1", "1 - FSR 3.1.5"),
                ("2", "2 - FSR 2.3.4"),
            ],
        )
        .section("Basic"),
    ]);

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("Basic").expect("basic section should render");
    ui.find("Spoof DLSS / DXGI")
        .expect("basic DXGI setting should render");
    ui.find("FSR upscaler backend")
        .expect("basic FSR selector should render");
}

#[test]
fn tools_optiscaler_typed_controls_emit_typed_updates() {
    let mut state = sample_tools_state("optiscaler");
    state.entries = vec![sample_tool_entry("optiscaler", "OptiScaler")];
    state.entries[0].settings = serde_json::json!({
        "copy_companion_files": false,
        "ini_overrides": {
            "FSR": {
                "Fsr4Update": "auto"
            }
        }
    });
    state.entries[0].setting_specs = vec![
        modde_games::tools::ToolSettingSpec::bool(
            "copy_companion_files",
            "Copy companion files",
            "Copy companion files.",
        )
        .section("Deployment"),
        modde_games::tools::ToolSettingSpec::tri_state_bool(
            "ini_overrides.FSR.Fsr4Update",
            "FSR4 update",
            "FSR4 update.",
        )
        .section("OptiScaler Upscaling"),
    ];
    state.show_advanced_settings = true;

    let mut rendered_ui = simulator(modde_ui::views::tools::view(&state));
    rendered_ui
        .find("Copy companion files")
        .expect("boolean setting label should render");

    let mut tri_state_ui = simulator(modde_ui::views::tools::view(&state));
    tri_state_ui.click("On").expect("tri-state on option");
    let messages: Vec<_> = tri_state_ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::UpdateToolSetting { tool_id, key, value }
                if tool_id == "optiscaler"
                    && key == "ini_overrides.FSR.Fsr4Update"
                    && value == &serde_json::json!(true)
        )),
        "should emit typed tri-state update, got: {messages:?}",
    );
}

#[test]
fn tools_proton_install_button_emits_message() {
    let mut state = sample_tools_state("proton");
    state.entries = vec![sample_tool_entry("proton", "Proton")];
    state.tool_option_catalog.insert(
        "proton.selected_version".to_string(),
        vec!["latest".to_string(), "GE-Proton10-1".to_string()],
    );

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.click("Install with protonup-rs")
        .expect("install proton button");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages
            .iter()
            .any(|m| matches!(m, Message::InstallProtonVersion)),
        "should emit InstallProtonVersion, got: {messages:?}",
    );
}

#[test]
fn tools_header_separates_availability_from_enabled_state() {
    let mut state = sample_tools_state("gamemode");
    state.entries = vec![sample_tool_entry("gamemode", "GameMode")];
    state.entries[0].available = true;
    state.entries[0].availability_text = "available".to_string();
    state.entries[0].enabled = false;

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("available")
        .expect("system availability status should render");
    ui.find("Enabled")
        .expect("per-game enabled control should be explicitly labelled");

    let enabled_label = ui
        .find("Enabled")
        .expect("per-game enabled control should be explicitly labelled");
    let label_bounds = enabled_label.bounds();
    ui.point_at(Point::new(
        label_bounds.x + label_bounds.width + 26.0,
        label_bounds.center_y(),
    ));
    let _ = ui.simulate(click());
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::ToggleTool { tool_id, enabled }
                if tool_id == "gamemode" && *enabled
        )),
        "should emit ToggleTool(gamemode, true), got: {messages:?}",
    );
}

#[test]
fn tools_header_disables_enabled_toggle_for_missing_tool() {
    let mut state = sample_tools_state("gamemode");
    state.entries = vec![sample_tool_entry("gamemode", "GameMode")];
    state.entries[0].available = false;
    state.entries[0].availability_text = "missing".to_string();
    state.entries[0].enabled = false;

    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("missing")
        .expect("missing availability status should render");
    let enabled_label = ui
        .find("Enabled")
        .expect("disabled toggler label should still be visible");
    let label_bounds = enabled_label.bounds();
    ui.point_at(Point::new(
        label_bounds.x + label_bounds.width + 26.0,
        label_bounds.center_y(),
    ));
    let _ = ui.simulate(click());
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        !messages
            .iter()
            .any(|m| matches!(m, Message::ToggleTool { .. })),
        "missing tool should not emit ToggleTool, got: {messages:?}",
    );
}

#[test]
fn tools_show_derived_facts_without_exe_subdir_setting() {
    let state = sample_tools_state("reshade");
    let mut ui = simulator(modde_ui::views::tools::view(&state));
    ui.find("Detected Game").expect("derived fact heading");
    ui.find("Executable directory")
        .expect("derived executable directory");
    assert!(
        ui.find("Executable subdir").is_err(),
        "boilerplate executable subdir setting should not be visible",
    );
}

#[test]
fn tools_setting_input_emits_update_message() {
    let state = sample_tools_state("reshade");
    let mut ui = simulator(modde_ui::views::tools::view(&state));

    ui.click("/tmp/reshade")
        .expect("should focus source directory input");
    ui.typewrite("/new");
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(
        messages.iter().any(|m| matches!(
            m,
            Message::UpdateToolSetting { tool_id, key, .. }
                if tool_id == "reshade" && key == "source_dir"
        )),
        "should emit UpdateToolSetting for source_dir, got: {messages:?}",
    );
}

fn sample_tools_state(active_tool_id: &str) -> ToolState {
    ToolState {
        active_tool_id: Some(active_tool_id.to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        loading: false,
        load_error: None,
        load_generation: 0,
        show_advanced_settings: false,
        active_operations: std::collections::HashSet::new(),
        tool_option_catalog: std::collections::HashMap::from([(
            "proton.selected_version".to_string(),
            vec!["latest".to_string()],
        )]),
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
            tool_id: "reshade".to_string(),
            display_name: "ReShade".to_string(),
            description: "Shader injection".to_string(),
            category: "Graphics".to_string(),
            available: true,
            availability_text: "available".to_string(),
            enabled: true,
            settings: serde_json::json!({
                "source_dir": "/tmp/reshade",
                "dll_name": "dxgi.dll",
                "exe_subdir": "",
            }),
            setting_specs: vec![
                modde_games::tools::ToolSettingSpec::path(
                    "source_dir",
                    "Source directory",
                    "Directory containing ReShade files.",
                ),
                modde_games::tools::ToolSettingSpec::select(
                    "dll_name",
                    "Proxy DLL",
                    "DLL copied beside the executable.",
                    &["dxgi.dll", "d3d11.dll"],
                ),
            ],
            generated_config_path: None,
            applied_files: vec!["dxgi.dll".to_string(), "ReShade.ini".to_string()],
            has_file_patching: true,
            release_support: ToolReleaseSupport::None,
            status_message: Some("Applied successfully".to_string()),
            env_preview: Vec::new(),
            dll_overrides: vec!["dxgi".to_string()],
            wrapper_preview: Vec::new(),
            derived_facts: vec![
                ("Game".to_string(), "Skyrim SE".to_string()),
                (
                    "Executable directory".to_string(),
                    "/games/skyrim".to_string(),
                ),
            ],
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

fn sample_tool_entry(tool_id: &str, display_name: &str) -> ToolUiEntry {
    let setting_specs = if tool_id == "optiscaler" {
        vec![
            modde_games::tools::ToolSettingSpec::labeled_select(
                "source_mode",
                "Source",
                "Where modde should get OptiScaler files from.",
                &[
                    ("github_release", "Official GitHub releases"),
                    ("goverlay_builds", "GOverlay builds"),
                    ("goverlay_fgmod", "GOverlay fgmod directory"),
                    ("local_dir", "Local OptiScaler directory"),
                ],
            ),
            modde_games::tools::ToolSettingSpec::select(
                "release_tag",
                "Release tag",
                "OptiScaler GitHub release tag selected in the UI.",
                &["v0.9.1"],
            ),
            modde_games::tools::ToolSettingSpec::select(
                "release_asset",
                "Release asset",
                "Release asset selected from GitHub.",
                &["Optiscaler_0.9.1-final.7z"],
            ),
        ]
    } else {
        vec![modde_games::tools::ToolSettingSpec::read_only(
            "summary",
            "Summary",
            "Tool summary.",
        )]
    };
    ToolUiEntry {
        tool_id: tool_id.to_string(),
        display_name: display_name.to_string(),
        description: format!("{display_name} settings"),
        category: "Tool".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: false,
        settings: if tool_id == "optiscaler" {
            serde_json::json!({
                "source_mode": "github_release",
                "release_tag": "v0.9.1",
                "release_asset": "Optiscaler_0.9.1-final.7z",
            })
        } else {
            serde_json::json!({ "summary": display_name })
        },
        setting_specs,
        generated_config_path: None,
        applied_files: Vec::new(),
        has_file_patching: false,
        release_support: ToolReleaseSupport::from_supports_releases(tool_id == "optiscaler"),
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
    }
}
