#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! system update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_system_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // update apply, uninstall, …). Re-read the profile from
            // the shared sqlite DB so the user sees the change without
            // restarting or switching profiles.
            Message::ExternalRefresh => {
                let task = self.reload_profile();
                self.status_message = "Refreshed from external change".to_string();
                return Task::batch([task, self.start_library_load()]);
            }
            Message::ProfileContextLoaded { generation, result } => {
                // Drop stale loads: a newer kickoff bumped the generation, so
                // this result is for a game/profile the user has moved on from.
                if generation != self.context_generation {
                    return Task::none();
                }
                match result {
                    Ok(snapshot) => {
                        let rerun_diagnostics = snapshot.rerun_diagnostics;
                        self.apply_profile_context(snapshot);
                        if rerun_diagnostics && matches!(self.active_view, View::Diagnostics) {
                            return self.update(Message::RunDiagnostics);
                        }
                    }
                    Err(err) => {
                        self.tool_state.loading = false;
                        self.status_message = format!("Failed to load profile: {err}");
                    }
                }
            }
            Message::DataTabConflictsLoaded { generation, result } => {
                if generation != self.data_tab_generation {
                    return Task::none();
                }
                match result {
                    Ok(conflicts) => {
                        self.data_tab_conflicts = conflicts.conflicts;
                        self.data_tab_state.missing_store_mod_count =
                            conflicts.missing_store_mod_count;
                    }
                    Err(err) => {
                        self.data_tab_conflicts.clear();
                        self.data_tab_state.missing_store_mod_count = 0;
                        self.status_message = err;
                    }
                }
            }
            Message::UpdateCheckLoaded(result) => match result {
                Ok(update) => {
                    self.update_available = update;
                }
                Err(error) => {
                    tracing::debug!(%error, "GUI update check failed");
                }
            },
            Message::OpenUpdateReleasePage => {
                if let Some(update) = self.update_available.clone() {
                    return Task::perform(
                        async move {
                            let _ = open::that(update.release_url);
                        },
                        |()| Message::Noop,
                    );
                }
            }
            Message::DismissUpdateBanner => {
                self.update_available = None;
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
