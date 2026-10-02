#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! saves update handlers.

use super::*;

impl Modde {
    fn installation_saves(
        &self,
        game_id: &GameId,
    ) -> Result<modde_core::library::SaveContext, String> {
        crate::app::block_on(modde_games::library::context::for_game(
            &self.settings,
            game_id.as_str(),
            &self.db,
        ))
        .map(|context| context.saves)
        .map_err(|error| error.to_string())
    }

    pub(super) fn handle_saves_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Saves ────────────────────────────────────────────
            Message::LoadSaveHistory => {
                self.selected_save_details = None;
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    if !Self::game_supports_save_profiles(game_id.as_str()) {
                        self.save_snapshots.clear();
                        self.current_fingerprint = None;
                        self.status_message =
                            "Save profiles are not supported for this game".to_string();
                        return Task::none();
                    }
                    let profile_name = profile.name.clone();
                    let context = match self.installation_saves(&game_id) {
                        Ok(context) => context,
                        Err(error) => {
                            self.save_snapshots.clear();
                            self.status_message = error;
                            return Task::none();
                        }
                    };
                    match modde_core::save::SaveManager::history(&context.scope, &profile_name, 20)
                    {
                        Ok(history) => self.save_snapshots = history,
                        Err(e) => {
                            self.save_snapshots = Vec::new();
                            self.status_message = format!("Could not load save history: {e}");
                        }
                    }
                }
            }
            Message::SelectSaveSnapshot(commit_id) => {
                if let Some(snap) = self.save_snapshots.iter().find(|s| s.id == commit_id) {
                    let compat = snap
                        .fingerprint
                        .as_ref()
                        .zip(self.current_fingerprint.as_ref())
                        .map(|(_, current)| snap.check_compatibility(current));

                    let mut details =
                        crate::views::save_details::SaveDetailsState::from_snapshot(snap, compat);

                    // Load file list synchronously (fast git tree walk)
                    if let Some(ref profile) = self.loaded_profile {
                        let context = match self.installation_saves(&profile.game_id) {
                            Ok(context) => context,
                            Err(error) => {
                                self.status_message = error;
                                return Task::none();
                            }
                        };
                        match modde_core::save::SaveManager::snapshot_file_list(
                            &context.scope,
                            &commit_id,
                        ) {
                            Ok(files) => details.file_paths = Some(files),
                            Err(_) => details.file_paths = Some(Vec::new()),
                        }
                    }

                    self.selected_save_details = Some(details);
                }
            }
            Message::RestoreSaveSnapshot(commit_id) => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    let profile_name = profile.name.clone();
                    let context =
                        match crate::app::block_on(modde_games::library::context::for_game(
                            &self.settings,
                            game_id.as_str(),
                            &self.db,
                        )) {
                            Ok(context) => context,
                            Err(error) => {
                                self.status_message = error.to_string();
                                return Task::none();
                            }
                        };
                    if context.saves.directory.is_some() {
                        match crate::app::block_on(self.db.get_active_profile(&context.saves.scope))
                        {
                            Ok(Some((_, active))) if active == profile_name => {}
                            Ok(_) => {
                                self.status_message = "Activate this profile for the selected installation before restoring its saves".into();
                                return Task::none();
                            }
                            Err(error) => {
                                self.status_message = error.to_string();
                                return Task::none();
                            }
                        }
                        let pm = ProfileManager::with_db(self.db.clone());
                        match crate::app::block_on(context.restore_saves(
                            &pm,
                            &profile_name,
                            &commit_id,
                            self.current_fingerprint.as_ref(),
                        )) {
                            Ok(count) => {
                                self.status_message = format!("Restored {count} save file(s)");
                            }
                            Err(e) => self.status_message = format!("Restore failed: {e}"),
                        }
                    } else if Self::game_supports_save_profiles(game_id.as_str()) {
                        self.status_message =
                            "Cannot detect save directory for this game".to_string();
                    } else {
                        self.status_message =
                            "Save profiles are not supported for this game".to_string();
                    }
                }
            }

            Message::DataTabFilterChanged(f) => {
                self.data_tab_state.filter = f;
            }
            Message::DataTabToggleConflicts(v) => {
                self.data_tab_state.show_conflicts_only = v;
            }
            Message::RunDiagnostics => {
                return self.start_diagnostics_load();
            }
            Message::CrashLogPathChanged(path) => {
                self.crash_log_path_draft = path;
            }
            Message::AnalyzeCrashLog => {
                return self.start_crash_log_analysis();
            }
            Message::DiagnosticsComputed { generation, result } => {
                if generation != self.diagnostics_generation {
                    return Task::none();
                }
                match result {
                    Ok(computed) => self.apply_diagnostics_computed(computed),
                    Err(err) => {
                        self.diagnostics_state = crate::views::diagnostics::DiagnosticsState::Error(
                            format!("Diagnostics failed: {err}"),
                        );
                        self.status_message = format!("Diagnostics failed: {err}");
                    }
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
