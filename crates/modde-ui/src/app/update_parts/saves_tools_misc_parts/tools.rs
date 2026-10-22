#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! tools update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_tools_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LoadTools => {
                self.set_status("Loading tools...".to_string());
                return self.start_tools_load();
            }
            Message::RefreshTools => {
                self.set_status("Loading tools...".to_string());
                return self.start_tools_load();
            }
            Message::LoadExecutables => {
                self.set_status("Loading executables...".to_string());
                return self.start_executables_load();
            }
            Message::RefreshExecutables => {
                self.set_status("Loading executables...".to_string());
                return self.start_executables_load();
            }
            Message::ToolsLoaded { generation, result } => {
                if generation != self.tool_state.load_generation {
                    return Task::none();
                }
                self.tool_state.loading = false;
                match result {
                    Ok(snapshot) => {
                        let count = snapshot.entries.len();
                        self.apply_tool_snapshot(snapshot);
                        let message =
                            if let Some(status) = self.pending_tools_load_status_message.take() {
                                status
                            } else if count == 0 {
                                "No tool state available for the current game".to_string()
                            } else {
                                format!("Loaded {count} tool(s)")
                            };
                        self.set_status(message);
                    }
                    Err(err) => {
                        self.pending_tools_load_status_message = None;
                        self.tool_state.load_error = Some(err.clone());
                        self.set_error(format!("Failed to load tools: {err}"));
                    }
                }
            }
            Message::ToolSettingWritten { tool_id, result } => match result {
                Ok(result) => {
                    if let Some(option_catalog) = result.tool_option_catalog {
                        self.tool_state.tool_option_catalog = option_catalog;
                    }
                    self.tool_state.active_tool_id = Some(tool_id);
                    self.set_status(result.status_message);
                    self.pending_tools_load_status_message = Some(self.status_message.clone());
                    return self.start_tools_load();
                }
                Err(err) => {
                    self.set_error(format!("Failed to update tool setting: {err}"));
                }
            },
            Message::ExecutablesLoaded { generation, result } => {
                if generation != self.tool_state.executables_load_generation {
                    return Task::none();
                }
                self.tool_state.executables_loading = false;
                match result {
                    Ok(executables) => {
                        let count = executables.len();
                        self.tool_state.executables = executables;
                        self.tool_state.executables_load_error = None;
                        self.set_status(format!("Loaded {count} executable(s)"));
                    }
                    Err(err) => {
                        self.tool_state.executables_load_error = Some(err.clone());
                        self.set_error(format!("Failed to load executables: {err}"));
                    }
                }
            }
            Message::RefreshOptiScalerReleases => {
                self.tool_state.optiscaler_releases_loading = true;
                self.set_status("Loading OptiScaler releases...".to_string());
                return Task::perform(
                    load_tool_releases("optiscaler".to_string()),
                    Message::OptiScalerReleasesLoaded,
                );
            }
            Message::OptiScalerReleasesLoaded(result) => {
                self.tool_state.optiscaler_releases_loading = false;
                match result {
                    Ok(releases) => {
                        let Some(game_id) = self.current_game_id().map(str::to_string) else {
                            self.set_status(
                                "Select a game before loading OptiScaler releases".to_string(),
                            );
                            return Task::none();
                        };
                        self.tool_state.optiscaler_releases = releases;
                        self.tool_state.active_tool_id = Some("optiscaler".to_string());
                        self.set_status("Loading OptiScaler release settings...".to_string());
                        return Task::perform(
                            save_optiscaler_release_selection_for_game(
                                self.db.clone(),
                                game_id,
                                self.tool_state.optiscaler_releases.clone(),
                                self.tool_state.tool_option_catalog.clone(),
                                self.current_tool_game_context(),
                            ),
                            |result| Message::ToolSettingWritten {
                                tool_id: "optiscaler".to_string(),
                                result,
                            },
                        );
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to load OptiScaler releases: {err}"));
                    }
                }
            }
            Message::InstallOptiScalerRelease => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before installing OptiScaler".to_string());
                    return Task::none();
                };
                self.set_status("Installing OptiScaler release...".to_string());
                return Task::perform(
                    install_selected_tool_release(
                        self.db.clone(),
                        game_id,
                        "optiscaler".to_string(),
                    ),
                    Message::OptiScalerReleaseInstalled,
                );
            }
            Message::OptiScalerReleaseInstalled(result) => match result {
                Ok(message) => {
                    self.tool_state.active_tool_id = Some("optiscaler".to_string());
                    self.set_status(message);
                    return self.start_tools_load();
                }
                Err(err) => {
                    self.set_error(format!("Failed to install OptiScaler: {err}"));
                }
            },
            Message::RefreshProtonVersions => {
                self.tool_state.proton_versions_loading = true;
                self.set_status("Loading Proton versions...".to_string());
                return Task::perform(load_proton_versions(), Message::ProtonVersionsLoaded);
            }
            Message::ProtonVersionsLoaded(result) => {
                self.tool_state.proton_versions_loading = false;
                match result {
                    Ok(versions) => {
                        let Some(game_id) = self.current_game_id().map(str::to_string) else {
                            self.set_status(
                                "Select a game before loading Proton versions".to_string(),
                            );
                            return Task::none();
                        };
                        let versions = if versions.is_empty() {
                            proton_version_options_for_ui()
                        } else {
                            versions
                        };
                        set_tool_options(
                            &mut self.tool_state.tool_option_catalog,
                            "proton",
                            "selected_version",
                            versions.clone(),
                        );
                        set_tool_options(
                            &mut self.tool_state.tool_option_catalog,
                            "proton",
                            "_catalog_loaded",
                            vec!["true".to_string()],
                        );
                        self.tool_state.active_tool_id = Some("proton".to_string());
                        self.set_status("Loading Proton version settings...".to_string());
                        return Task::perform(
                            save_proton_selected_version_for_game(
                                self.db.clone(),
                                game_id,
                                versions,
                                self.tool_state.tool_option_catalog.clone(),
                            ),
                            |result| Message::ToolSettingWritten {
                                tool_id: "proton".to_string(),
                                result,
                            },
                        );
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to load Proton versions: {err}"));
                    }
                }
            }
            Message::InstallProtonVersion => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before installing Proton".to_string());
                    return Task::none();
                };
                self.set_status("Installing Proton with protonup-rs...".to_string());
                return Task::perform(
                    install_selected_proton_version(self.db.clone(), game_id),
                    Message::ProtonVersionInstalled,
                );
            }
            Message::ProtonVersionInstalled(result) => match result {
                Ok(message) => {
                    self.tool_state.active_tool_id = Some("proton".to_string());
                    self.set_status(message);
                    return self.start_tools_load();
                }
                Err(err) => {
                    self.set_error(format!("Failed to install Proton: {err}"));
                }
            },
            Message::SelectToolTab(tool_id) => {
                let should_load_optiscaler = tool_id == "optiscaler"
                    && self.tool_state.optiscaler_releases.is_empty()
                    && !self.tool_state.optiscaler_releases_loading;
                let should_load_proton = tool_id == "proton"
                    && !self.tool_state.proton_versions_loading
                    && tool_options(
                        &self.tool_state.tool_option_catalog,
                        "proton",
                        "_catalog_loaded",
                    )
                    .is_none();
                self.tool_state.active_tool_id = Some(tool_id);
                if should_load_optiscaler {
                    self.tool_state.optiscaler_releases_loading = true;
                    self.set_status("Loading OptiScaler releases...".to_string());
                    return Task::perform(
                        load_tool_releases("optiscaler".to_string()),
                        Message::OptiScalerReleasesLoaded,
                    );
                }
                if should_load_proton {
                    self.tool_state.proton_versions_loading = true;
                    self.set_status("Loading Proton versions...".to_string());
                    return Task::perform(load_proton_versions(), Message::ProtonVersionsLoaded);
                }
            }
            Message::UpdateToolSetting {
                tool_id,
                key,
                value,
            } => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before configuring tools".to_string());
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&tool_id) else {
                    self.set_status(format!("Unknown tool: {tool_id}"));
                    return Task::none();
                };
                let display_name = tool.display_name().to_string();
                self.tool_state.active_tool_id = Some(tool_id.clone());
                self.set_status(format!("Updating {display_name} setting..."));
                return Task::perform(
                    save_tool_setting_for_game(
                        self.db.clone(),
                        game_id,
                        tool_id.clone(),
                        key,
                        value,
                        self.current_tool_game_context(),
                        self.tool_state.optiscaler_releases.clone(),
                        self.tool_state.tool_option_catalog.clone(),
                    ),
                    move |result| Message::ToolSettingWritten {
                        tool_id: tool_id.clone(),
                        result,
                    },
                );
            }
            Message::ToggleTool { tool_id, enabled } => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before toggling tools".to_string());
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&tool_id) else {
                    self.set_status(format!("Unknown tool: {tool_id}"));
                    return Task::none();
                };
                let display_name = tool.display_name().to_string();
                self.tool_state.active_tool_id = Some(tool_id.clone());
                self.set_status(format!(
                    "{} {}...",
                    if enabled { "Enabling" } else { "Disabling" },
                    display_name
                ));
                return Task::perform(
                    toggle_tool_for_game(
                        self.db.clone(),
                        game_id,
                        tool_id.clone(),
                        enabled,
                        self.current_tool_game_context(),
                    ),
                    move |result| Message::ToolSettingWritten {
                        tool_id: tool_id.clone(),
                        result,
                    },
                );
            }
            Message::ToggleToolAdvancedSettings => {
                self.tool_state.show_advanced_settings = !self.tool_state.show_advanced_settings;
            }
            Message::ActivateOptiScaler => {
                let id = "optiscaler".to_string();
                if self.tool_state.is_tool_busy(&id) {
                    self.set_status("OptiScaler operation already in progress".to_string());
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before activating OptiScaler".to_string());
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.set_status("Game install path is not configured".to_string());
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                self.tool_state.active_operations.insert(id.clone());
                self.set_status("Activating OptiScaler...".to_string());
                let db = self.db.clone();
                return Task::perform(
                    apply_tool_for_game(db, game_id, game_dir, id.clone(), context),
                    move |result| Message::ToolApplied {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::DeactivateOptiScaler => {
                let id = "optiscaler".to_string();
                if self.tool_state.is_tool_busy(&id) {
                    self.set_status("OptiScaler operation already in progress".to_string());
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before deactivating OptiScaler".to_string());
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.set_status("Game install path is not configured".to_string());
                    return Task::none();
                };
                self.tool_state.active_operations.insert(id.clone());
                self.set_status("Deactivating OptiScaler...".to_string());
                let db = self.db.clone();
                return Task::perform(
                    deactivate_optiscaler_for_game(db, game_id, game_dir),
                    move |result| Message::ToolReverted {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::RestoreToolSettings { tool_id, node_id } => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before restoring tool settings".to_string());
                    return Task::none();
                };
                self.set_status("Restoring tool settings...".to_string());
                let db = self.db.clone();
                return Task::perform(
                    restore_tool_settings_for_game(db, game_id, tool_id.clone(), node_id),
                    move |result| Message::ToolSettingWritten {
                        tool_id: tool_id.clone(),
                        result,
                    },
                );
            }
            Message::ToolSettingsRestored { tool_id, result } => match result {
                Ok(message) => {
                    self.tool_state.active_tool_id = Some(tool_id);
                    self.set_status(message);
                    if self.tool_load_request().is_some() {
                        return self.start_tools_load();
                    }
                }
                Err(err) => {
                    self.set_error(format!("Failed to restore tool settings: {err}"));
                }
            },
            Message::ApplyTool(id) => {
                if self.tool_state.is_tool_busy(&id) {
                    self.set_status(format!("{id} operation already in progress"));
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before applying tools".to_string());
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.set_status("Game install path is not configured".to_string());
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&id) else {
                    self.set_status(format!("Unknown tool: {id}"));
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                self.tool_state.active_operations.insert(id.clone());
                self.set_status(format!("Applying {}...", tool.display_name()));
                let db = self.db.clone();
                return Task::perform(
                    apply_tool_for_game(db, game_id, game_dir, id.clone(), context),
                    move |result| Message::ToolApplied {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::RevertTool(id) => {
                if self.tool_state.is_tool_busy(&id) {
                    self.set_status(format!("{id} operation already in progress"));
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before reverting tools".to_string());
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.set_status("Game install path is not configured".to_string());
                    return Task::none();
                };
                let Some(tool) = modde_games::tools::resolve_tool(&id) else {
                    self.set_status(format!("Unknown tool: {id}"));
                    return Task::none();
                };
                self.tool_state.active_operations.insert(id.clone());
                self.set_status(format!("Reverting {}...", tool.display_name()));
                let db = self.db.clone();
                return Task::perform(
                    revert_tool_for_game(db, game_id, game_dir, id.clone()),
                    move |result| Message::ToolReverted {
                        tool_id: id.clone(),
                        result,
                    },
                );
            }
            Message::ToolApplied { tool_id, result } => {
                self.tool_state.active_operations.remove(&tool_id);
                match result {
                    Ok(result) => {
                        self.tool_state.active_tool_id = Some(tool_id);
                        let validation = result
                            .validation_message
                            .map(|message| format!("; {message}"))
                            .unwrap_or_default();
                        self.set_status(format!(
                            "Applied {} ({} file(s)){validation}",
                            result.display_name, result.applied_file_count
                        ));
                        if self.tool_load_request().is_some() {
                            return self.start_tools_load();
                        }
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to apply tool: {err}"));
                    }
                }
            }
            Message::ToolReverted { tool_id, result } => {
                self.tool_state.active_operations.remove(&tool_id);
                match result {
                    Ok(result) => {
                        self.tool_state.active_tool_id = Some(tool_id);
                        self.set_status(format!("Reverted {}", result.display_name));
                        if self.tool_load_request().is_some() {
                            return self.start_tools_load();
                        }
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to revert tool: {err}"));
                    }
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
