#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! game_selection update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_game_selection_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Game selection ────────────────────────────────────
            Message::SelectGame(game_id) => {
                let previous_game = self.selected_game.clone();
                return self.accept_game_selection(game_id, previous_game);
            }
            Message::GamePathDialogBrowse => {
                let Some(game_id) = self.pending_game_path_game_id.clone() else {
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Game Directory")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    move |path| match path {
                        Some(path) => Message::GamePathDialogPathSelected {
                            game_id: game_id.clone(),
                            path,
                        },
                        None => Message::Noop,
                    },
                );
            }
            Message::GamePathDialogPathSelected { game_id, path } => {
                if !path.is_dir() {
                    self.game_path_dialog_error =
                        Some(format!("Not a directory: {}", path.display()));
                    self.set_status("Select a valid game directory".to_string());
                    return Task::none();
                }
                self.settings
                    .set_game_path(&GameId::from(game_id.as_str()), path);
                self.detected_games.insert(game_id.clone());
                self.selected_game = Some(game_id.clone());
                self.settings.selected_game = Some(game_id.clone());
                self.game_path_dialog_open = false;
                self.pending_game_path_game_id = None;
                self.previous_game_before_path_dialog = None;
                self.game_path_dialog_error = None;
                let task = self.switch_game_context(&game_id);
                self.save_settings();
                self.set_status(format!("Active game set to {game_id}"));
                return task;
            }
            Message::CancelGamePathDialog => {
                let previous = self.previous_game_before_path_dialog.clone();
                self.game_path_dialog_open = false;
                self.pending_game_path_game_id = None;
                self.previous_game_before_path_dialog = None;
                self.game_path_dialog_error = None;
                self.selected_game = previous.clone();
                self.settings.selected_game = previous.clone();
                let task = if let Some(game_id) = previous {
                    let task = self.switch_game_context(&game_id);
                    self.set_status(format!("Active game remains {game_id}"));
                    task
                } else {
                    self.clear_game_scoped_state();
                    self.profiles.clear();
                    self.active_profile = None;
                    self.loaded_profile = None;
                    self.mod_id_filter_keys.clear();
                    self.set_status("Game selection cancelled".to_string());
                    Task::none()
                };
                self.save_settings();
                return task;
            }
            Message::OpenAddCustomGame => {
                self.add_custom_game_dialog_open = true;
                self.manage_custom_games_dialog_open = false;
                self.add_custom_game.error = None;
            }
            Message::BrowseAddCustomGameInstallPath => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Custom Game Install Directory")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    |path| match path {
                        Some(path) => Message::AddCustomGameInstallPathPicked(path),
                        None => Message::Noop,
                    },
                );
            }
            Message::AddCustomGameFieldChanged { field, value } => {
                self.add_custom_game.error = None;
                match field {
                    AddCustomGameDraftField::Id => self.add_custom_game.draft.id = value,
                    AddCustomGameDraftField::DisplayName => {
                        self.add_custom_game.draft.display_name = value;
                    }
                    AddCustomGameDraftField::InstallPath => {
                        self.add_custom_game.draft.install_path = value;
                        self.add_custom_game.draft.executable_dir = None;
                        self.add_custom_game.detected_dirs.clear();
                    }
                    AddCustomGameDraftField::ExecutableDir => {
                        self.add_custom_game.draft.executable_dir = Some(value);
                    }
                    AddCustomGameDraftField::SteamAppId => {
                        self.add_custom_game.draft.steam_app_id = empty_to_none(Some(&value));
                    }
                    AddCustomGameDraftField::NexusDomain => {
                        self.add_custom_game.draft.nexus_domain = empty_to_none(Some(&value));
                    }
                    AddCustomGameDraftField::ProxyDlls => {
                        self.add_custom_game.draft.proxy_dlls_csv = value;
                    }
                }
            }
            Message::AddCustomGameInstallPathPicked(path) => {
                self.add_custom_game.error = None;
                self.add_custom_game.draft.install_path = path.display().to_string();
                match modde_games::detect_candidates(&path) {
                    Ok(candidates) => {
                        self.add_custom_game.detected_dirs = candidates;
                        self.add_custom_game.draft.executable_dir = self
                            .add_custom_game
                            .detected_dirs
                            .first()
                            .map(|candidate| candidate.relative_dir.clone());
                    }
                    Err(error) => {
                        self.add_custom_game.detected_dirs.clear();
                        self.add_custom_game.draft.executable_dir = None;
                        self.add_custom_game.error = Some(error.to_string());
                    }
                }
            }
            Message::AddCustomGameSubmit => {
                let install_path = PathBuf::from(self.add_custom_game.draft.install_path.trim());
                let spec = match self.add_custom_game.build_spec() {
                    Ok(spec) => spec,
                    Err(error) => {
                        self.add_custom_game.error = Some(error.clone());
                        self.set_error(error);
                        return Task::none();
                    }
                };
                match modde_games::add_user_game(&spec, false) {
                    Ok(_) => {
                        modde_games::reload_user_games();
                        self.settings
                            .set_game_path(&GameId::from(spec.id.as_str()), install_path);
                        self.refresh_available_games();
                        self.add_custom_game_dialog_open = false;
                        self.add_custom_game = AddCustomGameState::default();
                        let previous_game = self.selected_game.clone();
                        let task = self.accept_game_selection(spec.id.clone(), previous_game);
                        self.set_status(format!("Registered custom game '{}'", spec.display_name));
                        return task;
                    }
                    Err(error) => {
                        self.add_custom_game.error = Some(error.to_string());
                        self.set_error(format!("Custom game not saved: {error}"));
                    }
                }
            }
            Message::AddCustomGameCancel => {
                self.add_custom_game_dialog_open = false;
                self.add_custom_game = AddCustomGameState::default();
            }
            Message::OpenManageCustomGames => {
                self.manage_custom_games_dialog_open = true;
                self.add_custom_game_dialog_open = false;
                self.add_custom_game.error = None;
            }
            Message::CloseManageCustomGames => {
                self.manage_custom_games_dialog_open = false;
            }
            Message::RemoveCustomGame(id) => match modde_games::remove_user_game(&id) {
                Ok(_) => {
                    modde_games::reload_user_games();
                    self.settings
                        .game_paths
                        .retain(|entry| entry.game_id.as_ref() != id.as_str());
                    if self.settings.selected_game.as_deref() == Some(id.as_str()) {
                        self.settings.selected_game = None;
                        self.selected_game = None;
                    }
                    self.refresh_available_games();
                    let task = if self.selected_game.is_none()
                        && let Some((game_id, _)) = self.available_games.first().cloned()
                    {
                        self.accept_game_selection(game_id, None)
                    } else {
                        Task::none()
                    };
                    self.save_settings();
                    self.set_status(format!("Removed custom game '{id}'"));
                    return task;
                }
                Err(error) => {
                    self.set_error(format!("Custom game not removed: {error}"));
                }
            },
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
