use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use modde_core::resolver::GameId;
use modde_core::settings::AppSettings;
use modde_games::tools::ToolSettingSpec;

use super::{FOMODWizardState, ToolOptionCatalog};

/// Direction for `Message::ReorderMod`. Re-export of the core type so
/// view and message construction sites don't have to import from
/// `modde_core::profile` directly. The enforcement logic itself lives in
/// `modde_core::profile::try_reorder` and is the single source of truth.
pub use modde_core::profile::ReorderDirection;

#[path = "state_parts/wabbajack.rs"]
mod wabbajack;

pub(super) use self::wabbajack::prefill_wabbajack_game_dir;
pub use self::wabbajack::{WabbajackInstallerState, WabbajackTab};
/// Render a `LockReason` as a short, human-readable phrase for status
/// messages and UI banners. Centralised so `lock_reason` strings stay
/// consistent across the handler, `load_order` banner, and `mod_list` row.
pub(crate) fn format_lock_reason(reason: &modde_core::LockReason) -> String {
    use modde_core::LockReason::{Manual, NexusCollection, TomlImport, Wabbajack};
    match reason {
        Wabbajack { manifest_hash } => format!("Wabbajack (hash {manifest_hash})"),
        NexusCollection { slug, version } => format!("Nexus Collection '{slug}' v{version}"),
        TomlImport { source_path } => format!("TOML import from {source_path}"),
        Manual { note: Some(n) } => format!("manual ({n})"),
        Manual { note: None } => "manual".to_string(),
    }
}

/// Which view is currently displayed.
#[derive(Debug, Clone)]
pub enum View {
    ModList,
    Library,
    Collections,
    /// Unified Nexus browse surface — Top / Month / Collections / Search.
    BrowseNexus,
    WabbajackInstaller(WabbajackInstallerState),
    FOMODWizard(FOMODWizardState),
    Settings,
    Saves,
    Downloads,
    DataTab,
    Diagnostics,
    Tools,
    Executables,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SidebarGroup {
    Game,
    Install,
    General,
}

impl SidebarGroup {
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SidebarGroup::Game => "Game",
            SidebarGroup::Install => "Install",
            SidebarGroup::General => "General",
        }
    }
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Default)]
pub struct ToolState {
    pub entries: Vec<ToolUiEntry>,
    pub active_tool_id: Option<String>,
    pub game_label: Option<String>,
    pub game_dir_configured: bool,
    pub loading: bool,
    pub load_error: Option<String>,
    pub load_generation: u64,
    pub show_advanced_settings: bool,
    pub active_operations: HashSet<String>,
    pub tool_option_catalog: ToolOptionCatalog,
    pub optiscaler_releases: Vec<modde_games::tools::ToolReleaseSummary>,
    pub optiscaler_releases_loading: bool,
    pub proton_versions_loading: bool,
    pub executables: Vec<ExecutableUiEntry>,
    pub executables_loading: bool,
    pub executables_load_error: Option<String>,
    pub executables_load_generation: u64,
    pub executable_draft: ExecutableDraft,
    pub executable_editor_open: bool,
    pub executable_error: Option<String>,
    pub active_executable_operations: HashSet<String>,
}

impl ToolState {
    #[must_use]
    pub fn is_tool_busy(&self, tool_id: &str) -> bool {
        self.active_operations.contains(tool_id)
    }

    #[must_use]
    pub fn is_executable_busy(&self, name: &str) -> bool {
        self.active_executable_operations.contains(name)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutableUiEntry {
    pub name: String,
    pub executable_path: String,
    pub arguments: String,
    pub working_dir: String,
    pub environment: String,
    pub wine_dll_overrides: String,
    pub output_mod: String,
    pub enabled: bool,
}

impl ExecutableUiEntry {
    pub(super) fn from_row(row: modde_core::db::ExecutableConfigRow) -> Self {
        let args = serde_json::from_str::<Vec<String>>(&row.arguments_json).unwrap_or_default();
        let env = serde_json::from_str::<HashMap<String, String>>(&row.environment_json)
            .unwrap_or_default();
        let mut env_lines = env
            .into_iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect::<Vec<_>>();
        env_lines.sort();
        Self {
            name: row.name,
            executable_path: row.executable_path.display().to_string(),
            arguments: args.join(" "),
            working_dir: row
                .working_dir
                .map(|path| path.display().to_string())
                .unwrap_or_default(),
            environment: env_lines.join("\n"),
            wine_dll_overrides: row.wine_dll_overrides.unwrap_or_default(),
            output_mod: row.output_mod,
            enabled: row.enabled,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableDraft {
    pub name: String,
    pub executable_path: String,
    pub arguments: String,
    pub working_dir: String,
    pub environment: String,
    pub wine_dll_overrides: String,
    pub output_mod: String,
}

impl Default for ExecutableDraft {
    fn default() -> Self {
        Self {
            name: String::new(),
            executable_path: String::new(),
            arguments: String::new(),
            working_dir: String::new(),
            environment: String::new(),
            wine_dll_overrides: String::new(),
            output_mod: "__overwrite__".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutableDraftField {
    Name,
    Path,
    Arguments,
    WorkingDir,
    Environment,
    WineDllOverrides,
    OutputMod,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddCustomGameDraft {
    pub id: String,
    pub display_name: String,
    pub install_path: String,
    pub executable_dir: Option<String>,
    pub steam_app_id: Option<String>,
    pub nexus_domain: Option<String>,
    pub proxy_dlls_csv: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddCustomGameState {
    pub draft: AddCustomGameDraft,
    pub detected_dirs: Vec<modde_games::DetectCandidateDir>,
    pub error: Option<String>,
}

impl AddCustomGameState {
    #[must_use]
    pub fn can_submit(&self) -> bool {
        self.build_spec().is_ok()
    }

    pub fn build_spec(&self) -> Result<modde_games::generic::spec::GameSpec, String> {
        let install_path = PathBuf::from(self.draft.install_path.trim());
        if self.draft.id.trim().is_empty()
            || self.draft.display_name.trim().is_empty()
            || self.draft.executable_dir.is_none()
            || self.draft.install_path.trim().is_empty()
        {
            return Err("Fill in the required custom game fields.".to_string());
        }
        if !install_path.is_dir() {
            return Err(format!(
                "Install path does not exist: {}",
                install_path.display()
            ));
        }

        let spec = modde_games::generic::spec::GameSpec {
            id: self.draft.id.trim().to_string(),
            display_name: self.draft.display_name.trim().to_string(),
            steam_app_id: empty_to_none(self.draft.steam_app_id.as_deref()),
            install_dir_name: None,
            install_path_override: None,
            executable_dir: PathBuf::from(
                self.draft
                    .executable_dir
                    .as_deref()
                    .unwrap_or_default()
                    .trim(),
            ),
            mod_dir: None,
            nexus_domain: empty_to_none(self.draft.nexus_domain.as_deref()),
            proxy_dlls: self
                .draft
                .proxy_dlls_csv
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect(),
        };

        spec.validate().map_err(|error| error.to_string())?;
        Ok(spec)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddCustomGameDraftField {
    Id,
    DisplayName,
    InstallPath,
    ExecutableDir,
    SteamAppId,
    NexusDomain,
    ProxyDlls,
}

pub(super) fn empty_to_none(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecklistStatus {
    Done,
    Pending,
    Blocked,
}

#[derive(Debug, Clone)]
pub struct ConfigChecklistItem {
    pub label: String,
    pub status: ChecklistStatus,
    pub hint: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolUiEntry {
    pub tool_id: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    pub available: bool,
    pub availability_text: String,
    pub enabled: bool,
    pub settings: serde_json::Value,
    pub setting_specs: Vec<modde_games::tools::ToolSettingSpec>,
    pub generated_config_path: Option<String>,
    pub applied_files: Vec<String>,
    pub has_file_patching: bool,
    pub release_support: ToolReleaseSupport,
    pub status_message: Option<String>,
    pub env_preview: Vec<(String, String)>,
    pub dll_overrides: Vec<String>,
    pub wrapper_preview: Vec<String>,
    pub derived_facts: Vec<(String, String)>,
    pub optiscaler_state: Option<String>,
    pub optiscaler_latest_backup: Option<String>,
    pub optiscaler_detected_files: usize,
    pub apply_pending: bool,
    pub apply_missing_inputs: Vec<String>,
    pub setting_history: Vec<ToolHistoryUiEntry>,
    pub config_checklist: Vec<ConfigChecklistItem>,
    pub dirty_keys: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolHistoryUiEntry {
    pub node_id: String,
    pub label: String,
    pub reason: String,
    pub human_reason: String,
    pub enabled: bool,
    pub is_current: bool,
}

impl ToolHistoryUiEntry {
    pub(super) fn from_node(
        node: modde_core::db::ToolSettingHistoryNode,
        setting_specs: &[modde_games::tools::ToolSettingSpec],
    ) -> Self {
        let short_id = node.node_id.chars().take(18).collect::<String>();
        let state = if node.enabled { "enabled" } else { "disabled" };
        let human_reason = Self::humanize_reason(&node.reason, setting_specs);
        Self {
            node_id: node.node_id,
            label: format!("{} - {state}", node.created_at),
            reason: node.reason,
            human_reason,
            enabled: node.enabled,
            is_current: node.is_current,
        }
        .with_short_id(short_id)
    }

    fn humanize_reason(reason: &str, setting_specs: &[ToolSettingSpec]) -> String {
        if let Some(key) = reason.strip_prefix("ui:set:") {
            if let Some(spec) = setting_specs.iter().find(|s| s.key == key) {
                return format!("Changed {}", spec.label);
            }
            let pretty = key
                .strip_prefix("ini_overrides.")
                .unwrap_or(key)
                .replace('_', " ");
            return format!("Changed {pretty}");
        }
        match reason {
            "ui:apply" => "Applied configuration",
            "ui:enable" => "Enabled",
            "ui:disable" => "Disabled",
            "ui:adopt" => "Adopted existing install",
            "ui:reset" => "Reset config",
            "ui:update" => "Updated release selection",
            "ui:deactivate" => "Deactivated",
            r if r.starts_with("restore:") => "Restored previous version",
            other => other,
        }
        .to_string()
    }

    fn with_short_id(mut self, short_id: String) -> Self {
        if !short_id.is_empty() {
            self.label = format!("{} ({short_id})", self.label);
        }
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolReleaseSupport {
    None,
    Supported,
}

impl ToolReleaseSupport {
    #[must_use]
    pub fn from_supports_releases(supports_releases: bool) -> Self {
        if supports_releases {
            Self::Supported
        } else {
            Self::None
        }
    }

    #[must_use]
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

#[derive(Debug, Clone)]
pub struct ToolApplyResult {
    pub display_name: String,
    pub applied_file_count: usize,
    pub validation_message: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ToolRevertResult {
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct ToolLoadSnapshot {
    pub entries: Vec<ToolUiEntry>,
    pub active_tool_id: Option<String>,
    pub game_label: Option<String>,
    pub game_dir_configured: bool,
    pub tool_option_catalog: ToolOptionCatalog,
    pub executables: Vec<ExecutableUiEntry>,
}

#[derive(Debug, Clone)]
pub struct ToolSettingWriteResult {
    pub status_message: String,
    pub tool_option_catalog: Option<ToolOptionCatalog>,
}

#[derive(Debug, Clone)]
pub(crate) struct ToolLoadRequest {
    pub(super) game_id: String,
    pub(super) display_name: String,
    pub(super) configured_game_dir: Option<PathBuf>,
    pub(super) optiscaler_releases: Vec<modde_games::tools::ToolReleaseSummary>,
    pub(super) tool_option_catalog: ToolOptionCatalog,
    pub(super) previous_active_tool_id: Option<String>,
}

/// Owned, off-thread-computed result of (re)loading the profile + data-tab +
/// tool context for a game/profile pair.
///
/// Built by `load_profile_context` on a blocking task and
/// applied synchronously by `Modde::apply_profile_context`, so the iced event
/// loop never blocks on the multi-query profile reload that
/// `reload_profile`/`switch_game_context` used to perform inline.
#[derive(Debug, Clone)]
pub struct ProfileContextSnapshot {
    pub profiles: Vec<modde_core::profile::ProfileSummary>,
    pub active_profile: Option<String>,
    pub profile_outcome: ProfileLoadOutcome,
    pub data_tab_conflicts: Vec<(String, Vec<String>)>,
    pub missing_store_mod_count: usize,
    /// Folded tool snapshot. `Some` when a game is in scope (apply it via
    /// `apply_tool_snapshot`); `None` when no game is selected (clear the tool
    /// state, mirroring the old `refresh_tools_state` empty branch).
    pub tools: Option<ToolLoadSnapshot>,
    /// Re-run diagnostics after applying, when the Diagnostics view is active.
    /// Only set by the profile-switch handler (preserves the old synchronous
    /// `SwitchProfile` behavior).
    pub rerun_diagnostics: bool,
}

#[derive(Debug, Clone)]
pub struct DiagnosticsComputed {
    pub report: crate::views::diagnostics::DiagnosticsReport,
    pub data_tab_conflicts: Vec<(String, Vec<String>)>,
    pub missing_store_mod_count: usize,
}

/// What happened when the loader tried to (re)load the active profile.
#[derive(Debug, Clone)]
pub enum ProfileLoadOutcome {
    /// No active profile — clear `loaded_profile` and `mod_id_filter_keys`.
    Cleared,
    /// Profile loaded fresh from the DB.
    Loaded {
        profile: Box<modde_core::Profile>,
        experiment_depth: usize,
        current_fingerprint: Option<modde_core::save::SaveFingerprint>,
        mod_id_filter_keys: Vec<String>,
    },
    /// `pm.load` failed — leave the previously-loaded profile (and its derived
    /// fields) untouched, mirroring the old `reload_profile` behavior on a load
    /// error.
    KeepPrevious,
}

/// Inputs for [`super::model::load_profile_context`], built on the iced thread
/// from `&Modde` and moved into the blocking loader.
pub(super) struct ProfileContextRequest {
    pub(super) settings: modde_core::settings::AppSettings,
    pub(super) selected_game: Option<String>,
    pub(super) active_profile: Option<String>,
    /// `true` for game switches: recompute the active profile from the DB
    /// (active pointer, then first listed profile) instead of trusting
    /// `active_profile`.
    pub(super) recompute_active: bool,
    /// The currently-loaded profile, used to recompute data-tab conflicts when
    /// the load fails ([`ProfileLoadOutcome::KeepPrevious`]).
    pub(super) fallback_profile: Option<modde_core::Profile>,
    /// `Some` to fold a tool reload into the same off-thread job; `None` clears
    /// the tool state on apply.
    pub(super) tool_request: Option<ToolLoadRequest>,
    pub(super) rerun_diagnostics: bool,
}

/// Off-thread-computed data-tab conflict result — the lightweight refresh that
/// `Message::DataTabConflictsLoaded` applies when the Data tab is opened.
#[derive(Debug, Clone)]
pub struct DataTabConflicts {
    pub conflicts: Vec<(String, Vec<String>)>,
    pub missing_store_mod_count: usize,
}
