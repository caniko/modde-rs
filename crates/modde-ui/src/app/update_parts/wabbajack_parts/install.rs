#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! install update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_install_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WabbajackGenerateHmSnippet => {
                let (source, profile, game, game_dir) =
                    if let View::WabbajackInstaller(ref state) = self.active_view {
                        (
                            state.manual_source.clone(),
                            state.hm_profile.clone(),
                            state.hm_game.clone(),
                            state.hm_game_dir.clone(),
                        )
                    } else {
                        (String::new(), String::new(), String::new(), String::new())
                    };
                if source.is_empty() || profile.is_empty() || game.is_empty() {
                    self.set_status(
                        "Wabbajack source, HM profile, and game are required".to_string(),
                    );
                    return Task::none();
                }
                return Task::perform(
                    async move {
                        let client = reqwest::Client::new();
                        let cache_dir = modde_core::paths::downloads_dir().join("wabbajack");
                        let game_dir = (!game_dir.is_empty()).then(|| PathBuf::from(game_dir));
                        modde_sources::wabbajack::catalog::hm_snippet_for_source(
                            &client,
                            &source,
                            &profile,
                            &game,
                            game_dir.as_deref(),
                            &cache_dir,
                        )
                        .await
                        .map(|(snippet, _)| snippet)
                        .map_err(format_anyhow_error)
                    },
                    Message::WabbajackHmSnippetGenerated,
                );
            }
            Message::WabbajackHmSnippetGenerated(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(snippet) => {
                            state.hm_snippet = snippet;
                            state.status = "Generated Home Manager snippet".to_string();
                        }
                        Err(e) => {
                            state.status = format!("HM snippet failed: {e}");
                        }
                    }
                }
            }
            Message::WabbajackCopyHmSnippet => {
                if let View::WabbajackInstaller(ref state) = self.active_view
                    && !state.hm_snippet.is_empty()
                {
                    let snippet = state.hm_snippet.clone();
                    self.set_status("Copied Home Manager snippet");
                    return iced::clipboard::write(snippet);
                }
            }
            Message::WabbajackSaveHmSnippet => {
                let snippet = if let View::WabbajackInstaller(ref state) = self.active_view {
                    state.hm_snippet.clone()
                } else {
                    String::new()
                };
                if snippet.is_empty() {
                    self.set_status("Generate a Home Manager snippet first".to_string());
                    return Task::none();
                }
                return Task::perform(
                    async move {
                        let file = rfd::AsyncFileDialog::new()
                            .set_title("Save Home Manager snippet")
                            .set_file_name("modde-wabbajack.nix")
                            .save_file()
                            .await
                            .map(|h| h.path().to_path_buf());
                        let Some(path) = file else {
                            return Err("Save cancelled".to_string());
                        };
                        tokio::fs::write(&path, snippet)
                            .await
                            .map_err(|e| e.to_string())?;
                        Ok(path)
                    },
                    Message::WabbajackHmSnippetSaved,
                );
            }
            Message::WabbajackHmSnippetSaved(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(path) => {
                            state.status = format!("Saved snippet to {}", path.display());
                        }
                        Err(e) => {
                            state.status = e;
                        }
                    }
                }
            }
            Message::WabbajackOpenUrl(url) => {
                if let Err(e) = open::that(&url) {
                    self.set_error(format!("Failed to open {url}: {e}"));
                }
            }
            Message::OpenWabbajackFile => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select .wabbajack File")
                            .add_filter("Wabbajack", &["wabbajack"])
                            .pick_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::WabbajackFileSelected(p),
                        None => Message::Noop,
                    },
                );
            }
            Message::WabbajackFileSelected(path) => {
                let manifest =
                    modde_sources::wabbajack::runner::parse_wabbajack_manifest(&path).ok();
                self.wabbajack_manifest = manifest;
                let mut should_recheck = false;
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.file_path = Some(path.clone());
                    state.downloaded_path = Some(path.clone());
                    state.manual_source = path.display().to_string();
                    state.readiness = None;
                    state.readiness_error = None;
                    state.archive_import_results.clear();
                    state.archive_import_status = None;
                    state.progress = 0.0;
                    state.status = format!("Selected: {}", path.display());
                    state
                        .log_lines
                        .push(format!("File selected: {}", path.display()));
                    if let Some(manifest) = &self.wabbajack_manifest {
                        state.hm_profile = slugify_profile_name(&manifest.name);
                        state.hm_game = modde_games::normalize_wabbajack_game(&manifest.game)
                            .unwrap_or(&manifest.game)
                            .to_string();
                        state.hm_game_dir_user_edited = false;
                        prefill_wabbajack_game_dir(&self.settings, state);
                    }
                    should_recheck = true;
                }
                self.set_status(format!("Wabbajack file loaded: {}", path.display()));
                if should_recheck {
                    return self.update(Message::WabbajackCheckReadiness);
                }
            }
            Message::WabbajackProgress(progress) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.progress = progress;
                    state.status = format!("{:.0}% complete", progress * 100.0);
                }
            }
            Message::WabbajackStartInstall => {
                let current_game_dir = self.current_game_dir();
                let (path, profile_name, game_dir) =
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        if let Some(blocker) = state.install_blocker() {
                            state.status = blocker.clone();
                            self.set_status(blocker);
                            return Task::none();
                        }
                        let Some(path) = state.file_path.clone() else {
                            self.set_status("No wabbajack file selected".to_string());
                            return Task::none();
                        };
                        state.status = "Starting installation...".to_string();
                        state.installing = true;
                        state.install_phase = "Starting".to_string();
                        state.install_current_item.clear();
                        state.log_lines.push("Installation started".to_string());
                        state.progress = 0.0;
                        (
                            path,
                            (!state.hm_profile.trim().is_empty())
                                .then(|| state.hm_profile.trim().to_string())
                                .or_else(|| self.active_profile.clone()),
                            if state.hm_game_dir.trim().is_empty() {
                                current_game_dir
                            } else {
                                Some(PathBuf::from(state.hm_game_dir.trim()))
                            },
                        )
                    } else {
                        self.set_status("No wabbajack file selected".to_string());
                        return Task::none();
                    };
                let db = self.db.clone();
                return Task::run(
                    run_wabbajack_install_for_ui_stream(db, path, profile_name, game_dir),
                    Message::WabbajackInstallEvent,
                );
            }
            Message::WabbajackInstallEvent(event) => match event {
                WabbajackInstallEvent::Progress(progress) => {
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        let line = format_install_progress(&progress);
                        apply_wabbajack_progress_state(state, &progress, line.clone());
                        state.log_lines.push(line);
                    }
                }
                WabbajackInstallEvent::Complete(result) => {
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        state.installing = false;
                        match result {
                            Ok(summary) => {
                                state.progress = 1.0;
                                state.install_phase = "Complete".to_string();
                                state.install_current_item.clear();
                                state.status = summary.status_message.clone();
                                state.log_lines.push(summary.status_message.clone());
                                self.set_status(summary.status_message);
                                return self.reload_profile();
                            }
                            Err(error) => {
                                state.install_phase = "Failed".to_string();
                                state.install_current_item.clear();
                                state.status = format!("Install failed: {error}");
                                let message = state.status.clone();
                                state.log_lines.push(message.clone());
                                self.set_error(message);
                            }
                        }
                    }
                }
            },
            Message::WabbajackInstallComplete(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.installing = false;
                    match result {
                        Ok((summary, lines)) => {
                            state.progress = 1.0;
                            state.install_phase = "Complete".to_string();
                            state.install_current_item.clear();
                            state.status = summary.clone();
                            state.log_lines.extend(lines);
                            self.set_status(summary);
                            return self.reload_profile();
                        }
                        Err(e) => {
                            state.install_phase = "Failed".to_string();
                            state.install_current_item.clear();
                            state.status = format!("Install failed: {e}");
                            let message = state.status.clone();
                            state.log_lines.push(message.clone());
                            self.set_error(message);
                        }
                    }
                }
            }
            Message::WabbajackLog(line) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.log_lines.push(line.clone());
                    state.status = line;
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
