#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! setup update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_setup_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Wabbajack ────────────────────────────────────────
            Message::LoadWabbajackCatalog => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.loading = true;
                    state.error = None;
                    state.status = "Loading Wabbajack catalogs...".to_string();
                }
                return Task::perform(
                    async {
                        let client = reqwest::Client::new();
                        modde_sources::wabbajack::catalog::fetch_catalog(
                            &client,
                            modde_sources::wabbajack::catalog::CatalogSource::Both,
                        )
                        .await
                        .map_err(|e| e.to_string())
                    },
                    Message::WabbajackCatalogLoaded,
                );
            }
            Message::WabbajackCatalogLoaded(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.loading = false;
                    match result {
                        Ok(entries) => {
                            state.status = format!("Loaded {} Wabbajack entries", entries.len());
                            state.entries = entries;
                            state.error = None;
                        }
                        Err(e) => {
                            state.status = format!("Failed to load catalog: {e}");
                            state.error = Some(e);
                        }
                    }
                }
            }
            Message::WabbajackTabChanged(tab) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.tab = tab;
                    state.selected_index = None;
                }
            }
            Message::WabbajackSearchChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.search = value;
                }
            }
            Message::WabbajackGameFilterChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.game_filter = value;
                    state.game_filter_user_edited = true;
                }
            }
            Message::WabbajackToggleOfficialOnly(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.official_only = value;
                }
            }
            Message::WabbajackToggleNsfw(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.include_nsfw = value;
                }
            }
            Message::WabbajackToggleDown(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.include_down = value;
                }
            }
            Message::WabbajackSelectEntry(index) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.selected_index = Some(index);
                    if let Some(entry) = state.entries.get(index) {
                        state.manual_source = entry.download_url.clone();
                        state.hm_profile = slugify_profile_name(&entry.title);
                        if let Some(game) = &entry.game {
                            state.hm_game = modde_games::normalize_wabbajack_game(game)
                                .unwrap_or(game)
                                .to_string();
                            state.hm_game_dir_user_edited = false;
                            prefill_wabbajack_game_dir(&self.settings, state);
                        }
                    }
                }
            }
            Message::WabbajackManualSourceChanged(value) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.manual_source = value;
                }
            }
            Message::WabbajackHmProfileChanged(value) => {
                let should_recheck =
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        state.hm_profile = value;
                        state.file_path.is_some()
                    } else {
                        false
                    };
                if should_recheck {
                    return self.update(Message::WabbajackCheckReadiness);
                }
            }
            Message::WabbajackHmGameChanged(value) => {
                let should_recheck =
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        state.hm_game = value;
                        prefill_wabbajack_game_dir(&self.settings, state);
                        state.file_path.is_some()
                    } else {
                        false
                    };
                if should_recheck {
                    return self.update(Message::WabbajackCheckReadiness);
                }
            }
            Message::WabbajackHmGameDirChanged(value) => {
                let should_recheck =
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        state.hm_game_dir = value;
                        state.hm_game_dir_user_edited = true;
                        state.file_path.is_some()
                    } else {
                        false
                    };
                if should_recheck {
                    return self.update(Message::WabbajackCheckReadiness);
                }
            }
            Message::WabbajackDownloadSelected => {
                let source = if let View::WabbajackInstaller(ref state) = self.active_view {
                    state.manual_source.clone()
                } else {
                    String::new()
                };
                if source.is_empty() {
                    self.set_status("Enter or select a .wabbajack source first".to_string());
                    return Task::none();
                }
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.status = "Downloading .wabbajack file...".to_string();
                }
                return Task::perform(
                    async move { download_wabbajack_source(source).await },
                    Message::WabbajackDownloadComplete,
                );
            }
            Message::WabbajackDownloadComplete(result) => {
                let mut should_recheck = false;
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(path) => {
                            state.downloaded_path = Some(path.clone());
                            state.file_path = Some(path.clone());
                            state.readiness = None;
                            state.readiness_error = None;
                            state.archive_import_results.clear();
                            state.archive_import_status = None;
                            state.status = format!("Downloaded {}", path.display());
                            state.log_lines.push(state.status.clone());
                            self.wabbajack_manifest =
                                modde_sources::wabbajack::runner::parse_wabbajack_manifest(&path)
                                    .ok();
                            if let Some(manifest) = &self.wabbajack_manifest {
                                state.hm_profile = slugify_profile_name(&manifest.name);
                                state.hm_game =
                                    modde_games::normalize_wabbajack_game(&manifest.game)
                                        .unwrap_or(&manifest.game)
                                        .to_string();
                                state.hm_game_dir_user_edited = false;
                                prefill_wabbajack_game_dir(&self.settings, state);
                            }
                            should_recheck = true;
                        }
                        Err(e) => {
                            state.status = format!("Download failed: {e}");
                            state.log_lines.push(state.status.clone());
                        }
                    }
                }
                if should_recheck {
                    return self.update(Message::WabbajackCheckReadiness);
                }
            }
            Message::WabbajackCheckReadiness => {
                let fallback_game_dir = self.current_game_dir();
                let (path, profile_name, game_dir) =
                    if let View::WabbajackInstaller(ref mut state) = self.active_view {
                        let Some(path) = state.file_path.clone() else {
                            self.set_status(
                                "Select or download a .wabbajack file first".to_string(),
                            );
                            return Task::none();
                        };
                        state.readiness_loading = true;
                        state.readiness_error = None;
                        state.status = "Checking Wabbajack readiness...".to_string();
                        (
                            path,
                            (!state.hm_profile.trim().is_empty())
                                .then(|| state.hm_profile.trim().to_string()),
                            if state.hm_game_dir.trim().is_empty() {
                                fallback_game_dir
                            } else {
                                Some(PathBuf::from(state.hm_game_dir.trim()))
                            },
                        )
                    } else {
                        self.set_status("No Wabbajack installer view is active".to_string());
                        return Task::none();
                    };
                return Task::perform(
                    async move { assess_wabbajack_readiness_for_ui(path, profile_name, game_dir).await },
                    Message::WabbajackReadinessLoaded,
                );
            }
            Message::WabbajackReadinessLoaded(result) => {
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.readiness_loading = false;
                    match result {
                        Ok(report) => {
                            let ready = report.install_ready;
                            let hard_count = report.hard_blockers.len();
                            let manual_count = report.manual_downloads.len();
                            state.readiness = Some(report);
                            state.readiness_error = None;
                            state.status = if ready {
                                "Ready for Wabbajack install".to_string()
                            } else if hard_count > 0 {
                                format!("{hard_count} readiness blocker(s) must be fixed")
                            } else {
                                format!("{manual_count} manual archive(s) must be imported")
                            };
                            state.log_lines.push(state.status.clone());
                        }
                        Err(error) => {
                            state.readiness = None;
                            state.readiness_error = Some(error.clone());
                            state.status = format!("Readiness check failed: {error}");
                            state.log_lines.push(state.status.clone());
                        }
                    }
                }
            }
            Message::WabbajackImportArchives => {
                let manifest_path = if let View::WabbajackInstaller(ref state) = self.active_view {
                    let Some(path) = state.file_path.clone() else {
                        self.set_status(
                            "Select a .wabbajack file before importing archives".to_string(),
                        );
                        return Task::none();
                    };
                    path
                } else {
                    self.set_status("No Wabbajack installer view is active".to_string());
                    return Task::none();
                };
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    state.archive_import_status =
                        Some("Waiting for archive selection...".to_string());
                }
                return Task::perform(
                    async move {
                        let files = rfd::AsyncFileDialog::new()
                            .set_title("Import Wabbajack manual archives")
                            .pick_files()
                            .await
                            .map(|handles| {
                                handles
                                    .into_iter()
                                    .map(|handle| handle.path().to_path_buf())
                                    .collect::<Vec<_>>()
                            });
                        let Some(files) = files else {
                            return Err("Import cancelled".to_string());
                        };
                        if files.is_empty() {
                            return Err("No archives selected".to_string());
                        }
                        import_wabbajack_archives_for_ui(manifest_path, files).await
                    },
                    Message::WabbajackArchivesImported,
                );
            }
            Message::WabbajackArchivesImported(result) => {
                let mut should_recheck = false;
                if let View::WabbajackInstaller(ref mut state) = self.active_view {
                    match result {
                        Ok(results) => {
                            let accepted = results
                                .iter()
                                .filter(|result| {
                                    matches!(
                                        result.status,
                                        modde_sources::wabbajack::import::ArchiveImportStatus::Imported
                                            | modde_sources::wabbajack::import::ArchiveImportStatus::AlreadyPresent
                                    )
                                })
                                .count();
                            let rejected = results.len().saturating_sub(accepted);
                            state.archive_import_results = results;
                            state.archive_import_status = Some(format!(
                                "Imported {accepted} archive(s); {rejected} rejected by hash"
                            ));
                            state
                                .log_lines
                                .push(state.archive_import_status.clone().unwrap_or_default());
                            should_recheck = true;
                        }
                        Err(error) => {
                            state.archive_import_status = Some(error.clone());
                            state.status = error;
                        }
                    }
                }
                if should_recheck {
                    return self.update(Message::WabbajackCheckReadiness);
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
