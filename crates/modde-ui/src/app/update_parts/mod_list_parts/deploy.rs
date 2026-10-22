#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! deploy update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_deploy_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Deploy => {
                self.set_status("Deploying mods...".to_string());
                if let Some(ref profile) = self.loaded_profile {
                    let profile_name = profile.name.clone();
                    let game_id = profile.game_id.clone();
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
