#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! navigation update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_navigation_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Navigation ───────────────────────────────────────
            Message::SwitchView(view) => match view {
                View::Library => {
                    self.active_view = View::Library;
                    match modde_core::library::PendingSession::load_blocking() {
                        Ok(pending) => {
                            self.library.session_revision =
                                self.library.session_revision.wrapping_add(1);
                            self.library.pending = pending;
                        }
                        Err(error) => {
                            self.status_message = format!("Could not load recovery state: {error}")
                        }
                    }
                    if self.library.entries.is_empty() && !self.library.loading {
                        return self.update(Message::LibraryRefresh);
                    }
                }
                View::Saves => {
                    self.active_view = View::Saves;
                    if !self.current_game_supports_save_profiles() {
                        self.save_snapshots.clear();
                        self.selected_save_details = None;
                        self.current_fingerprint = None;
                        self.status_message =
                            "Save profiles are not supported for this game".to_string();
                        return Task::none();
                    }
                    return self.update(Message::LoadSaveHistory);
                }
                View::DataTab => {
                    self.active_view = View::DataTab;
                    return self.refresh_data_tab_conflicts();
                }
                View::Tools => {
                    self.active_view = View::Tools;
                    return self.update(Message::LoadTools);
                }
                View::Executables => {
                    self.active_view = View::Executables;
                    return self.update(Message::LoadExecutables);
                }
                View::Diagnostics => {
                    self.active_view = View::Diagnostics;
                    return self.update(Message::RunDiagnostics);
                }
                View::WabbajackInstaller(state) => {
                    let mut state = state;
                    self.initialize_wabbajack_game_filter(&mut state);
                    let should_load = state.entries.is_empty();
                    self.active_view = View::WabbajackInstaller(state);
                    if should_load {
                        return self.update(Message::LoadWabbajackCatalog);
                    }
                }
                View::BrowseNexus => {
                    self.active_view = View::BrowseNexus;
                    self.sync_browse_game_to_current(false);
                }
                other => {
                    self.active_view = other;
                }
            },
            Message::ToggleSidebarGroup(group) => {
                if !self.collapsed_sidebar_groups.insert(group) {
                    self.collapsed_sidebar_groups.remove(&group);
                }
            }
            Message::SwitchProfile(name) => {
                self.active_profile = Some(name);
                // The reload re-runs diagnostics once it resolves if the
                // Diagnostics view is active (it needs the freshly loaded
                // profile), preserving the old synchronous behavior.
                let task = self.reload_profile_refresh_diagnostics();
                self.sync_browse_game_to_current(true);
                self.selected_save_details = None;
                self.status_message = "Profile switched".to_string();
                return task;
            }
            Message::CreateProfile { name, game_id } => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    self.status_message = "Profile name is required".to_string();
                    return Task::none();
                }
                let profile = modde_core::Profile {
                    id: None,
                    name: name.clone(),
                    game_id: modde_core::GameId::from(game_id.clone()),
                    source: modde_core::ProfileSource::Manual,
                    mods: Vec::new(),
                    overrides: PathBuf::from("overrides"),
                    load_order_rules: smallvec::SmallVec::new(),
                    load_order_lock: None,
                };
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                self.status_message = format!("Creating profile '{name}'...");
                return Task::perform(create_profile(self.db.clone(), profile), move |result| {
                    Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Create {
                            name: name.clone(),
                            game_id: game_id.clone(),
                        },
                        result,
                    }
                });
            }
            Message::DeleteProfile(name) => {
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                self.status_message = format!("Deleting profile '{name}'...");
                return Task::perform(
                    delete_profile(self.db.clone(), name.clone()),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Delete { name: name.clone() },
                        result,
                    },
                );
            }
            Message::ForkProfile { source, new_name } => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    self.status_message = format!("Forking profile as '{new_name}'...");
                    return Task::perform(
                        fork_profile(
                            self.db.clone(),
                            source,
                            new_name.clone(),
                            game_id,
                            self.settings.clone(),
                        ),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::Fork {
                                new_name: new_name.clone(),
                            },
                            result,
                        },
                    );
                }
            }
            Message::ProfileWriteDone {
                generation,
                kind,
                result,
            } => {
                if generation != self.context_generation {
                    return Task::none();
                }
                match result {
                    Ok(outcome) => {
                        match &kind {
                            ProfileWriteKind::Create { name, game_id } => {
                                self.active_profile = Some(name.clone());
                                self.selected_game = Some(game_id.clone());
                                self.settings.selected_game = Some(game_id.clone());
                                self.save_settings();
                                self.new_profile_name.clear();
                                self.new_profile_dialog_open = false;
                            }
                            ProfileWriteKind::Fork { new_name } => {
                                self.active_profile = Some(new_name.clone());
                            }
                            ProfileWriteKind::RemoveMod => {
                                self.selected_mod_index = None;
                            }
                            _ => {}
                        }
                        if let Some(status) = outcome.status_message {
                            self.status_message = status;
                        }
                        if outcome.reload {
                            return match kind {
                                ProfileWriteKind::Delete { name } => {
                                    if let Some(game_id) = self.selected_game.clone() {
                                        self.switch_game_context(&game_id)
                                    } else if self.active_profile.as_deref() == Some(&name) {
                                        self.reload_profile_recompute_active()
                                    } else {
                                        self.reload_profile()
                                    }
                                }
                                _ => self.reload_profile(),
                            };
                        }
                    }
                    Err(err) => {
                        self.status_message = match kind {
                            ProfileWriteKind::Create { .. } => {
                                format!("Failed to create profile: {err}")
                            }
                            ProfileWriteKind::Delete { .. } => {
                                format!("Failed to delete profile: {err}")
                            }
                            ProfileWriteKind::Fork { .. } => format!("Fork failed: {err}"),
                            ProfileWriteKind::AddMod { .. } => format!("Failed to add mod: {err}"),
                            ProfileWriteKind::RemoveMod => {
                                format!("Failed to remove mod: {err}")
                            }
                            ProfileWriteKind::ToggleMod { .. } => {
                                format!("Failed to update mod: {err}")
                            }
                            ProfileWriteKind::Reorder { .. } => {
                                format!("Failed to reorder mod: {err}")
                            }
                            ProfileWriteKind::Lock { .. } => {
                                format!("Failed to pin mod: {err}")
                            }
                            ProfileWriteKind::Unlock { .. } => {
                                format!("Failed to unpin mod: {err}")
                            }
                        };
                    }
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
