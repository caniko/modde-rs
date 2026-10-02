use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use iced::{Task, window};
use modde_core::filter::{FilterCriterion, FilterKind, FilterMode};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;
use modde_core::settings::AppSettings;
use modde_sources::wabbajack::installer::InstallProgress;
use smallvec::SmallVec;

use super::install_ops::{
    assess_wabbajack_readiness_for_ui, download_wabbajack_source, format_anyhow_error,
    format_install_progress, import_wabbajack_archives_for_ui, run_browse_install,
    run_wabbajack_install_for_ui_stream, slugify_profile_name,
};
use super::profile_ops::{
    add_mod_to_profile, create_profile, delete_profile, fork_profile, remove_mod_from_profile,
    reorder_mod, run_experiment_write, set_mod_lock, toggle_mod_enabled,
};
use super::state::{empty_to_none, prefill_wabbajack_game_dir};
use super::tool_ops::{
    apply_tool_for_game, deactivate_optiscaler_for_game, executable_draft_to_row,
    install_selected_proton_version, install_selected_tool_release, load_proton_versions,
    load_tool_releases, proton_version_options_for_ui, remove_executable_for_game,
    revert_tool_for_game, run_saved_executable_for_game, save_executable_for_game,
};
use super::tool_settings::{
    adopt_optiscaler_for_game, reset_optiscaler_config_for_game, restore_tool_settings_for_game,
    save_optiscaler_release_selection_for_game, save_proton_selected_version_for_game,
    save_tool_setting_for_game, set_tool_options, toggle_tool_for_game, tool_options,
};
use super::{
    AddCustomGameDraftField, AddCustomGameState, BUTTON_HOVER_TOAST_DELAY, ButtonHoverToast,
    ButtonHoverToastState, ExecutableDraft, ExecutableDraftField, ExperimentWriteKind,
    FOMODWizardState, Message, Modde, NexusAuthStatus, ProfileWriteKind, SidebarGroup, View,
    WabbajackInstallEvent, WabbajackInstallerState, detected_game_ids, resize_thumbnail_bytes,
};

fn apply_wabbajack_progress_state(
    state: &mut WabbajackInstallerState,
    progress: &InstallProgress,
    line: String,
) {
    state.status = line;
    match progress {
        InstallProgress::Starting { total_downloads } => {
            state.install_phase = "Starting".to_string();
            state.install_current_item = format!("{total_downloads} download(s) queued");
            state.progress = 0.0;
        }
        InstallProgress::Downloading { name, bytes, total } => {
            state.install_phase = "Downloading".to_string();
            state.install_current_item = name.clone();
            if *total > 0 {
                state.progress = (*bytes as f32 / *total as f32).clamp(0.0, 1.0);
            }
        }
        InstallProgress::DownloadComplete { name } => {
            state.install_phase = "Downloaded".to_string();
            state.install_current_item = name.clone();
        }
        InstallProgress::Verifying { name } => {
            state.install_phase = "Verifying".to_string();
            state.install_current_item = name.clone();
        }
        InstallProgress::Applying {
            directive_index,
            total,
        } => {
            state.install_phase = "Applying".to_string();
            state.install_current_item = format!("directive {}/{}", directive_index + 1, total);
            if *total > 0 {
                state.progress = ((*directive_index + 1) as f32 / *total as f32).clamp(0.0, 1.0);
            }
        }
        InstallProgress::Patching { name } => {
            state.install_phase = "Patching".to_string();
            state.install_current_item = name.clone();
        }
        InstallProgress::CreatingBSA { name } => {
            state.install_phase = "Creating BSA".to_string();
            state.install_current_item = name.clone();
        }
        InstallProgress::LauncherConfigured { .. } => {
            state.install_phase = "Launcher".to_string();
            state.install_current_item.clear();
        }
        InstallProgress::InlineFile { name } => {
            state.install_phase = "Writing inline file".to_string();
            state.install_current_item = name.clone();
        }
        InstallProgress::StagingAdopted { .. } => {
            state.install_phase = "Resuming staging".to_string();
            state.install_current_item.clear();
        }
        InstallProgress::Complete => {
            state.install_phase = "Complete".to_string();
            state.install_current_item.clear();
            state.progress = 1.0;
        }
        InstallProgress::Failed { .. } => {
            state.install_phase = "Failed".to_string();
            state.install_current_item.clear();
        }
    }
}

impl Modde {
    pub(super) fn new() -> (Self, Task<Message>) {
        let settings = AppSettings::load();
        let theme_name = if settings.theme.is_empty() {
            "Dark".to_string()
        } else {
            settings.theme.clone()
        };
        let selected_game = settings.selected_game.clone();

        // TODO(phase-02): surface startup database failures as a Task error state.
        let db = crate::app::block_on(modde_core::db::ModdeDb::open())
            .expect("failed to open modde database at startup");
        let all_profiles =
            crate::app::block_on(ProfileManager::with_db(db.clone()).list()).unwrap_or_default();

        let available_games: SmallVec<[(String, String); 8]> = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        let detected_games = detected_game_ids(&settings, available_games.as_slice());

        let mut app = Self {
            db,
            active_view: View::Library,
            active_profile: None,
            profiles: Vec::new(),
            status_message: "Ready".to_string(),
            button_hover_toast: ButtonHoverToastState::default(),
            pending_tools_load_status_message: None,
            settings,
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
            theme_name,
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
            available_games,
            detected_games,
            selected_game,
            stock_snapshot_exists: false,
            window_id: window::Id::unique(),
            collapsed_categories: HashSet::new(),
            mod_categories: vec![(None, "Uncategorized".to_string())],
            data_tab_state: Default::default(),
            data_tab_conflicts: Vec::new(),
            diagnostics_state: Default::default(),
            crash_log_path_draft: String::new(),
            tool_state: Default::default(),
            library: Default::default(),
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
        };
        app.refresh_nexus_api_key_state();

        // Auto-detect: if no game is selected but profiles exist, pick the first profile's game
        if app.selected_game.is_none()
            && let Some(first) = all_profiles.first()
        {
            app.selected_game = Some(first.game_id.to_string());
            app.settings.selected_game = Some(first.game_id.to_string());
        }

        // Kick off the first profile/game-context load asynchronously. The
        // heavy multi-query reload (profile + conflicts + tools) resolves off
        // the render thread via `Message::ProfileContextLoaded`, so the first
        // paint is immediate. The one-time `list()` above is only used for the
        // auto-detect above and is not stored in `app.profiles` (which fills in
        // from the async load).
        let initial_context_task = if let Some(game_id) = app.selected_game.clone() {
            app.accept_game_selection(game_id, None)
        } else {
            Task::none()
        };

        let initial_library_task = app.update(Message::LibraryRefresh);
        (
            app,
            Task::batch([
                window::oldest().map(Message::GotWindowId),
                Task::perform(
                    async {
                        modde_core::update_check::check_latest()
                            .await
                            .map_err(|error| error.to_string())
                    },
                    Message::UpdateCheckLoaded,
                ),
                initial_context_task,
                initial_library_task,
            ]),
        )
    }

    pub(super) fn title(&self) -> String {
        "modde".to_string()
    }
}

#[path = "update_parts/browse_nexus.rs"]
mod browse_nexus;
#[path = "update_parts/collections.rs"]
mod collections;
#[path = "update_parts/downloads.rs"]
mod downloads;
#[path = "update_parts/experiments.rs"]
mod experiments;
#[path = "update_parts/fomod.rs"]
mod fomod;
#[path = "update_parts/game_selection.rs"]
mod game_selection;
#[path = "update_parts/library.rs"]
mod library;
#[path = "update_parts/load_order.rs"]
mod load_order;
#[path = "update_parts/mod_list.rs"]
mod mod_list;
#[path = "update_parts/navigation.rs"]
mod navigation;
#[path = "update_parts/profile_dialog.rs"]
mod profile_dialog;
#[path = "update_parts/saves_tools_misc.rs"]
mod saves_tools_misc;
#[path = "update_parts/settings.rs"]
mod settings;
#[path = "update_parts/stock_game.rs"]
mod stock_game;
#[path = "update_parts/system.rs"]
mod system;
#[path = "update_parts/wabbajack.rs"]
mod wabbajack;
#[path = "update_parts/window_controls.rs"]
mod window_controls;

impl Modde {
    pub(super) fn update(&mut self, message: Message) -> Task<Message> {
        if !message.mutates_game() || self.library.coordination.mutation_dispatch {
            return self.update_inner(message);
        }
        if !self.library.launching.is_empty() {
            self.status_message =
                "Finish the game session before changing profiles, mods, runners or saves".into();
            return Task::none();
        }
        let guard = match modde_core::library::mutation_lock() {
            Ok(guard) => guard,
            Err(error) => {
                self.status_message = error.to_string();
                return Task::none();
            }
        };
        match modde_core::library::PendingSession::load_blocking() {
            Ok(None) => {}
            Ok(Some(_)) => {
                self.status_message =
                    "Finish or recover the Library session before changing game state".into();
                return Task::none();
            }
            Err(error) => {
                self.status_message = error.to_string();
                return Task::none();
            }
        }
        self.library.coordination.mutation_dispatch = true;
        let task = self.update_inner(message);
        self.library.coordination.mutation_dispatch = false;
        // Keep the lease through all work emitted by this request. This also
        // prevents a CLI launch racing an already-running GUI deployment.
        task.chain(Task::perform(
            async move {
                drop(guard);
            },
            |()| Message::Noop,
        ))
    }

    fn update_inner(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ExternalRefresh
            | Message::ProfileContextLoaded { .. }
            | Message::DataTabConflictsLoaded { .. }
            | Message::UpdateCheckLoaded(_)
            | Message::OpenUpdateReleasePage
            | Message::DismissUpdateBanner => self.handle_system_update(message),

            Message::SwitchView(_)
            | Message::ToggleSidebarGroup(_)
            | Message::SwitchProfile(_)
            | Message::CreateProfile { .. }
            | Message::DeleteProfile(_)
            | Message::ForkProfile { .. }
            | Message::ProfileWriteDone { .. } => self.handle_navigation_update(message),

            Message::OpenNewProfileDialog
            | Message::NewProfileNameChanged(_)
            | Message::CancelNewProfileDialog
            | Message::SubmitNewProfileDialog => self.handle_profile_dialog_update(message),

            Message::SelectGame(_)
            | Message::GamePathDialogBrowse
            | Message::GamePathDialogPathSelected { .. }
            | Message::CancelGamePathDialog
            | Message::OpenAddCustomGame
            | Message::BrowseAddCustomGameInstallPath
            | Message::AddCustomGameFieldChanged { .. }
            | Message::AddCustomGameInstallPathPicked(_)
            | Message::AddCustomGameSubmit
            | Message::AddCustomGameCancel
            | Message::OpenManageCustomGames
            | Message::CloseManageCustomGames
            | Message::RemoveCustomGame(_) => self.handle_game_selection_update(message),

            Message::GotWindowId(_)
            | Message::TitleBarDrag
            | Message::WindowMinimize
            | Message::WindowToggleMaximize
            | Message::WindowClose => self.handle_window_controls_update(message),

            Message::ToggleMod { .. }
            | Message::FilterChanged(_)
            | Message::ToggleFilterMode
            | Message::CycleFilter(_)
            | Message::ClearFilters
            | Message::ToggleCompactModList
            | Message::ToggleSeparator(_)
            | Message::AddMod
            | Message::AddModFromPath(_)
            | Message::RemoveMod(_)
            | Message::SelectMod(_)
            | Message::ModDetailsLoaded { .. }
            | Message::ModGalleryLoaded { .. }
            | Message::ModThumbnailLoaded { .. }
            | Message::ModGalleryNext
            | Message::OpenModPage
            | Message::ModEndorseToggle
            | Message::ModEndorseResult { .. }
            | Message::ModTrackToggle
            | Message::ModTrackResult { .. }
            | Message::ModTrackedSetLoaded { .. }
            | Message::Deploy
            | Message::DeployComplete(_) => self.handle_mod_list_update(message),

            Message::LibraryFilterChanged(_)
            | Message::LibraryCategoryChanged(_)
            | Message::LibraryFavorite(_)
            | Message::LibraryInstall(_)
            | Message::LibraryLaunchFieldChanged(..)
            | Message::LibrarySandboxChanged(_)
            | Message::LibraryNetworkChanged(_)
            | Message::LibraryActiveProfileChanged(_)
            | Message::LibrarySaveLaunch
            | Message::LibraryEditLaunch
            | Message::LibraryBrowseLaunch(_)
            | Message::LibraryPathPicked { .. }
            | Message::LibraryImportLaunch
            | Message::LibraryExportLaunch
            | Message::LibraryLaunchImported { .. }
            | Message::LibraryInstallHook
            | Message::LibraryHookInstalled { .. }
            | Message::LibraryHookChanged(_)
            | Message::LibrarySteamCloudChanged(_)
            | Message::LibraryProfilesLoaded { .. }
            | Message::LibrarySessionTick
            | Message::LibrarySessionLoaded { .. }
            | Message::LibraryAdoptSaves
            | Message::LibraryPreferenceSaved(_)
            | Message::LibraryFinishSession
            | Message::LibrarySkipAnalysis
            | Message::LibrarySteamIdChanged(_)
            | Message::LibrarySyncSteam
            | Message::LibrarySteamSynced(_)
            | Message::LibrarySelectEntry(_)
            | Message::LibraryRefresh
            | Message::LibraryOpenLogs
            | Message::LibraryExportDiagnostics
            | Message::LibraryDiagnosticsDone(_)
            | Message::LibraryLoaded { .. }
            | Message::LibraryPlay { .. }
            | Message::LibraryPlayComplete { .. }
            | Message::LibraryManageGame(_) => self.handle_library_update(message),

            Message::ReorderMod { .. } | Message::LockMod { .. } | Message::UnlockMod { .. } => {
                self.handle_load_order_update(message)
            }

            Message::SearchCollections(_) | Message::InstallCollection { .. } => {
                self.handle_collections_update(message)
            }

            Message::BrowseTabSwitched(_)
            | Message::BrowseGameChanged(_)
            | Message::BrowseSearchChanged(_)
            | Message::BrowseSearchSubmit
            | Message::BrowseModsLoaded(_)
            | Message::BrowseCollectionsLoaded(_)
            | Message::BrowseInstallMod { .. }
            | Message::BrowseInstallResult { .. } => self.handle_browse_nexus_update(message),

            Message::LoadWabbajackCatalog
            | Message::WabbajackCatalogLoaded(_)
            | Message::WabbajackTabChanged(_)
            | Message::WabbajackSearchChanged(_)
            | Message::WabbajackGameFilterChanged(_)
            | Message::WabbajackToggleOfficialOnly(_)
            | Message::WabbajackToggleNsfw(_)
            | Message::WabbajackToggleDown(_)
            | Message::WabbajackSelectEntry(_)
            | Message::WabbajackManualSourceChanged(_)
            | Message::WabbajackHmProfileChanged(_)
            | Message::WabbajackHmGameChanged(_)
            | Message::WabbajackHmGameDirChanged(_)
            | Message::WabbajackDownloadSelected
            | Message::WabbajackDownloadComplete(_)
            | Message::WabbajackCheckReadiness
            | Message::WabbajackReadinessLoaded(_)
            | Message::WabbajackImportArchives
            | Message::WabbajackArchivesImported(_)
            | Message::WabbajackGenerateHmSnippet
            | Message::WabbajackHmSnippetGenerated(_)
            | Message::WabbajackCopyHmSnippet
            | Message::WabbajackSaveHmSnippet
            | Message::WabbajackHmSnippetSaved(_)
            | Message::WabbajackOpenUrl(_)
            | Message::OpenWabbajackFile
            | Message::WabbajackFileSelected(_)
            | Message::WabbajackProgress(_)
            | Message::WabbajackStartInstall
            | Message::WabbajackInstallEvent(_)
            | Message::WabbajackInstallComplete(_)
            | Message::WabbajackLog(_) => self.handle_wabbajack_update(message),

            Message::StartFOMOD { .. }
            | Message::FOMODChoice { .. }
            | Message::FOMODNext
            | Message::FOMODBack
            | Message::FOMODCancel
            | Message::FOMODUndo
            | Message::FOMODInstallComplete(_) => self.handle_fomod_update(message),

            Message::DownloadProgress { .. }
            | Message::DownloadComplete { .. }
            | Message::DownloadFailed { .. } => self.handle_downloads_update(message),

            Message::SetNexusApiKeyDraft(_)
            | Message::ToggleNexusApiKeyVisibility
            | Message::ReplaceNexusApiKey
            | Message::RemoveNexusConfigKey
            | Message::SetGamePath { .. }
            | Message::SetDownloadDir(_)
            | Message::BrowseGamePath
            | Message::BrowseDownloadDir
            | Message::SetTheme(_)
            | Message::ValidateNexusKey
            | Message::NexusKeyValidated(_) => self.handle_settings_update(message),

            Message::CreateStockSnapshot
            | Message::StockSnapshotCreated(_)
            | Message::VerifyStockSnapshot
            | Message::StockVerifyResult(_) => self.handle_stock_game_update(message),

            Message::TryProfile
            | Message::RollbackExperiment
            | Message::CommitExperiment
            | Message::ExperimentWriteDone { .. } => self.handle_experiments_update(message),

            _ => self.handle_saves_tools_misc_update(message),
        }
    }
}

fn deploy_profile_blocking(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    game_id: GameId,
) -> Result<String, String> {
    let pm = ProfileManager::with_db(db);
    let profile =
        crate::app::block_on(pm.load(&profile_name, Some(&game_id))).map_err(|e| e.to_string())?;
    let resolved = modde_core::resolver::resolve(&profile).map_err(|e| e.to_string())?;
    let game_plugin = modde_games::resolve_game_plugin(game_id.as_str())
        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
    let context = crate::app::block_on(modde_games::library::context::for_game(
        &AppSettings::load(),
        game_id.as_str(),
        pm.db(),
    ))
    .map_err(|error| error.to_string())?;
    let install_path = context
        .game
        .install_path
        .ok_or_else(|| "installation missing".to_string())?;
    let staging_dir = ProfileManager::staging_dir(&profile.name);
    game_plugin
        .deploy_to_install_at(&staging_dir, &install_path, context.prefix.as_deref())
        .map_err(|e| e.to_string())?;
    game_plugin
        .post_deploy_at(&install_path, context.prefix.as_deref())
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "Deployed {} mod(s) for {}",
        resolved.order.len(),
        game_id
    ))
}
