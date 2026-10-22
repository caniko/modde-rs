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
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
