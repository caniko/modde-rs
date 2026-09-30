#![allow(clippy::wildcard_imports)]
use super::*;
use iced_test::Selector;
use iced_test::core::widget::Operation;
use iced_test::core::{Event, Font, Point, Settings, Size, mouse};
use iced_test::selector;
use iced_test::selector::Bounded;
use modde_core::profile::{LoadOrderLock, LockReason, ProfileManager, ProfileSource};
use std::path::{Path, PathBuf};

fn test_app() -> Modde {
    fixtures::isolated_data_dir();
    Modde {
        db: test_db(),
        active_view: View::ModList,
        active_profile: None,
        profiles: Vec::new(),
        status_message: "Ready".to_string(),
        button_hover_toast: ButtonHoverToastState::default(),
        pending_tools_load_status_message: None,
        settings: AppSettings::default(),
        collection_search: String::new(),
        collections: Vec::new(),
        fomod_installer: None,
        fomod_visible_step_indices: SmallVec::new(),
        fomod_wizard_pos: 0,
        fomod_source_dir: None,
        fomod_dest_dir: None,
        fomod_conflicts: SmallVec::new(),
        fomod_can_undo: false,
        fomod_selections: HashMap::new(),
        selected_mod_index: None,
        selected_mod_details: None,
        mod_filter: String::new(),
        mod_id_filter_keys: Vec::new(),
        theme_name: "Dark".to_string(),
        wabbajack_manifest: None,
        active_downloads: Vec::new(),
        download_queue: modde_sources::queue::DownloadQueue::new(2),
        download_lookup: HashMap::new(),
        loaded_profile: None,
        save_snapshots: Vec::new(),
        current_fingerprint: None,
        selected_save_details: None,
        experiment_depth: 0,
        nexus_status: None,
        nexus_api_key_draft: String::new(),
        nexus_api_key_visible: false,
        nexus_api_key_source: None,
        nexus_config_key_exists: false,
        new_profile_name: String::new(),
        new_profile_dialog_open: false,
        game_path_dialog_open: false,
        add_custom_game_dialog_open: false,
        manage_custom_games_dialog_open: false,
        pending_game_path_game_id: None,
        previous_game_before_path_dialog: None,
        game_path_dialog_error: None,
        add_custom_game: AddCustomGameState::default(),
        available_games: smallvec::smallvec![
            ("skyrim-se".to_string(), "Skyrim SE".to_string()),
            ("fallout4".to_string(), "Fallout 4".to_string()),
            ("stellar-blade".to_string(), "Stellar Blade".to_string()),
        ],
        detected_games: HashSet::new(),
        selected_game: None,
        stock_snapshot_exists: false,
        window_id: window::Id::unique(),
        collapsed_categories: HashSet::new(),
        mod_categories: vec![(None, "Uncategorized".to_string())],
        data_tab_state: Default::default(),
        data_tab_conflicts: Vec::new(),
        diagnostics_state: Default::default(),
        crash_log_path_draft: String::new(),
        library: Default::default(),
        tool_state: Default::default(),
        browse_nexus: Default::default(),
        filter_mode: FilterMode::default(),
        filter_criteria: vec![
            FilterCriterion::new(FilterKind::Enabled),
            FilterCriterion::new(FilterKind::HasNotes),
            FilterCriterion::new(FilterKind::HasNexusId),
        ],
        compact_mod_list: false,
        collapsed_sidebar_groups: HashSet::from([SidebarGroup::General]),
        update_available: None,
        context_generation: 0,
        data_tab_generation: 0,
        diagnostics_generation: 0,
    }
}

fn by_test_id(test_id: &str) -> impl iced_test::Selector<Output = iced_test::selector::Target> {
    selector::id(crate::semantics::widget_id(test_id.to_string()))
}

struct AppUiHarness {
    app: Modde,
    renderer: iced_test::renderer::Renderer,
    cache: iced_test::runtime::user_interface::Cache,
    size: Size,
    cursor: mouse::Cursor,
}

impl AppUiHarness {
    fn new(app: Modde, size: Size) -> Self {
        use iced_test::core::renderer::Headless;

        let settings = Settings::default();
        let default_font = match settings.default_font {
            Font::DEFAULT => Font::with_name("Fira Sans"),
            font => font,
        };
        let renderer = iced_test::futures::futures::executor::block_on(
            iced_test::renderer::Renderer::new(default_font, settings.default_text_size, None),
        )
        .expect("create headless renderer");

        Self {
            app,
            renderer,
            cache: iced_test::runtime::user_interface::Cache::new(),
            size,
            cursor: mouse::Cursor::Unavailable,
        }
    }

    fn rebuild(&mut self) {
        let cache = std::mem::take(&mut self.cache);
        let ui = iced_test::runtime::UserInterface::build(
            self.app.view(),
            self.size,
            cache,
            &mut self.renderer,
        );
        self.cache = ui.into_cache();
    }

    fn find<S>(&mut self, selector: S) -> S::Output
    where
        S: Selector + Send,
        S::Output: Clone + Send,
    {
        let description = selector.description();
        let cache = std::mem::take(&mut self.cache);
        let mut ui = iced_test::runtime::UserInterface::build(
            self.app.view(),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut operation = selector.find();

        ui.operate(
            &self.renderer,
            &mut iced_test::core::widget::operation::black_box(&mut operation),
        );
        let output = match operation.finish() {
            iced_test::core::widget::operation::Outcome::Some(output) => {
                output.unwrap_or_else(|| panic!("selector should find target: {description}"))
            }
            _ => panic!("selector should find target: {description}"),
        };

        self.cache = ui.into_cache();
        output
    }

    fn point_at(&mut self, position: Point) {
        self.cursor = mouse::Cursor::Available(position);
    }

    fn move_cursor_to(&mut self, position: Point) {
        self.point_at(position);
        self.simulate([Event::Mouse(mouse::Event::CursorMoved { position })]);
    }

    fn simulate(&mut self, events: impl IntoIterator<Item = Event>) {
        let events = events.into_iter().collect::<Vec<_>>();
        let cache = std::mem::take(&mut self.cache);
        let mut ui = iced_test::runtime::UserInterface::build(
            self.app.view(),
            self.size,
            cache,
            &mut self.renderer,
        );
        let mut messages = Vec::new();
        let _ = ui.update(
            &events,
            self.cursor,
            &mut self.renderer,
            &mut iced_test::core::clipboard::Null,
            &mut messages,
        );
        self.cache = ui.into_cache();

        for message in messages {
            let _ = self.app.update(message);
        }
    }

    fn click<S>(&mut self, selector: S)
    where
        S: Selector + Send,
        S::Output: iced_test::selector::Bounded + Clone + Send,
    {
        let target = self.find(selector);
        let bounds = target
            .visible_bounds()
            .expect("click target should be visible");

        self.point_at(bounds.center());
        self.simulate(iced_test::simulator::click());
    }

    fn click_near_left<S>(&mut self, selector: S)
    where
        S: Selector + Send,
        S::Output: iced_test::selector::Bounded + Clone + Send,
    {
        let target = self.find(selector);
        let bounds = target
            .visible_bounds()
            .expect("click target should be visible");

        self.point_at(Point::new(bounds.x + 8.0, bounds.y + (bounds.height / 2.0)));
        self.simulate(iced_test::simulator::click());
    }

    fn typewrite(&mut self, text: &str) {
        self.simulate(iced_test::simulator::typewrite(text));
    }

    fn update_app(&mut self, message: Message) {
        let _ = self.app.update(message);
        self.rebuild();
    }
}

fn test_tool_ui_entry(tool_id: &str) -> ToolUiEntry {
    ToolUiEntry {
        tool_id: tool_id.to_string(),
        display_name: tool_id.to_string(),
        description: "test tool".to_string(),
        category: "test".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: false,
        settings: serde_json::json!({}),
        setting_specs: Vec::new(),
        generated_config_path: None,
        applied_files: Vec::new(),
        has_file_patching: false,
        release_support: ToolReleaseSupport::None,
        status_message: None,
        env_preview: Vec::new(),
        dll_overrides: Vec::new(),
        wrapper_preview: Vec::new(),
        derived_facts: Vec::new(),
        optiscaler_state: None,
        optiscaler_latest_backup: None,
        optiscaler_detected_files: 0,
        apply_pending: false,
        apply_missing_inputs: Vec::new(),
        setting_history: Vec::new(),
        config_checklist: Vec::new(),
        dirty_keys: std::collections::HashSet::new(),
    }
}

fn test_tool_load_snapshot(entries: Vec<ToolUiEntry>) -> ToolLoadSnapshot {
    ToolLoadSnapshot {
        entries,
        active_tool_id: Some("reshade".to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        executables: Vec::new(),
        tool_option_catalog: HashMap::from([
            (
                "proton.selected_version".to_string(),
                vec!["latest".to_string()],
            ),
            (
                "optiscaler.release_tag".to_string(),
                vec!["v1.0.0".to_string()],
            ),
            (
                "optiscaler.release_asset".to_string(),
                vec!["OptiScaler.zip".to_string()],
            ),
        ]),
    }
}

fn scroll_heavy_tools_app() -> Modde {
    let mut app = test_app();
    app.active_view = View::Tools;
    app.tool_state = scroll_heavy_optiscaler_state();
    app
}

fn scroll_heavy_optiscaler_state() -> ToolState {
    let mut entry = test_tool_ui_entry("optiscaler");
    let mut settings = serde_json::Map::new();
    let mut setting_specs = Vec::new();

    for index in 0..40 {
        let key = format!("stress_setting_{index}");
        let label = format!("Stress setting {index}");
        let description = format!("Long setting row used to make the tool panel scroll {index}.");

        settings.insert(key.clone(), serde_json::json!(false));
        setting_specs.push(modde_games::tools::ToolSettingSpec::bool(
            key,
            label,
            description,
        ));
    }

    entry.display_name = "OptiScaler".to_string();
    entry.description = "Upscaling injection".to_string();
    entry.category = "Graphics".to_string();
    entry.available = true;
    entry.availability_text = "available".to_string();
    entry.enabled = true;
    entry.settings = serde_json::Value::Object(settings);
    entry.setting_specs = setting_specs;
    entry.applied_files = vec!["dxgi.dll".to_string(), "OptiScaler.ini".to_string()];
    entry.has_file_patching = true;
    entry.release_support = ToolReleaseSupport::Supported;
    entry.optiscaler_state = Some(
        "partially managed; version goverlay-edge_edge-0.9.12.0323; proxy d3d12.dll".to_string(),
    );
    entry.optiscaler_latest_backup = Some("/tmp/optiscaler-backup".to_string());
    entry.optiscaler_detected_files = 3;
    entry.apply_pending = true;

    let mut state = ToolState {
        active_tool_id: Some("optiscaler".to_string()),
        game_label: Some("Stellar Blade".to_string()),
        game_dir_configured: true,
        show_advanced_settings: true,
        entries: vec![entry],
        ..Default::default()
    };
    state.tool_option_catalog.insert(
        "optiscaler.release_tag".to_string(),
        vec!["v0.9.1".to_string()],
    );
    state.tool_option_catalog.insert(
        "optiscaler.release_asset".to_string(),
        vec!["Optiscaler_0.9.1-final.7z".to_string()],
    );
    state
}

fn scroll_y(target: iced_test::selector::Target) -> f32 {
    match target {
        iced_test::selector::Target::Scrollable { translation, .. } => translation.y,
        other => panic!("expected scrollable target, got {other:?}"),
    }
}

fn scroll_tools_panel_down(harness: &mut AppUiHarness) -> f32 {
    let scrollable = harness.find(by_test_id("tools.optiscaler.scroll"));
    let bounds = scrollable
        .visible_bounds()
        .expect("tools scrollable should be visible");
    harness.point_at(bounds.center());
    harness.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -900.0 },
    })]);

    let scrolled_y = scroll_y(harness.find(by_test_id("tools.optiscaler.scroll")));
    assert!(
        scrolled_y.abs() > 1.0,
        "test setup must scroll the Tools panel before checking stability; got translation {scrolled_y}",
    );
    scrolled_y
}

fn scroll_until_visible(
    harness: &mut AppUiHarness,
    test_id: &str,
) -> (iced_test::selector::Target, f32) {
    let scrollable = harness.find(by_test_id("tools.optiscaler.scroll"));
    let bounds = scrollable
        .visible_bounds()
        .expect("tools scrollable should be visible");
    harness.point_at(bounds.center());

    for _ in 0..16 {
        let target = harness.find(by_test_id(test_id));
        let scroll_y = scroll_y(harness.find(by_test_id("tools.optiscaler.scroll")));
        if target.visible_bounds().is_some() {
            assert!(
                scroll_y.abs() > 1.0,
                "test target {test_id} must be reached after scrolling"
            );
            return (target, scroll_y);
        }

        harness.simulate([Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -700.0 },
        })]);
    }

    panic!("target {test_id} did not become visible after scrolling");
}

fn assert_scroll_y_unchanged(harness: &mut AppUiHarness, before: f32) {
    let after = scroll_y(harness.find(by_test_id("tools.optiscaler.scroll")));
    assert!(
        (after - before).abs() <= 0.5,
        "Tools scroll position changed: before={before}, after={after}",
    );
}

#[path = "tests/core.rs"]
mod core;
#[path = "tests/fixtures.rs"]
mod fixtures;
#[path = "tests/library.rs"]
mod library;
#[path = "tests/profiles.rs"]
mod profiles;
#[path = "tests/reorder.rs"]
mod reorder;
#[path = "tests/tool_settings.rs"]
mod tool_settings;
#[path = "tests/tools_ui.rs"]
mod tools_ui;
#[path = "tests/wabbajack.rs"]
mod wabbajack;

pub(super) use fixtures::{db_lock, reset_isolated_db, test_db};
pub(super) use profiles::{
    complete_create_profile_write, complete_experiment_write, complete_lock_write,
    complete_reorder_write, loaded_test_app, mod_ids, profile_for_game, reload_seeded, seed_mod,
    seed_profile,
};
