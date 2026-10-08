use std::sync::atomic::{AtomicU64, Ordering};

use crate::app::{Message, ReorderDirection, SidebarGroup, View, WabbajackTab};
use crate::views::browse_nexus::BrowseTab;
use iced::Element;
use iced::widget::{Button, mouse_area};
use modde_core::NexusModId;
use modde_core::filter::FilterKind;

static NEXT_BUTTON_HOVER_ID: AtomicU64 = AtomicU64::new(1);

/// Trait-enforced hover copy for GUI button actions.
pub trait ButtonActionDescription {
    fn button_description(&self) -> &'static str;
}

#[derive(Debug, Clone)]
pub enum ButtonAction {
    SwitchView(View),
    ToggleSidebarGroup(SidebarGroup),
    DeleteProfile(String),
    OpenNewProfileDialog,
    ForkProfile {
        source: String,
        new_name: String,
    },
    RollbackExperiment,
    CommitExperiment,
    TryProfile,
    OpenModPage,
    ModGalleryNext,
    ModEndorseToggle,
    ModTrackToggle,
    RestoreSaveSnapshot(String),
    AddMod,
    RemoveMod(usize),
    Deploy,
    ToggleFilterMode,
    CycleFilter(FilterKind),
    ClearFilters,
    ToggleCompactModList,
    ToggleSeparator(Option<i64>),
    ReorderMod {
        mod_id: String,
        direction: ReorderDirection,
    },
    SelectMod(usize),
    SearchCollections(String),
    InstallCollection {
        slug: String,
        version: String,
    },
    BrowseTabSwitched(BrowseTab),
    BrowseInstallMod {
        game_domain: String,
        mod_id: NexusModId,
    },
    LoadWabbajackCatalog,
    WabbajackTabChanged(WabbajackTab),
    OpenWabbajackFile,
    WabbajackDownloadSelected,
    WabbajackCheckReadiness,
    WabbajackImportArchives,
    WabbajackStartInstall,
    WabbajackSelectEntry(usize),
    WabbajackOpenUrl(String),
    WabbajackGenerateHmSnippet,
    WabbajackCopyHmSnippet,
    WabbajackSaveHmSnippet,
    FomodCancel,
    FomodUndo,
    FomodBack,
    FomodNext,
    PauseDownload(usize),
    ResumeDownload(usize),
    CancelDownload(usize),
    RunDiagnostics,
    AnalyzeCrashLog,
    ClearOverwrite,
    MoveOverwriteToMod(String),
    LoadSaveHistory,
    ValidateNexusKey,
    ToggleNexusApiKeyVisibility,
    ReplaceNexusApiKey,
    RemoveNexusConfigKey,
    BrowseGamePath,
    BrowseDownloadDir,
    CreateStockSnapshot,
    VerifyStockSnapshot,
    RefreshTools,
    SelectToolTab(String),
    UpdateToolSetting {
        tool_id: String,
        key: String,
        value: serde_json::Value,
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
    RefreshOptiScalerReleases,
    InstallOptiScalerRelease,
    RefreshProtonVersions,
    InstallProtonVersion,
    OpenExecutableEditor,
    RefreshExecutables,
    ClearExecutableDraft,
    EditExecutable(String),
    SaveExecutable,
    RemoveExecutable(String),
    RunExecutable(String),
    BrowseExecutablePath,
    BrowseExecutableWorkingDir,
    RefreshLibrary,
    OpenLibraryLogs,
    ExportLibraryDiagnostics,
    FavoriteLibraryEntry(String),
    InstallLibraryEntry(String),
    SaveLibraryLaunch,
    EditLibraryLaunch,
    BrowseLibraryLaunch(crate::views::library::LaunchField),
    ImportLibraryLaunch,
    ExportLibraryLaunch,
    InstallLibraryHook,
    AdoptLibrarySaves,
    FinishLibrarySession,
    SkipLibraryAnalysis,
    SyncLibrarySteam,
    SelectLibraryEntry(String),
    PlayLibraryEntry {
        id: String,
        hd: bool,
    },
    ManageLibraryGame(String),
    WindowMinimize,
    WindowToggleMaximize,
    WindowClose,
    CancelNewProfileDialog,
    SubmitNewProfileDialog,
    GamePathDialogBrowse,
    CancelGamePathDialog,
    OpenAddCustomGame,
    BrowseAddCustomGameInstallPath,
    AddCustomGameSubmit,
    AddCustomGameCancel,
    OpenManageCustomGames,
    CloseManageCustomGames,
    RemoveCustomGame(String),
    OpenUpdateReleasePage,
    DismissUpdateBanner,
}

#[path = "action_button_parts/conversions.rs"]
mod conversions;
#[path = "action_button_parts/descriptions.rs"]
mod descriptions;
#[path = "action_button_parts/widgets.rs"]
mod widgets;

pub use self::widgets::DescribedButtonExt;
