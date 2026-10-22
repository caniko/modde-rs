#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! profile_dialog update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_profile_dialog_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Profile dialog ───────────────────────────────────
            Message::OpenNewProfileDialog => {
                self.new_profile_dialog_open = true;
            }
            Message::NewProfileNameChanged(name) => self.new_profile_name = name,
            Message::CancelNewProfileDialog => {
                self.new_profile_dialog_open = false;
                self.new_profile_name.clear();
            }
            Message::SubmitNewProfileDialog => {
                let Some(game_id) = self.selected_game.clone() else {
                    self.set_error("Select a game before creating a profile".to_string());
                    return Task::none();
                };
                let name = self.new_profile_name.trim().to_string();
                if name.is_empty() {
                    self.set_error("Profile name is required".to_string());
                    return Task::none();
                }
                return self.update(Message::CreateProfile { name, game_id });
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
