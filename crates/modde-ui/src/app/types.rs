#![allow(clippy::wildcard_imports)]
use super::*;

/// Settings view state — consumed by the settings view.
#[derive(Debug, Clone, Default)]
pub struct SettingsState {
    pub nexus_api_key_draft: String,
    pub nexus_api_key_visible: bool,
    pub nexus_api_key_source: Option<modde_sources::nexus::auth::ApiKeySource>,
    pub nexus_config_key_exists: bool,
    pub game_install_paths: Vec<SettingsGameInstall>,
    pub download_dir: Option<PathBuf>,
    pub effective_download_dir: PathBuf,
    pub has_stock_snapshot: bool,
    pub theme_name: String,
    pub nexus_status: Option<NexusAuthStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsGameInstall {
    pub game_id: String,
    pub display_name: String,
    pub source: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub enum NexusAuthStatus {
    Checking,
    Valid { username: String, is_premium: bool },
    Invalid(String),
}

#[derive(Debug, Clone)]
pub enum ProfileWriteKind {
    Create {
        name: String,
        game_id: String,
    },
    Delete {
        name: String,
    },
    Fork {
        new_name: String,
    },
    AddMod {
        mod_id: String,
    },
    RemoveMod,
    ToggleMod {
        mod_id: String,
        enabled: bool,
    },
    Reorder {
        mod_id: String,
        direction: ReorderDirection,
    },
    Lock {
        mod_id: String,
    },
    Unlock {
        mod_id: String,
    },
}

#[derive(Debug, Clone)]
pub struct ProfileWriteOutcome {
    pub status_message: Option<String>,
    pub reload: bool,
}

#[derive(Debug, Clone)]
pub struct WabbajackInstallUiSummary {
    pub status_message: String,
}

#[derive(Debug, Clone)]
pub enum WabbajackInstallEvent {
    Progress(modde_sources::wabbajack::installer::InstallProgress),
    Complete(Result<WabbajackInstallUiSummary, String>),
}

#[derive(Debug, Clone)]
pub enum ExperimentWriteKind {
    Try,
    Rollback,
    Commit,
}

#[derive(Debug, Clone)]
pub struct ExperimentWriteOutcome {
    pub previous_profile: Option<String>,
    pub status_message: String,
    pub reload: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ButtonHoverToast {
    pub id: u64,
    pub description: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ButtonHoverToastState {
    pub pending: Option<ButtonHoverToast>,
    pub visible: Option<ButtonHoverToast>,
}

/// Top-level application state.
#[allow(clippy::struct_excessive_bools)]
pub struct Modde {
    pub(crate) db: modde_core::db::ModdeDb,
    pub active_view: View,
    pub active_profile: Option<String>,
    pub profiles: Vec<modde_core::profile::ProfileSummary>,
    pub status_message: String,
    pub button_hover_toast: ButtonHoverToastState,
    pub pending_tools_load_status_message: Option<String>,
    pub settings: AppSettings,
    pub collection_search: String,
    pub collections: Vec<CollectionManifest>,
    pub fomod_installer: Option<FOMODWizardState>,
    pub fomod_visible_step_indices: SmallVec<[usize; 16]>,
    pub fomod_wizard_pos: usize,
    pub fomod_source_dir: Option<PathBuf>,
    pub fomod_dest_dir: Option<PathBuf>,
    pub fomod_conflicts: SmallVec<[String; 4]>,
    pub fomod_can_undo: bool,
    pub fomod_selections: HashMap<(usize, usize), Vec<usize>>,
    pub selected_mod_index: Option<usize>,
    /// Loaded Nexus metadata for the currently selected mod — populates the
    /// detail panel at the bottom of the left nav sidebar. `None` means no
    /// Nexus-tracked mod is selected (either nothing is selected or the
    /// selected mod has no `nexus_mod_id`).
    pub selected_mod_details: Option<crate::views::mod_details::ModDetailsState>,
    pub mod_filter: String,
    pub mod_id_filter_keys: Vec<String>,
    pub theme_name: String,
    pub wabbajack_manifest: Option<modde_core::WabbajackManifest>,
    pub active_downloads: Vec<crate::views::collections::CollectionDownload>,
    pub download_queue: modde_sources::queue::DownloadQueue,
    pub download_lookup: HashMap<String, usize>,
    // ── New state fields ──
    pub loaded_profile: Option<modde_core::Profile>,
    pub save_snapshots: Vec<SaveSnapshot>,
    pub current_fingerprint: Option<modde_core::save::SaveFingerprint>,
    pub selected_save_details: Option<crate::views::save_details::SaveDetailsState>,
    pub experiment_depth: usize,
    pub nexus_status: Option<NexusAuthStatus>,
    pub nexus_api_key_draft: String,
    pub nexus_api_key_visible: bool,
    pub nexus_api_key_source: Option<modde_sources::nexus::auth::ApiKeySource>,
    pub nexus_config_key_exists: bool,
    pub new_profile_name: String,
    pub new_profile_dialog_open: bool,
    pub game_path_dialog_open: bool,
    pub add_custom_game_dialog_open: bool,
    pub manage_custom_games_dialog_open: bool,
    pub pending_game_path_game_id: Option<String>,
    pub previous_game_before_path_dialog: Option<String>,
    pub game_path_dialog_error: Option<String>,
    pub add_custom_game: AddCustomGameState,
    pub available_games: SmallVec<[(String, String); 8]>,
    pub detected_games: HashSet<String>,
    pub selected_game: Option<String>,
    pub stock_snapshot_exists: bool,
    pub window_id: window::Id,
    /// Which category groups are collapsed in the mod list view.
    /// `None` key = the "Uncategorized" group.
    pub collapsed_categories: HashSet<Option<i64>>,
    /// Category id-to-name mapping for the mod list view.
    pub mod_categories: Vec<(Option<i64>, String)>,
    pub data_tab_state: crate::views::data_tab::DataTabState,
    pub data_tab_conflicts: Vec<(String, Vec<String>)>,
    /// State for the Browse Nexus view (Phase 6 of the installer pipeline).
    pub browse_nexus: crate::views::browse_nexus::NexusBrowseState,
    pub diagnostics_state: crate::views::diagnostics::DiagnosticsState,
    pub crash_log_path_draft: String,
    pub library: crate::views::library::LibraryState,
    pub tool_state: ToolState,
    /// Filter mode (AND/OR) for the mod list filter toolbar.
    pub filter_mode: FilterMode,
    /// Active tri-state filter criteria for the mod list.
    pub filter_criteria: Vec<FilterCriterion>,
    /// Whether the mod list uses compact row rendering.
    pub compact_mod_list: bool,
    /// Sidebar groups the user has collapsed for this session.
    pub collapsed_sidebar_groups: HashSet<SidebarGroup>,
    pub update_available: Option<modde_core::update_check::UpdateInfo>,
    /// Generation guard for the async profile/game-context load. Bumped on
    /// every `Message::ProfileContextLoaded`-producing kickoff; a resolved load
    /// whose captured generation no longer matches is discarded, so a slow load
    /// for game A can never clobber state after the user switched to game B.
    pub context_generation: u64,
    /// Generation guard for the lightweight async data-tab conflict refresh
    /// (`Message::DataTabConflictsLoaded`). Independent of `context_generation`
    /// so opening the Data tab never cancels an in-flight profile reload.
    pub data_tab_generation: u64,
    /// Generation guard for async diagnostics runs. Bumped by diagnostics
    /// kickoffs and by profile-context reloads so stale reports for a previous
    /// active profile never overwrite the current profile's diagnostics state.
    pub diagnostics_generation: u64,
}
