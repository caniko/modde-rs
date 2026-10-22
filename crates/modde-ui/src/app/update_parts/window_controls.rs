#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! window_controls update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_window_controls_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Window controls (custom title bar) ───────────────
            Message::GotWindowId(Some(id)) => {
                self.window_id = id;
            }
            Message::GotWindowId(None) => {}
            Message::TitleBarDrag => {
                return window::drag(self.window_id);
            }
            Message::WindowMinimize => {
                return window::minimize(self.window_id, true);
            }
            Message::WindowToggleMaximize => {
                return window::toggle_maximize(self.window_id);
            }
            Message::WindowClose => {
                let busy = matches!(&self.active_view, View::WabbajackInstaller(state) if state.installing)
                    || !self.library.launching.is_empty();
                if busy && self.pending_confirm.is_none() {
                    self.pending_confirm = Some(crate::app::PendingConfirm {
                        title: "Quit while busy?",
                        body: "An install or game session is still running. Quit anyway?"
                            .to_string(),
                        on_confirm: Message::ForceWindowClose,
                    });
                    return Task::none();
                }
                return window::close(self.window_id);
            }
            Message::ForceWindowClose => {
                return window::close(self.window_id);
            }
            Message::Shortcut(action) => return self.handle_shortcut(action),
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}

impl Modde {
    /// Resolve a matched keyboard shortcut against the current state.
    fn handle_shortcut(&mut self, action: &'static str) -> Task<Message> {
        let message = match action {
            "deploy" => Message::Deploy,
            "refresh" => Message::ExternalRefresh,
            "open_info" => Message::OpenModPage,
            "focus_filter" => {
                return iced::widget::operation::focus(crate::semantics::widget_id(
                    "mod_list.filter",
                ));
            }
            // Topmost layer first, mirroring the stacking order in `view`.
            "dismiss_modal" => {
                if self.pending_confirm.is_some() {
                    Message::ConfirmCancelled
                } else if self.manage_custom_games_dialog_open {
                    Message::CloseManageCustomGames
                } else if self.add_custom_game_dialog_open {
                    Message::AddCustomGameCancel
                } else if self.game_path_dialog_open {
                    Message::CancelGamePathDialog
                } else if self.new_profile_dialog_open {
                    Message::CancelNewProfileDialog
                } else if matches!(self.active_view, View::FOMODWizard(_)) {
                    Message::FOMODCancel
                } else {
                    return Task::none();
                }
            }
            "remove_selected" | "toggle_selected" => {
                if !matches!(self.active_view, View::ModList) {
                    return Task::none();
                }
                let Some(index) = self.selected_mod_index else {
                    return Task::none();
                };
                let Some(selected) = self.loaded_profile.as_ref().and_then(|p| p.mods.get(index))
                else {
                    return Task::none();
                };
                if action == "remove_selected" {
                    let name = selected.display_name.as_deref().unwrap_or(&selected.mod_id);
                    Message::RequestConfirm(Box::new(crate::app::PendingConfirm {
                        title: "Remove mod",
                        body: format!("Remove '{name}' from this profile?"),
                        on_confirm: Message::RemoveMod(index),
                    }))
                } else {
                    Message::ToggleMod {
                        mod_id: selected.mod_id.clone(),
                        enabled: !selected.enabled,
                    }
                }
            }
            _ => return Task::none(),
        };
        self.update(message)
    }
}
