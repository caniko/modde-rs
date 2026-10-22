#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! stock_game update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_stock_game_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Stock game ───────────────────────────────────────
            Message::CreateStockSnapshot => {
                self.set_status("Creating stock game snapshot...".to_string());
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> Result<String, String> {
                                let game_plugin =
                                    modde_games::resolve_game_plugin(game_id.as_str())
                                        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let install_path =
                                    game_plugin.detect_install().ok_or_else(|| {
                                        format!("could not detect install for {game_id}")
                                    })?;
                                let mgr = modde_core::stock::StockGameManager::new(
                                    modde_core::stock::StockGameManager::default_dir(),
                                );
                                let rt = tokio::runtime::Handle::current();
                                rt.block_on(mgr.snapshot(&game_id, &install_path))
                                    .map_err(|e| e.to_string())?;
                                Ok(format!("Snapshot created for {game_id}"))
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::StockSnapshotCreated,
                    );
                }
                self.set_status("No active profile".to_string());
            }
            Message::StockSnapshotCreated(result) => match result {
                Ok(msg) => {
                    self.stock_snapshot_exists = true;
                    self.set_status(msg);
                }
                Err(e) => self.set_error(format!("Snapshot failed: {e}")),
            },
            Message::VerifyStockSnapshot => {
                self.set_status("Verifying stock snapshot...".to_string());
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || -> Result<String, String> {
                                let game_plugin =
                                    modde_games::resolve_game_plugin(game_id.as_str())
                                        .ok_or_else(|| format!("unsupported game: {game_id}"))?;
                                let _install_path =
                                    game_plugin.detect_install().ok_or_else(|| {
                                        format!("could not detect install for {game_id}")
                                    })?;
                                let mgr = modde_core::stock::StockGameManager::new(
                                    modde_core::stock::StockGameManager::default_dir(),
                                );
                                let rt = tokio::runtime::Handle::current();
                                match rt.block_on(mgr.verify(&game_id)) {
                                    Ok(true) => Ok("Stock snapshot verified: OK".to_string()),
                                    Ok(false) => Ok("Stock snapshot MODIFIED".to_string()),
                                    Err(e) => Err(e.to_string()),
                                }
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::StockVerifyResult,
                    );
                }
            }
            Message::StockVerifyResult(result) => match result {
                Ok(msg) => self.set_status(msg),
                Err(e) => self.set_error(format!("Verify failed: {e}")),
            },
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
