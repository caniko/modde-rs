#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! collections update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_collections_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Collections ──────────────────────────────────────
            Message::SearchCollections(query) => {
                self.collection_search = query.clone();
                if query.is_empty() {
                    self.collections = Vec::new();
                    return Task::none();
                }
                self.set_status("Searching collections...".to_string());
                return Task::perform(
                    async move { Ok::<Vec<CollectionManifest>, anyhow::Error>(Vec::new()) },
                    |result| match result {
                        Ok(_) => Message::Noop,
                        Err(_) => Message::Noop,
                    },
                );
            }
            Message::InstallCollection { slug, version } => {
                self.set_status(format!("Installing collection {slug} v{version}..."));
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
