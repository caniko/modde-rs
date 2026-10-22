#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! deploy update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_deploy_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Deploy => {
                let Some((profile_name, game_id)) = self
                    .loaded_profile
                    .as_ref()
                    .map(|profile| (profile.name.clone(), profile.game_id.clone()))
                else {
                    self.set_error("No active profile to deploy");
                    return Task::none();
                };
                self.set_status("Deploying mods...");
                {
                    let db = self.db.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                deploy_profile_blocking(db, profile_name, game_id)
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::DeployComplete,
                    );
                }
            }
            Message::DeployComplete(result) => match result {
                Ok(msg) => self.set_status(msg),
                Err(e) => self.set_error(format!("Deploy failed: {e}")),
            },
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
