#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! downloads update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_downloads_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Downloads ────────────────────────────────────────
            Message::DownloadProgress { id, bytes, total } => {
                let task_id = self.track_download(&id, &id);
                if let Some(task) = self.download_queue.get_mut(task_id) {
                    task.meta.bytes_downloaded = bytes;
                    task.meta.total_bytes = Some(total);
                    task.meta.status = "downloading".to_string();
                    task.state = modde_sources::queue::DownloadState::Active {
                        bytes_downloaded: bytes,
                        total_bytes: Some(total),
                    };
                }
                let pct = if total > 0 {
                    (bytes as f64 / total as f64) * 100.0
                } else {
                    0.0
                };
                self.set_status(format!("Downloading {id}: {pct:.0}%"));
            }
            Message::DownloadComplete { id } => {
                if let Some(task_id) = self.download_lookup.get(&id).copied()
                    && let Some(task) = self.download_queue.get_mut(task_id)
                {
                    task.meta.status = "complete".to_string();
                    task.state = modde_sources::queue::DownloadState::Complete {
                        path: task.dest.clone(),
                        hash: task.expected_hash.unwrap_or(0),
                    };
                }
                self.set_status(format!("Download complete: {id}"));
            }
            Message::DownloadFailed { id, error } => {
                if let Some(task_id) = self.download_lookup.get(&id).copied()
                    && let Some(task) = self.download_queue.get_mut(task_id)
                {
                    task.meta.status = "failed".to_string();
                    task.state = modde_sources::queue::DownloadState::Failed {
                        error: error.clone(),
                    };
                }
                self.set_error(format!("Download failed ({id}): {error}"));
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
