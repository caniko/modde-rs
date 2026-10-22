#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! executables_misc update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_executables_misc_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::UpdateExecutableDraft { field, value } => {
                self.tool_state.executable_editor_open = true;
                match field {
                    ExecutableDraftField::Name => self.tool_state.executable_draft.name = value,
                    ExecutableDraftField::Path => {
                        self.tool_state.executable_draft.executable_path = value;
                    }
                    ExecutableDraftField::Arguments => {
                        self.tool_state.executable_draft.arguments = value;
                    }
                    ExecutableDraftField::WorkingDir => {
                        self.tool_state.executable_draft.working_dir = value;
                    }
                    ExecutableDraftField::Environment => {
                        self.tool_state.executable_draft.environment = value;
                    }
                    ExecutableDraftField::WineDllOverrides => {
                        self.tool_state.executable_draft.wine_dll_overrides = value;
                    }
                    ExecutableDraftField::OutputMod => {
                        self.tool_state.executable_draft.output_mod = value;
                    }
                }
                self.tool_state.executable_error = None;
            }
            Message::OpenExecutableEditor => {
                self.tool_state.executable_draft = ExecutableDraft::default();
                self.tool_state.executable_editor_open = true;
                self.tool_state.executable_error = None;
            }
            Message::ClearExecutableDraft => {
                self.tool_state.executable_draft = ExecutableDraft::default();
                self.tool_state.executable_editor_open = false;
                self.tool_state.executable_error = None;
            }
            Message::EditExecutable(name) => {
                if let Some(entry) = self
                    .tool_state
                    .executables
                    .iter()
                    .find(|entry| entry.name == name)
                    .cloned()
                {
                    self.tool_state.executable_draft = ExecutableDraft {
                        name: entry.name,
                        executable_path: entry.executable_path,
                        arguments: entry.arguments,
                        working_dir: entry.working_dir,
                        environment: entry.environment,
                        wine_dll_overrides: entry.wine_dll_overrides,
                        output_mod: entry.output_mod,
                    };
                    self.tool_state.executable_editor_open = true;
                    self.tool_state.executable_error = None;
                }
            }
            Message::SaveExecutable => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before saving executables".to_string());
                    return Task::none();
                };
                let row = match executable_draft_to_row(&game_id, &self.tool_state.executable_draft)
                {
                    Ok(row) => row,
                    Err(err) => {
                        self.tool_state.executable_error = Some(err.clone());
                        self.set_error(format!("Executable not saved: {err}"));
                        return Task::none();
                    }
                };
                self.set_status(format!("Saving executable '{}'...", row.name));
                return Task::perform(
                    save_executable_for_game(self.db.clone(), row),
                    Message::ExecutableSaved,
                );
            }
            Message::ExecutableSaved(result) => match result {
                Ok(message) => {
                    self.set_status(message);
                    self.tool_state.executable_error = None;
                    self.tool_state.executable_draft = ExecutableDraft::default();
                    self.tool_state.executable_editor_open = false;
                    return self.refresh_executables_or_tools();
                }
                Err(err) => {
                    self.tool_state.executable_error = Some(err.clone());
                    self.set_error(format!("Executable not saved: {err}"));
                }
            },
            Message::RemoveExecutable(name) => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before removing executables".to_string());
                    return Task::none();
                };
                self.tool_state
                    .active_executable_operations
                    .insert(name.clone());
                let db = self.db.clone();
                return Task::perform(
                    remove_executable_for_game(db, game_id, name.clone()),
                    move |result| Message::ExecutableRemoved {
                        name: name.clone(),
                        result,
                    },
                );
            }
            Message::ExecutableRemoved { name, result } => {
                self.tool_state.active_executable_operations.remove(&name);
                match result {
                    Ok(message) => {
                        self.set_status(message);
                        return self.refresh_executables_or_tools();
                    }
                    Err(err) => {
                        self.tool_state.executable_error = Some(err.clone());
                        self.set_error(format!("Executable not removed: {err}"));
                    }
                }
            }
            Message::RunExecutable(name) => {
                if self.tool_state.is_executable_busy(&name) {
                    self.set_status(format!("Executable '{name}' is already running"));
                    return Task::none();
                }
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before running executables".to_string());
                    return Task::none();
                };
                self.tool_state
                    .active_executable_operations
                    .insert(name.clone());
                self.set_status(format!("Running executable '{name}'..."));
                let profile = self.active_profile.clone();
                let db = self.db.clone();
                return Task::perform(
                    run_saved_executable_for_game(db, game_id, name.clone(), profile),
                    move |result| Message::ExecutableRunComplete {
                        name: name.clone(),
                        result,
                    },
                );
            }
            Message::ExecutableRunComplete { name, result } => {
                self.tool_state.active_executable_operations.remove(&name);
                match result {
                    Ok(message) => {
                        self.set_status(message);
                        return self.refresh_executables_or_tools();
                    }
                    Err(err) => {
                        self.tool_state.executable_error = Some(err.clone());
                        self.set_error(format!("Executable failed: {err}"));
                    }
                }
            }
            Message::BrowseExecutablePath => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select executable")
                            .pick_file()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::ExecutablePathSelected,
                );
            }
            Message::ExecutablePathSelected(path) => {
                if let Some(path) = path {
                    self.tool_state.executable_draft.executable_path = path.display().to_string();
                    self.tool_state.executable_editor_open = true;
                }
            }
            Message::BrowseExecutableWorkingDir => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select working directory")
                            .pick_folder()
                            .await
                            .map(|handle| handle.path().to_path_buf())
                    },
                    Message::ExecutableWorkingDirSelected,
                );
            }
            Message::ExecutableWorkingDirSelected(path) => {
                if let Some(path) = path {
                    self.tool_state.executable_draft.working_dir = path.display().to_string();
                    self.tool_state.executable_editor_open = true;
                }
            }
            Message::AdoptOptiScaler => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_error("Select a game before adopting OptiScaler".to_string());
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.set_status("Game install path is not configured".to_string());
                    return Task::none();
                };
                let context = self.current_tool_game_context();
                self.tool_state.active_tool_id = Some("optiscaler".to_string());
                self.set_status("Adopting OptiScaler...".to_string());
                return Task::perform(
                    adopt_optiscaler_for_game(self.db.clone(), game_id, game_dir, context),
                    |result| Message::ToolSettingWritten {
                        tool_id: "optiscaler".to_string(),
                        result,
                    },
                );
            }
            Message::RestoreOptiScalerBackup => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before restoring OptiScaler".to_string());
                    return Task::none();
                };
                let Some(game_dir) = self.current_game_dir() else {
                    self.set_status("Game install path is not configured".to_string());
                    return Task::none();
                };
                match modde_games::tools::optiscaler::restore_latest_optiscaler_backup(
                    &game_id, &game_dir,
                ) {
                    Ok(path) => {
                        self.tool_state.active_tool_id = Some("optiscaler".to_string());
                        self.set_status(format!("Restored OptiScaler backup {}", path.display()));
                        self.pending_tools_load_status_message = Some(self.status_message.clone());
                        return self.start_tools_load();
                    }
                    Err(err) => {
                        self.set_error(format!("Failed to restore OptiScaler: {err}"));
                    }
                }
            }
            Message::ResetOptiScalerConfig => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_status("Select a game before resetting OptiScaler".to_string());
                    return Task::none();
                };
                self.tool_state.active_tool_id = Some("optiscaler".to_string());
                self.set_status("Resetting OptiScaler config...".to_string());
                return Task::perform(
                    reset_optiscaler_config_for_game(self.db.clone(), game_id),
                    |result| Message::ToolSettingWritten {
                        tool_id: "optiscaler".to_string(),
                        result,
                    },
                );
            }
            // Downloads
            Message::PauseDownload(id) => {
                self.download_queue.pause(id);
                self.set_status("Download paused".to_string());
            }
            Message::ResumeDownload(id) => {
                self.download_queue.resume(id);
                if let Some(task) = self.download_queue.get_mut(id) {
                    task.state = modde_sources::queue::DownloadState::Active {
                        bytes_downloaded: task.meta.bytes_downloaded,
                        total_bytes: task.meta.total_bytes,
                    };
                    task.meta.status = "downloading".to_string();
                }
                self.set_status("Download resumed".to_string());
            }
            Message::CancelDownload(id) => {
                self.download_lookup.retain(|_, value| *value != id);
                self.download_queue.cancel(id);
                self.set_status("Download cancelled".to_string());
            }

            // Overwrite management
            Message::ClearOverwrite => {
                if let Some(profile) = &self.loaded_profile {
                    let _ = std::fs::remove_dir_all(&profile.overrides);
                    let _ = std::fs::create_dir_all(&profile.overrides);
                    self.set_status("Overrides cleared".to_string());
                }
            }
            Message::MoveOverwriteToMod(mod_name) => {
                if let Some(profile) = &self.loaded_profile {
                    let store = modde_core::paths::store_dir();
                    let dest = store.join(&mod_name);
                    if profile.overrides.exists() {
                        let _ = std::fs::create_dir_all(&dest);
                        if let Ok(files) = modde_core::fs::walk_files_relative(&profile.overrides) {
                            for (rel, src) in &files {
                                let dst = dest.join(rel);
                                if let Some(parent) = dst.parent() {
                                    let _ = std::fs::create_dir_all(parent);
                                }
                                let _ = std::fs::rename(src, &dst);
                            }
                        }
                        self.set_status(format!("Moved overrides to mod '{mod_name}'"));
                    }
                }
            }

            Message::ButtonHoverStarted { id, description } => {
                self.button_hover_toast.pending = Some(ButtonHoverToast { id, description });
                self.button_hover_toast.visible = None;
                return Task::perform(
                    async move {
                        tokio::time::sleep(BUTTON_HOVER_TOAST_DELAY).await;
                        id
                    },
                    |id| Message::ButtonHoverElapsed { id },
                );
            }
            Message::ButtonHoverElapsed { id } => {
                if self
                    .button_hover_toast
                    .pending
                    .is_some_and(|toast| toast.id == id)
                {
                    self.button_hover_toast.visible = self.button_hover_toast.pending;
                }
            }
            Message::ButtonHoverEnded { id } => {
                if self
                    .button_hover_toast
                    .pending
                    .is_some_and(|toast| toast.id == id)
                {
                    self.button_hover_toast.pending = None;
                }
                if self
                    .button_hover_toast
                    .visible
                    .is_some_and(|toast| toast.id == id)
                {
                    self.button_hover_toast.visible = None;
                }
            }

            Message::Noop => {}
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
