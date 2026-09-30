#![allow(clippy::wildcard_imports)]
//! core model helpers.

use super::*;

impl Modde {
    pub fn settings_state(&self) -> SettingsState {
        SettingsState {
            nexus_api_key_draft: self.nexus_api_key_draft.clone(),
            nexus_api_key_visible: self.nexus_api_key_visible,
            nexus_api_key_source: self.nexus_api_key_source.clone(),
            nexus_config_key_exists: self.nexus_config_key_exists,
            game_install_paths: settings_game_install_paths(
                &self.settings,
                modde_games::scan_installed_games(),
            ),
            download_dir: self.settings.download_dir.clone(),
            effective_download_dir: self
                .settings
                .download_dir
                .clone()
                .unwrap_or_else(modde_core::paths::downloads_dir),
            has_stock_snapshot: self.stock_snapshot_exists,
            theme_name: self.theme_name.clone(),
            nexus_status: self.nexus_status.clone(),
        }
    }

    pub(crate) fn refresh_nexus_api_key_state(&mut self) {
        self.nexus_config_key_exists = modde_sources::nexus::auth::config_api_key_exists();
        if let Ok(loaded) = modde_sources::nexus::auth::load_api_key_with_source() {
            self.nexus_api_key_draft = loaded.key;
            self.nexus_api_key_source = Some(loaded.source);
        } else {
            self.nexus_api_key_draft.clear();
            self.nexus_api_key_source = None;
        }
    }

    pub fn fomod_is_last_step(&self) -> bool {
        if self.fomod_visible_step_indices.is_empty() {
            return true;
        }
        self.fomod_wizard_pos >= self.fomod_visible_step_indices.len().saturating_sub(1)
    }

    pub fn reset_fomod(&mut self) {
        self.fomod_installer = None;
        self.fomod_source_dir = None;
        self.fomod_dest_dir = None;
        self.fomod_visible_step_indices.clear();
        self.fomod_wizard_pos = 0;
        self.fomod_selections.clear();
        self.fomod_conflicts.clear();
        self.fomod_can_undo = false;
    }

    pub fn refresh_fomod_visible_steps(&mut self) {
        if let Some(ref installer) = self.fomod_installer {
            self.fomod_visible_step_indices = installer
                .visible_steps()
                .iter()
                .map(|&(idx, _)| idx)
                .collect();
        }
    }

    pub(crate) fn refresh_fomod_conflicts(&mut self) {
        if let Some(ref installer) = self.fomod_installer {
            self.fomod_conflicts = installer.detect_conflicts().into();
        }
    }

    pub(crate) fn clear_game_scoped_state(&mut self) {
        self.selected_mod_index = None;
        self.selected_mod_details = None;
        self.selected_save_details = None;
        self.save_snapshots.clear();
        self.current_fingerprint = None;
        self.experiment_depth = 0;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
        self.data_tab_conflicts.clear();
        self.data_tab_state.missing_store_mod_count = 0;
    }

    pub(crate) fn game_supports_save_profiles(game_id: &str) -> bool {
        modde_games::resolve_game_plugin(game_id)
            .is_some_and(modde_games::GamePlugin::supports_save_profiles)
    }

    pub(crate) fn current_game_supports_save_profiles(&self) -> bool {
        self.loaded_profile
            .as_ref()
            .map(|p| p.game_id.as_str())
            .or(self.selected_game.as_deref())
            .is_some_and(Self::game_supports_save_profiles)
    }

    /// Reload the active profile, its data-tab conflicts, and the tool state
    /// off the render thread, returning the `Task` that resolves to
    /// `Message::ProfileContextLoaded`. Replaces the old synchronous
    /// `reload_profile` (which drove the whole multi-query reload via
    /// `block_on` on the iced thread).
    pub(crate) fn reload_profile(&mut self) -> Task<Message> {
        let request = self.reload_request(false);
        self.dispatch_profile_context(request)
    }

    /// Like [`Self::reload_profile`], but re-runs diagnostics after the load
    /// resolves when the Diagnostics view is active. Used only by the
    /// profile-switch handler to preserve its previous synchronous behavior.
    pub(crate) fn reload_profile_refresh_diagnostics(&mut self) -> Task<Message> {
        let request = self.reload_request(true);
        self.dispatch_profile_context(request)
    }

    /// Reload, recomputing the active profile from the DB (active pointer, then
    /// first listed profile). Used after deleting the active profile in a
    /// no-game context, where the previous active profile is gone.
    pub(crate) fn reload_profile_recompute_active(&mut self) -> Task<Message> {
        let mut request = self.reload_request(false);
        request.recompute_active = true;
        self.dispatch_profile_context(request)
    }

    /// Build the request for an in-place reload (no scoped-state clear, keeps
    /// the current `selected_game`/`active_profile`).
    fn reload_request(&self, rerun_diagnostics: bool) -> ProfileContextRequest {
        ProfileContextRequest {
            settings: self.settings.clone(),
            selected_game: self.selected_game.clone(),
            active_profile: self.active_profile.clone(),
            recompute_active: false,
            fallback_profile: self.loaded_profile.clone(),
            tool_request: self.tool_load_request(),
            rerun_diagnostics,
        }
    }

    /// Bump the context generation, mark the tool state as loading (when a tool
    /// reload is folded in), and spawn the off-thread loader.
    fn dispatch_profile_context(&mut self, request: ProfileContextRequest) -> Task<Message> {
        self.context_generation = self.context_generation.wrapping_add(1);
        let generation = self.context_generation;
        self.diagnostics_generation = self.diagnostics_generation.wrapping_add(1);
        // A composite reload recomputes the data-tab conflicts authoritatively,
        // so any in-flight standalone data-tab refresh is now stale — invalidate
        // it so its older result can't clobber the new game's conflicts.
        self.data_tab_generation = self.data_tab_generation.wrapping_add(1);
        if request.tool_request.is_some() {
            self.tool_state.loading = true;
            self.tool_state.load_error = None;
        }
        let db = self.db.clone();
        Task::perform(load_profile_context(db, request), move |result| {
            Message::ProfileContextLoaded { generation, result }
        })
    }

    /// Apply a resolved [`ProfileContextSnapshot`] into `self` — the synchronous
    /// tail of the old `reload_profile`/`switch_game_context`/
    /// `refresh_data_tab_conflicts`/`refresh_tools_state` helpers.
    pub(crate) fn apply_profile_context(&mut self, snapshot: ProfileContextSnapshot) {
        self.profiles = snapshot.profiles;
        self.active_profile = snapshot.active_profile;
        match snapshot.profile_outcome {
            ProfileLoadOutcome::Loaded {
                profile,
                experiment_depth,
                current_fingerprint,
                mod_id_filter_keys,
            } => {
                self.experiment_depth = experiment_depth;
                self.current_fingerprint = current_fingerprint;
                self.mod_id_filter_keys = mod_id_filter_keys;
                self.loaded_profile = Some(*profile);
            }
            ProfileLoadOutcome::Cleared => {
                self.loaded_profile = None;
                self.mod_id_filter_keys.clear();
            }
            ProfileLoadOutcome::KeepPrevious => {}
        }
        self.data_tab_conflicts = snapshot.data_tab_conflicts;
        self.data_tab_state.missing_store_mod_count = snapshot.missing_store_mod_count;
        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Idle;
        if let Some(tools) = snapshot.tools {
            self.apply_tool_snapshot(tools);
        } else {
            // No game in scope — clear the tool state (mirrors the old
            // `refresh_tools_state` empty-game branch).
            self.tool_state.entries.clear();
            self.tool_state.active_tool_id = None;
            self.tool_state.game_label = None;
            self.tool_state.game_dir_configured = false;
            self.tool_state.loading = false;
            self.tool_state.load_error = None;
        }
    }

    /// Synchronously drive [`load_profile_context`] + [`Self::apply_profile_context`]
    /// for tests, which pump `update` but discard returned `Task`s.
    #[cfg(test)]
    pub(crate) fn reload_profile_blocking(&mut self) {
        let request = self.reload_request(false);
        let snapshot = crate::app::block_on(load_profile_context(self.db.clone(), request))
            .expect("load profile context");
        self.apply_profile_context(snapshot);
    }

    /// Synchronously complete the game-context switch that a `SelectGame` /
    /// `GamePathDialogPathSelected` handler just kicked off (the returned
    /// `Task` is discarded by the test harness). Mirrors the request
    /// `switch_game_context` builds, using the already-set `selected_game`.
    #[cfg(test)]
    pub(crate) fn finish_pending_switch_blocking(&mut self) {
        let game_id = self
            .selected_game
            .clone()
            .expect("selected_game set by the switch kickoff");
        let request = ProfileContextRequest {
            settings: self.settings.clone(),
            selected_game: Some(game_id.clone()),
            active_profile: None,
            recompute_active: true,
            fallback_profile: None,
            tool_request: Some(self.tool_load_request_for(&game_id)),
            rerun_diagnostics: false,
        };
        let snapshot = crate::app::block_on(load_profile_context(self.db.clone(), request))
            .expect("load profile context");
        self.apply_profile_context(snapshot);
    }

    /// Switch the game context: clear game-scoped state and the previously
    /// loaded profile (so the view repaints to a "loading" state immediately
    /// rather than showing the old game's data), then reload the new game's
    /// profile/conflicts/tools off-thread. Returns the resolving `Task`.
    pub(crate) fn switch_game_context(&mut self, game_id: &str) -> Task<Message> {
        self.clear_game_scoped_state();
        self.loaded_profile = None;
        self.mod_id_filter_keys.clear();
        let request = ProfileContextRequest {
            settings: self.settings.clone(),
            selected_game: Some(game_id.to_string()),
            active_profile: None,
            recompute_active: true,
            fallback_profile: None,
            tool_request: Some(self.tool_load_request_for(game_id)),
            rerun_diagnostics: false,
        };
        self.dispatch_profile_context(request)
    }

    pub(crate) fn accept_game_selection(
        &mut self,
        game_id: String,
        previous_game: Option<String>,
    ) -> Task<Message> {
        self.selected_game = Some(game_id.clone());
        self.settings.selected_game = Some(game_id.clone());
        let typed_game_id = GameId::from(game_id.as_str());

        let configured_path_valid = self
            .settings
            .game_path(&typed_game_id)
            .is_some_and(|path| path.is_dir());
        if !configured_path_valid {
            if let Some(path) = modde_games::find_detected_game(&typed_game_id)
                .map(|detected| detected.install_path)
                .or_else(|| {
                    modde_games::resolve_game_plugin(&game_id)
                        .and_then(modde_games::GamePlugin::detect_install)
                })
            {
                self.settings.set_game_path(&typed_game_id, path);
                self.detected_games.insert(game_id.clone());
            } else {
                self.game_path_dialog_open = true;
                self.pending_game_path_game_id = Some(game_id.clone());
                self.previous_game_before_path_dialog = previous_game;
                self.game_path_dialog_error = None;
                self.status_message = format!("Set the game directory for {game_id}");
                self.save_settings();
                return Task::none();
            }
        }

        self.game_path_dialog_open = false;
        self.pending_game_path_game_id = None;
        self.previous_game_before_path_dialog = None;
        self.game_path_dialog_error = None;
        let task = self.switch_game_context(&game_id);
        self.sync_browse_game_to_current(true);
        self.save_settings();
        self.status_message = format!("Active game set to {game_id}");
        task
    }

    pub(crate) fn save_settings(&self) {
        self.settings.save();
    }

    pub(crate) fn refresh_available_games(&mut self) {
        self.available_games = modde_games::supported_games()
            .iter()
            .map(|(id, name)| (id.to_string(), name.to_string()))
            .collect();
        self.detected_games = detected_game_ids(&self.settings, self.available_games.as_slice());
    }

    pub(crate) fn custom_games(&self) -> Vec<(String, String)> {
        self.available_games
            .iter()
            .filter(|(id, _)| !modde_games::SUPPORTED_GAME_IDS.contains(&id.as_str()))
            .cloned()
            .collect()
    }

    pub(crate) fn current_game_id(&self) -> Option<&str> {
        self.loaded_profile
            .as_ref()
            .map(|profile| profile.game_id.as_str())
            .or(self.selected_game.as_deref())
    }

    pub(crate) fn current_game_dir(&self) -> Option<PathBuf> {
        let game_id = self.current_game_id()?;
        self.settings
            .game_path(&GameId::from(game_id))
            .cloned()
            .or_else(|| {
                modde_games::resolve_game_plugin(game_id)
                    .and_then(modde_games::GamePlugin::detect_install)
            })
    }

    pub(crate) fn add_custom_game_modal(&self) -> Element<'_, Message> {
        opaque(
            container(crate::views::add_custom_game::add_dialog(
                &self.add_custom_game,
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
    }

    pub(crate) fn manage_custom_games_modal(&self) -> Element<'_, Message> {
        opaque(
            container(crate::views::add_custom_game::manage_dialog(
                self.custom_games(),
            ))
            .width(Length::Fill)
            .height(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill),
        )
    }

    pub(crate) fn current_tool_game_context(&self) -> Option<modde_games::tools::ToolGameContext> {
        let game_id = self.current_game_id()?;
        let display_name = self
            .available_games
            .iter()
            .find(|(id, _)| id == game_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| game_id.to_string());
        let install_path = self.current_game_dir();
        let detected = modde_games::detection::find_detected_game(&GameId::from(game_id));
        Some(modde_games::tools::ToolGameContext::from_parts(
            game_id,
            display_name,
            install_path,
            detected.as_ref(),
        ))
    }
}
