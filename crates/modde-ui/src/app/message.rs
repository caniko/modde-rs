#![allow(clippy::wildcard_imports)]
use super::*;

// ─── Application Messages ────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum Message {
    /// External process (typically the CLI) notified the GUI that the
    /// profile DB has changed. Triggers a profile reload.
    ExternalRefresh,

    /// Async result of `load_profile_context` — the off-thread profile +
    /// data-tab + tool reload. `generation` guards against stale loads
    /// clobbering newer state (see `Modde::context_generation`).
    ProfileContextLoaded {
        generation: u64,
        result: Result<ProfileContextSnapshot, String>,
    },
    /// Async result of the lightweight data-tab conflict refresh fired when the
    /// Data tab is opened (see `Modde::data_tab_generation`).
    DataTabConflictsLoaded {
        generation: u64,
        result: Result<DataTabConflicts, String>,
    },

    // Navigation
    SwitchView(View),
    ToggleSidebarGroup(SidebarGroup),
    SwitchProfile(String),
    CreateProfile {
        name: String,
        game_id: String,
    },
    DeleteProfile(String),
    ForkProfile {
        source: String,
        new_name: String,
    },
    ProfileWriteDone {
        generation: u64,
        kind: ProfileWriteKind,
        result: Result<ProfileWriteOutcome, String>,
    },

    // Profile dialog
    OpenNewProfileDialog,
    NewProfileNameChanged(String),
    CancelNewProfileDialog,
    SubmitNewProfileDialog,

    // Game selection
    SelectGame(String),
    GamePathDialogBrowse,
    GamePathDialogPathSelected {
        game_id: String,
        path: PathBuf,
    },
    CancelGamePathDialog,
    OpenAddCustomGame,
    BrowseAddCustomGameInstallPath,
    AddCustomGameFieldChanged {
        field: AddCustomGameDraftField,
        value: String,
    },
    AddCustomGameInstallPathPicked(PathBuf),
    AddCustomGameSubmit,
    AddCustomGameCancel,
    OpenManageCustomGames,
    CloseManageCustomGames,
    RemoveCustomGame(String),

    // Library catalogue
    LibraryFilterChanged(String),
    LibraryCategoryChanged(crate::views::library::LibraryFilter),
    LibraryFavorite(String),
    LibraryInstall(String),
    LibraryLaunchFieldChanged(crate::views::library::LaunchField, String),
    LibrarySandboxChanged(bool),
    LibraryNetworkChanged(bool),
    LibraryActiveProfileChanged(bool),
    LibrarySaveLaunch,
    LibraryEditLaunch,
    LibraryBrowseLaunch(crate::views::library::LaunchField),
    LibraryPathPicked { id: String, revision: u64, field: crate::views::library::LaunchField, path: Option<PathBuf> },
    LibraryImportLaunch,
    LibraryExportLaunch,
    LibraryLaunchImported { id: String, revision: u64, result: Result<Option<modde_core::library::LaunchSettings>, String> },
    LibraryInstallHook,
    LibraryHookInstalled { id: String, revision: u64, result: Result<String, String> },
    LibrarySessionTick,
    LibrarySessionLoaded { revision: u64, result: Result<Option<modde_core::library::PendingSession>, String> },
    LibraryHookChanged(bool),
    LibraryProfilesLoaded { id: String, result: Result<Vec<String>, String> },
    LibraryAdoptSaves,
    LibraryPreferenceSaved(Result<(), String>),
    LibraryFinishSession,
    LibrarySkipAnalysis,
    LibrarySteamIdChanged(String),
    LibrarySyncSteam,
    LibrarySteamSynced(Result<usize, String>),
    LibrarySelectEntry(String),
    LibraryRefresh,
    LibraryLoaded {
        generation: u64,
        session_revision: u64,
        result: Result<LibraryLoadResult, String>,
    },
    LibraryPlay {
        id: String,
        hd: bool,
    },
    LibraryPlayComplete {
        id: String,
        hd: bool,
        result: Result<String, String>,
    },
    LibraryManageGame(String),

    // Window controls (custom title bar)
    GotWindowId(Option<window::Id>),
    TitleBarDrag,
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,

    // Mod list
    ToggleMod {
        mod_id: String,
        enabled: bool,
    },
    FilterChanged(String),
    AddMod,
    AddModFromPath(PathBuf),
    RemoveMod(usize),
    SelectMod(usize),
    /// Initial Nexus v1 `get_mod` response for the selected mod. Carries
    /// `nexus_mod_id` so stale responses (from a previous selection) are
    /// discarded when they race a newer click.
    ModDetailsLoaded {
        nexus_mod_id: modde_core::NexusModId,
        result: Result<modde_sources::nexus::api::NexusMod, String>,
    },
    /// Gallery image URL list returned by the v2 GraphQL endpoint.
    ModGalleryLoaded {
        nexus_mod_id: modde_core::NexusModId,
        urls: Vec<String>,
    },
    /// Image bytes downloaded for a specific gallery slot. Guarded by both
    /// `nexus_mod_id` and `gallery_index` so clicking through the gallery
    /// rapidly doesn't let an old image overwrite a newer one.
    ModThumbnailLoaded {
        nexus_mod_id: modde_core::NexusModId,
        gallery_index: usize,
        bytes: Vec<u8>,
    },
    /// User clicked the thumbnail — advance to the next image in the gallery.
    ModGalleryNext,
    /// User clicked the "Open in Nexus" link.
    OpenModPage,
    Deploy,
    DeployComplete(Result<String, String>),

    // Load order
    /// Move a specific mod up or down by one position. Mod-id-based (not
    /// index-based) because the `load_order` view and `mod_list` view operate
    /// on different index spaces — `resolved_order` vs. `profile.mods` —
    /// and an index-based message was latently unsound. Also lets the
    /// handler consult the per-mod lock without an index round-trip.
    ReorderMod {
        mod_id: String,
        direction: ReorderDirection,
    },
    /// Pin an individual mod in place (per-mod lock).
    LockMod {
        mod_id: String,
    },
    /// Release an individual mod's per-mod pin.
    UnlockMod {
        mod_id: String,
    },

    // Collections
    SearchCollections(String),
    InstallCollection {
        slug: String,
        version: String,
    },

    // ── Browse Nexus (Phase 6) ───────────────────────────────
    /// Switch the active browse tab. Fires a task to load the feed
    /// for the new tab if its contents are empty.
    BrowseTabSwitched(crate::views::browse_nexus::BrowseTab),
    /// Switch the Nexus browser to a different supported game.
    BrowseGameChanged(Option<String>),
    /// Live search box keystroke.
    BrowseSearchChanged(String),
    /// Submit the search (Enter pressed). Runs the appropriate query
    /// depending on the active tab.
    BrowseSearchSubmit,
    /// Async result of a mods feed fetch.
    BrowseModsLoaded(Result<Vec<modde_sources::nexus::graphql::GqlModTile>, String>),
    /// Async result of a collections feed fetch.
    BrowseCollectionsLoaded(Result<Vec<modde_sources::nexus::graphql::GqlCollectionTile>, String>),
    /// User clicked "Install" on a mod tile. Runs the install
    /// pipeline via `modde_sources::nexus::install::install_single_mod`.
    BrowseInstallMod {
        game_domain: String,
        mod_id: modde_core::NexusModId,
    },
    /// Async completion of a browse install. The `Ok` payload is a
    /// short human-readable status message; `Err` is an error string.
    BrowseInstallResult {
        download_key: String,
        result: Result<String, String>,
    },

    // Wabbajack
    LoadWabbajackCatalog,
    WabbajackCatalogLoaded(
        Result<Vec<modde_sources::wabbajack::catalog::WabbajackCatalogEntry>, String>,
    ),
    WabbajackTabChanged(WabbajackTab),
    WabbajackSearchChanged(String),
    WabbajackGameFilterChanged(Option<String>),
    WabbajackToggleOfficialOnly(bool),
    WabbajackToggleNsfw(bool),
    WabbajackToggleDown(bool),
    WabbajackSelectEntry(usize),
    WabbajackManualSourceChanged(String),
    WabbajackHmProfileChanged(String),
    WabbajackHmGameChanged(String),
    WabbajackHmGameDirChanged(String),
    WabbajackDownloadSelected,
    WabbajackDownloadComplete(Result<PathBuf, String>),
    WabbajackCheckReadiness,
    WabbajackReadinessLoaded(
        Result<modde_sources::wabbajack::readiness::WabbajackReadinessReport, String>,
    ),
    WabbajackImportArchives,
    WabbajackArchivesImported(
        Result<Vec<modde_sources::wabbajack::import::ArchiveImportResult>, String>,
    ),
    WabbajackGenerateHmSnippet,
    WabbajackHmSnippetGenerated(Result<String, String>),
    WabbajackCopyHmSnippet,
    WabbajackSaveHmSnippet,
    WabbajackHmSnippetSaved(Result<PathBuf, String>),
    WabbajackOpenUrl(String),
    OpenWabbajackFile,
    WabbajackFileSelected(PathBuf),
    WabbajackProgress(f32),
    WabbajackStartInstall,
    WabbajackInstallEvent(WabbajackInstallEvent),
    WabbajackInstallComplete(Result<(String, Vec<String>), String>),
    WabbajackLog(String),

    // FOMOD
    StartFOMOD {
        mod_path: PathBuf,
        dest_path: PathBuf,
    },
    FOMODChoice {
        step: usize,
        group: usize,
        option: usize,
        selected: bool,
    },
    FOMODNext,
    FOMODBack,
    FOMODCancel,
    FOMODUndo,
    FOMODInstallComplete(Result<(), String>),

    // Downloads
    DownloadProgress {
        id: String,
        bytes: u64,
        total: u64,
    },
    DownloadComplete {
        id: String,
    },
    DownloadFailed {
        id: String,
        error: String,
    },

    // Settings
    SetNexusApiKeyDraft(String),
    ToggleNexusApiKeyVisibility,
    ReplaceNexusApiKey,
    RemoveNexusConfigKey,
    SetGamePath {
        game_id: String,
        path: PathBuf,
    },
    SetDownloadDir(PathBuf),
    BrowseGamePath,
    BrowseDownloadDir,
    SetTheme(String),
    ValidateNexusKey,
    NexusKeyValidated(Result<(String, bool), String>),

    // Stock game
    CreateStockSnapshot,
    StockSnapshotCreated(Result<String, String>),
    VerifyStockSnapshot,
    StockVerifyResult(Result<String, String>),

    // Experiments
    TryProfile,
    RollbackExperiment,
    CommitExperiment,
    ExperimentWriteDone {
        generation: u64,
        kind: ExperimentWriteKind,
        result: Result<ExperimentWriteOutcome, String>,
    },

    // Saves
    LoadSaveHistory,
    RestoreSaveSnapshot(String),
    SelectSaveSnapshot(String),

    // Mod list – category separators
    ToggleSeparator(Option<i64>),

    // Data tab
    DataTabFilterChanged(String),
    DataTabToggleConflicts(bool),

    // Diagnostics
    RunDiagnostics,
    CrashLogPathChanged(String),
    AnalyzeCrashLog,
    DiagnosticsComputed {
        generation: u64,
        result: Result<DiagnosticsComputed, String>,
    },

    // Tools
    LoadTools,
    ToolsLoaded {
        generation: u64,
        result: Result<ToolLoadSnapshot, String>,
    },
    RefreshTools,
    LoadExecutables,
    RefreshExecutables,
    ExecutablesLoaded {
        generation: u64,
        result: Result<Vec<ExecutableUiEntry>, String>,
    },
    SelectToolTab(String),
    UpdateToolSetting {
        tool_id: String,
        key: String,
        value: serde_json::Value,
    },
    ToggleTool {
        tool_id: String,
        enabled: bool,
    },
    ToolSettingWritten {
        tool_id: String,
        result: Result<ToolSettingWriteResult, String>,
    },
    ToggleToolAdvancedSettings,
    ApplyTool(String),
    RevertTool(String),
    ActivateOptiScaler,
    DeactivateOptiScaler,
    AdoptOptiScaler,
    RestoreOptiScalerBackup,
    ResetOptiScalerConfig,
    RestoreToolSettings {
        tool_id: String,
        node_id: String,
    },
    ToolSettingsRestored {
        tool_id: String,
        result: Result<String, String>,
    },
    RefreshOptiScalerReleases,
    OptiScalerReleasesLoaded(Result<Vec<modde_games::tools::ToolReleaseSummary>, String>),
    InstallOptiScalerRelease,
    OptiScalerReleaseInstalled(Result<String, String>),
    RefreshProtonVersions,
    ProtonVersionsLoaded(Result<Vec<String>, String>),
    InstallProtonVersion,
    ProtonVersionInstalled(Result<String, String>),
    ToolApplied {
        tool_id: String,
        result: Result<ToolApplyResult, String>,
    },
    ToolReverted {
        tool_id: String,
        result: Result<ToolRevertResult, String>,
    },
    UpdateExecutableDraft {
        field: ExecutableDraftField,
        value: String,
    },
    OpenExecutableEditor,
    ClearExecutableDraft,
    EditExecutable(String),
    SaveExecutable,
    ExecutableSaved(Result<String, String>),
    RemoveExecutable(String),
    ExecutableRemoved {
        name: String,
        result: Result<String, String>,
    },
    RunExecutable(String),
    ExecutableRunComplete {
        name: String,
        result: Result<String, String>,
    },
    BrowseExecutablePath,
    ExecutablePathSelected(Option<PathBuf>),
    BrowseExecutableWorkingDir,
    ExecutableWorkingDirSelected(Option<PathBuf>),

    // Downloads
    PauseDownload(usize),
    ResumeDownload(usize),
    CancelDownload(usize),

    // Sidebar mod detail — Nexus interactions
    /// User clicked the endorse/abstain toggle button.
    ModEndorseToggle,
    /// Async result of an endorse or abstain call. Carries the target
    /// status the handler optimistically applied, so it can roll back if
    /// the request failed.
    ModEndorseResult {
        nexus_mod_id: modde_core::NexusModId,
        new_status: String,
        result: Result<(), String>,
    },
    /// User clicked the track/untrack toggle button.
    ModTrackToggle,
    /// Async result of a track or untrack call.
    ModTrackResult {
        nexus_mod_id: modde_core::NexusModId,
        new_tracked: bool,
        result: Result<(), String>,
    },
    /// Async result of the initial `get_tracked_mods` call fired alongside
    /// `get_mod` when a mod is selected.
    ModTrackedSetLoaded {
        nexus_mod_id: modde_core::NexusModId,
        is_tracked: bool,
    },

    // Overwrite management
    ClearOverwrite,
    MoveOverwriteToMod(String),

    // Mod list filter toolbar
    ToggleFilterMode,
    CycleFilter(FilterKind),
    ClearFilters,
    ToggleCompactModList,

    // Button hover help
    ButtonHoverStarted {
        id: u64,
        description: &'static str,
    },
    ButtonHoverElapsed {
        id: u64,
    },
    ButtonHoverEnded {
        id: u64,
    },

    // Misc
    Noop,
    UpdateCheckLoaded(Result<Option<modde_core::update_check::UpdateInfo>, String>),
    OpenUpdateReleasePage,
    DismissUpdateBanner,
}

impl Message {
    /// Requests that can change a live deployment, profile, runner or save set.
    /// Async completion messages remain deliverable while a session is open.
    pub(super) fn mutates_game(&self) -> bool {
        matches!(self,
            Self::CreateProfile { .. } | Self::DeleteProfile(_) | Self::ForkProfile { .. }
            | Self::SubmitNewProfileDialog | Self::AddCustomGameSubmit | Self::RemoveCustomGame(_)
            | Self::ToggleMod { .. } | Self::AddModFromPath(_) | Self::RemoveMod(_) | Self::Deploy
            | Self::ReorderMod { .. } | Self::LockMod { .. } | Self::UnlockMod { .. }
            | Self::InstallCollection { .. } | Self::BrowseInstallMod { .. }
            | Self::WabbajackStartInstall | Self::StartFOMOD { .. } | Self::FOMODNext | Self::FOMODUndo
            | Self::SetGamePath { .. } | Self::LibraryManageGame(_) | Self::CreateStockSnapshot | Self::TryProfile
            | Self::RollbackExperiment | Self::CommitExperiment | Self::RestoreSaveSnapshot(_)
            | Self::UpdateToolSetting { .. } | Self::ToggleTool { .. } | Self::ApplyTool(_)
            | Self::RevertTool(_) | Self::ActivateOptiScaler | Self::DeactivateOptiScaler
            | Self::AdoptOptiScaler | Self::RestoreOptiScalerBackup | Self::ResetOptiScalerConfig
            | Self::RestoreToolSettings { .. } | Self::InstallOptiScalerRelease | Self::InstallProtonVersion
            | Self::SaveExecutable | Self::RemoveExecutable(_) | Self::RunExecutable(_)
            | Self::ClearOverwrite | Self::MoveOverwriteToMod(_)
        )
    }
}
