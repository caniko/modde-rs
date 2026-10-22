#![allow(clippy::wildcard_imports)]
//! tasks model helpers.

use super::*;

impl Modde {
    pub(crate) fn refresh_data_tab_conflicts(&mut self) -> Task<Message> {
        let Some(profile) = self.loaded_profile.clone() else {
            self.data_tab_conflicts.clear();
            self.data_tab_state.missing_store_mod_count = 0;
            return Task::none();
        };
        self.data_tab_generation = self.data_tab_generation.wrapping_add(1);
        let generation = self.data_tab_generation;
        let db = self.db.clone();
        Task::perform(load_data_tab_conflicts(db, profile), move |result| {
            Message::DataTabConflictsLoaded { generation, result }
        })
    }

    pub(crate) fn start_diagnostics_load(&mut self) -> Task<Message> {
        let Some(profile) = self.loaded_profile.clone() else {
            self.set_status("Select a profile before running diagnostics".to_string());
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                "Select a profile before running diagnostics.".to_string(),
            );
            return Task::none();
        };
        self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
        let generation = self.diagnostics_generation;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Running;
        self.set_status("Running diagnostics...".to_string());
        Task::perform(load_diagnostics(self.db.clone(), profile), move |result| {
            Message::DiagnosticsComputed { generation, result }
        })
    }

    pub(crate) fn start_crash_log_analysis(&mut self) -> Task<Message> {
        let Some(profile) = self.loaded_profile.clone() else {
            self.set_status("Select a profile before analyzing crash logs".to_string());
            self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                "Select a profile before analyzing crash logs.".to_string(),
            );
            return Task::none();
        };
        let path = self.crash_log_path_draft.trim().to_string();
        if path.is_empty() {
            self.set_status("Enter a crash log path first".to_string());
            return Task::none();
        }
        self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
        let generation = self.diagnostics_generation;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Running;
        self.set_status("Analyzing crash log...".to_string());
        Task::perform(
            load_diagnostics_with_crash(self.db.clone(), profile, std::path::PathBuf::from(path)),
            move |result| Message::DiagnosticsComputed { generation, result },
        )
    }

    pub(crate) fn apply_diagnostics_computed(&mut self, computed: DiagnosticsComputed) {
        let diagnostic_count = computed.report.entries.len();
        let broken_count = computed.report.integrity.broken_symlinks.len();
        self.data_tab_state.missing_store_mod_count = computed.missing_store_mod_count;
        self.data_tab_conflicts = computed.data_tab_conflicts;
        self.diagnostics_state =
            crate::views::diagnostics::DiagnosticsState::Complete(computed.report);
        let message = if diagnostic_count == 0 && broken_count == 0 {
            "Diagnostics complete: no issues found".to_string()
        } else {
            format!(
                "Diagnostics complete: {diagnostic_count} issue(s), {broken_count} broken symlink(s)"
            )
        };
        self.set_status(message);
    }

    pub(crate) fn verify_staging_integrity(
        staging_dir: &std::path::Path,
    ) -> crate::views::diagnostics::IntegritySummary {
        let mut results = crate::views::diagnostics::IntegritySummary::default();
        if !staging_dir.exists() {
            return results;
        }

        fn walk(dir: &std::path::Path, results: &mut crate::views::diagnostics::IntegritySummary) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_dir() {
                        walk(&path, results);
                    } else if path.is_symlink() {
                        match std::fs::read_link(&path) {
                            Ok(target) if target.exists() => {
                                results.ok_count += 1;
                            }
                            _ => results.broken_symlinks.push(path),
                        }
                    } else {
                        results.ok_count += 1;
                    }
                }
            }
        }

        walk(staging_dir, &mut results);
        results
    }

    pub(crate) fn start_tools_load(&mut self) -> Task<Message> {
        let Some(request) = self.tool_load_request() else {
            self.tool_state.entries.clear();
            self.tool_state.active_tool_id = None;
            self.tool_state.game_label = None;
            self.tool_state.game_dir_configured = false;
            self.tool_state.loading = false;
            self.tool_state.load_error = None;
            self.set_status("Select a game before loading tools".to_string());
            return Task::none();
        };
        self.tool_state.load_generation = self.tool_state.load_generation.wrapping_add(1);
        let generation = self.tool_state.load_generation;
        self.tool_state.loading = true;
        self.tool_state.load_error = None;
        let db = self.db.clone();
        Task::perform(load_tools_state(db, request), move |result| {
            Message::ToolsLoaded { generation, result }
        })
    }

    pub(crate) fn tool_load_request(&self) -> Option<ToolLoadRequest> {
        let game_id = self.current_game_id()?.to_string();
        let display_name = self
            .available_games
            .iter()
            .find(|(id, _)| id == &game_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| game_id.clone());
        Some(ToolLoadRequest {
            game_id,
            display_name,
            configured_game_dir: self
                .settings
                .game_path(&GameId::from(self.current_game_id()?))
                .cloned(),
            optiscaler_releases: self.tool_state.optiscaler_releases.clone(),
            tool_option_catalog: self.tool_state.tool_option_catalog.clone(),
            previous_active_tool_id: self.tool_state.active_tool_id.clone(),
        })
    }

    /// Build a tool-load request for an explicit game id, rather than relying on
    /// `current_game_id()`. Used by `switch_game_context`, where `loaded_profile`
    /// has just been cleared so `current_game_id()` could not be trusted.
    pub(crate) fn tool_load_request_for(&self, game_id: &str) -> ToolLoadRequest {
        let display_name = self
            .available_games
            .iter()
            .find(|(id, _)| id == game_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| game_id.to_string());
        ToolLoadRequest {
            game_id: game_id.to_string(),
            display_name,
            configured_game_dir: self.settings.game_path(&GameId::from(game_id)).cloned(),
            optiscaler_releases: self.tool_state.optiscaler_releases.clone(),
            tool_option_catalog: self.tool_state.tool_option_catalog.clone(),
            previous_active_tool_id: self.tool_state.active_tool_id.clone(),
        }
    }

    pub(crate) fn apply_tool_snapshot(&mut self, snapshot: ToolLoadSnapshot) {
        self.tool_state.entries = snapshot.entries;
        self.tool_state.active_tool_id = snapshot.active_tool_id;
        self.tool_state.game_label = snapshot.game_label;
        self.tool_state.game_dir_configured = snapshot.game_dir_configured;
        self.tool_state.tool_option_catalog = snapshot.tool_option_catalog;
        self.tool_state.executables = snapshot.executables;
        self.tool_state.loading = false;
        self.tool_state.load_error = None;
    }

    pub(crate) fn start_executables_load(&mut self) -> Task<Message> {
        let Some(game_id) = self.current_game_id().map(str::to_string) else {
            self.tool_state.executables.clear();
            self.tool_state.game_label = None;
            self.tool_state.executables_loading = false;
            self.tool_state.executables_load_error = None;
            self.set_status("Select a game before loading executables".to_string());
            return Task::none();
        };
        self.tool_state.game_label = self
            .available_games
            .iter()
            .find(|(id, _)| id == &game_id)
            .map(|(_, name)| name.clone())
            .or_else(|| Some(game_id.clone()));
        self.tool_state.executables_load_generation =
            self.tool_state.executables_load_generation.wrapping_add(1);
        let generation = self.tool_state.executables_load_generation;
        self.tool_state.executables_loading = true;
        self.tool_state.executables_load_error = None;
        let db = self.db.clone();
        Task::perform(load_executables_for_game(db, game_id), move |result| {
            Message::ExecutablesLoaded { generation, result }
        })
    }

    pub(crate) fn refresh_executables_or_tools(&mut self) -> Task<Message> {
        if matches!(self.active_view, View::Executables) {
            self.start_executables_load()
        } else if self.tool_load_request().is_some() {
            self.start_tools_load()
        } else {
            Task::none()
        }
    }

    pub(crate) fn track_download(&mut self, key: &str, name: &str) -> usize {
        if let Some(id) = self.download_lookup.get(key).copied() {
            return id;
        }

        let dest_root = self
            .settings
            .download_dir
            .clone()
            .unwrap_or_else(modde_core::paths::downloads_dir);
        let file_name = key.replace(['/', ':', ' '], "_");
        let dest = dest_root.join(format!("{file_name}.download"));
        let id = self.download_queue.enqueue(
            key.to_string(),
            dest,
            None,
            build_default_download_meta(key, name),
        );
        self.download_lookup.insert(key.to_string(), id);
        id
    }

    pub(crate) fn downloads_view_tasks(&self) -> Vec<crate::views::downloads::DownloadTask> {
        self.download_queue
            .all()
            .iter()
            .map(|task| {
                let state = match &task.state {
                    modde_sources::queue::DownloadState::Queued => {
                        crate::views::downloads::DownloadState::Queued
                    }
                    modde_sources::queue::DownloadState::Active {
                        bytes_downloaded,
                        total_bytes,
                    } => crate::views::downloads::DownloadState::Active {
                        bytes_downloaded: *bytes_downloaded,
                        total_bytes: *total_bytes,
                    },
                    modde_sources::queue::DownloadState::Paused {
                        bytes_downloaded,
                        total_bytes,
                    } => crate::views::downloads::DownloadState::Paused {
                        bytes_downloaded: *bytes_downloaded,
                        total_bytes: *total_bytes,
                    },
                    modde_sources::queue::DownloadState::Complete { path, .. } => {
                        crate::views::downloads::DownloadState::Complete { path: path.clone() }
                    }
                    modde_sources::queue::DownloadState::Failed { error } => {
                        crate::views::downloads::DownloadState::Failed {
                            error: error.clone(),
                        }
                    }
                };

                crate::views::downloads::DownloadTask {
                    id: task.id,
                    name: task
                        .meta
                        .mod_name
                        .clone()
                        .unwrap_or_else(|| task.url.clone()),
                    state,
                }
            })
            .collect()
    }

    // ── Browse Nexus helpers (Phase 6) ──────────────────────────

    /// Return the currently-selected game's Nexus domain, if the game
    /// plugin defines one. Used by the Browse Nexus view to issue
    /// GraphQL queries scoped to the right game.
    pub fn current_game_nexus_domain(&self) -> Option<String> {
        let game_id = self
            .loaded_profile
            .as_ref()
            .map(|p| p.game_id.to_string())
            .or_else(|| self.selected_game.clone())?;
        Self::nexus_domain_for_game(&game_id)
    }

    pub(crate) fn nexus_domain_for_game(game_id: &str) -> Option<String> {
        let game = modde_games::resolve_game(game_id)?;
        game.nexus_game_id?;
        game.nexus_domain.map(str::to_string)
    }

    pub(crate) fn first_supported_nexus_game(&self) -> Option<String> {
        self.available_games
            .iter()
            .map(|(id, _)| id)
            .find(|id| Self::nexus_domain_for_game(id).is_some())
            .cloned()
    }

    pub(crate) fn default_browse_game_id(&self) -> Option<String> {
        self.current_game_id()
            .filter(|game_id| Self::nexus_domain_for_game(game_id).is_some())
            .map(str::to_string)
            .or_else(|| self.first_supported_nexus_game())
    }

    pub(crate) fn browse_game_nexus_domain(&self) -> Option<String> {
        self.browse_nexus
            .selected_game_id
            .as_deref()
            .and_then(Self::nexus_domain_for_game)
    }

    pub(crate) fn clear_browse_results(&mut self) {
        self.browse_nexus.mods.clear();
        self.browse_nexus.collections.clear();
        self.browse_nexus.error = None;
        self.browse_nexus.install_status = None;
    }

    pub(crate) fn sync_browse_game_to_current(&mut self, force: bool) {
        let selected_is_supported = self
            .browse_nexus
            .selected_game_id
            .as_deref()
            .is_some_and(|game_id| Self::nexus_domain_for_game(game_id).is_some());
        if !force && selected_is_supported {
            return;
        }
        let next = self.default_browse_game_id();
        if self.browse_nexus.selected_game_id != next {
            self.browse_nexus.selected_game_id = next;
            self.clear_browse_results();
        }
    }

    pub(crate) fn initialize_wabbajack_game_filter(&self, state: &mut WabbajackInstallerState) {
        if state.game_filter_user_edited || state.game_filter.is_some() {
            return;
        }
        state.game_filter = self.current_game_id().map(str::to_string);
    }

    /// Kick off an async feed load for the Browse Nexus view. Picks
    /// the right GraphQL query based on the tab.
    pub fn spawn_browse_load(
        &mut self,
        tab: crate::views::browse_nexus::BrowseTab,
        game_domain: String,
        search_query: String,
    ) -> Task<Message> {
        use crate::views::browse_nexus::BrowseTab;
        self.browse_nexus.loading = true;
        self.browse_nexus.error = None;
        match tab {
            BrowseTab::Top | BrowseTab::Month => {
                let kind = match tab {
                    BrowseTab::Top => modde_sources::nexus::graphql::ModFeedKind::Trending,
                    _ => modde_sources::nexus::graphql::ModFeedKind::MonthlyTop,
                };
                Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        api.browse_feed_gql(&game_domain, kind)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::BrowseModsLoaded,
                )
            }
            BrowseTab::Search => Task::perform(
                async move {
                    let api_key =
                        modde_sources::nexus::auth::load_api_key().map_err(|e| e.to_string())?;
                    let client = reqwest::Client::new();
                    let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                    api.search_mods_gql(&game_domain, &search_query, 1)
                        .await
                        .map_err(|e| e.to_string())
                },
                Message::BrowseModsLoaded,
            ),
            BrowseTab::Collections => {
                let term = if search_query.is_empty() {
                    None
                } else {
                    Some(search_query)
                };
                Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        api.collections_feed_gql(&game_domain, term.as_deref())
                            .await
                            .map_err(|e| e.to_string())
                    },
                    Message::BrowseCollectionsLoaded,
                )
            }
        }
    }
}
