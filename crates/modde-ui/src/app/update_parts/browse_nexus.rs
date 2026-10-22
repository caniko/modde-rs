#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! browse_nexus update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_browse_nexus_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Browse Nexus (Phase 6) ───────────────────────────
            Message::BrowseTabSwitched(tab) => {
                self.sync_browse_game_to_current(false);
                self.browse_nexus.active_tab = tab;
                self.browse_nexus.error = None;
                let domain = match self.browse_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                return self.spawn_browse_load(tab, domain, self.browse_nexus.search_query.clone());
            }
            Message::BrowseGameChanged(game_id) => {
                self.browse_nexus.selected_game_id = game_id;
                self.clear_browse_results();
                let domain = match self.browse_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                let tab = self.browse_nexus.active_tab;
                let query = self.browse_nexus.search_query.clone();
                return self.spawn_browse_load(tab, domain, query);
            }
            Message::BrowseSearchChanged(query) => {
                self.browse_nexus.search_query = query;
            }
            Message::BrowseSearchSubmit => {
                self.sync_browse_game_to_current(false);
                self.browse_nexus.active_tab = crate::views::browse_nexus::BrowseTab::Search;
                self.browse_nexus.error = None;
                let domain = match self.browse_game_nexus_domain() {
                    Some(d) => d,
                    None => return Task::none(),
                };
                let tab = self.browse_nexus.active_tab;
                let query = self.browse_nexus.search_query.clone();
                return self.spawn_browse_load(tab, domain, query);
            }
            Message::BrowseModsLoaded(result) => {
                self.browse_nexus.loading = false;
                match result {
                    Ok(mods) => {
                        self.browse_nexus.mods = mods;
                        self.browse_nexus.error = None;
                    }
                    Err(e) => {
                        self.browse_nexus.mods.clear();
                        self.browse_nexus.error = Some(e);
                    }
                }
            }
            Message::BrowseCollectionsLoaded(result) => {
                self.browse_nexus.loading = false;
                match result {
                    Ok(cols) => {
                        self.browse_nexus.collections = cols;
                        self.browse_nexus.error = None;
                    }
                    Err(e) => {
                        self.browse_nexus.collections.clear();
                        self.browse_nexus.error = Some(e);
                    }
                }
            }
            Message::BrowseInstallMod {
                game_domain,
                mod_id,
            } => {
                self.browse_nexus.install_status = Some(format!("Installing mod {mod_id}…"));
                let download_key = format!("browse:{game_domain}:{mod_id}");
                let task_id = self.track_download(&download_key, &format!("Nexus mod {mod_id}"));
                if let Some(task) = self.download_queue.get_mut(task_id) {
                    task.state = modde_sources::queue::DownloadState::Active {
                        bytes_downloaded: 0,
                        total_bytes: None,
                    };
                    task.meta.status = "installing".to_string();
                }
                let db = self.db.clone();
                return Task::perform(
                    async move { run_browse_install(db, game_domain, mod_id).await },
                    move |result| Message::BrowseInstallResult {
                        download_key: download_key.clone(),
                        result,
                    },
                );
            }
            Message::BrowseInstallResult {
                download_key,
                result,
            } => match result {
                Ok(msg) => {
                    self.browse_nexus.install_status = Some(msg.clone());
                    self.set_status(msg);
                    if let Some(task_id) = self.download_lookup.get(&download_key).copied()
                        && let Some(task) = self.download_queue.get_mut(task_id)
                    {
                        task.state = modde_sources::queue::DownloadState::Complete {
                            path: task.dest.clone(),
                            hash: 0,
                        };
                        task.meta.status = "complete".to_string();
                    }
                    return self.reload_profile();
                }
                Err(e) => {
                    self.browse_nexus.install_status = Some(format!("Install failed: {e}"));
                    self.set_error(format!("Install failed: {e}"));
                    if let Some(task_id) = self.download_lookup.get(&download_key).copied()
                        && let Some(task) = self.download_queue.get_mut(task_id)
                    {
                        task.state =
                            modde_sources::queue::DownloadState::Failed { error: e.clone() };
                        task.meta.status = "failed".to_string();
                    }
                }
            },
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
