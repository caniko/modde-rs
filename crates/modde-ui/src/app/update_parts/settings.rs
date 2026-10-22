#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! settings update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_settings_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Settings ─────────────────────────────────────────
            Message::SetNexusApiKeyDraft(key) => {
                self.nexus_api_key_draft = key;
                self.nexus_status = None;
            }
            Message::ToggleNexusApiKeyVisibility => {
                self.nexus_api_key_visible = !self.nexus_api_key_visible;
            }
            Message::ReplaceNexusApiKey => {
                match modde_sources::nexus::auth::write_config_api_key(&self.nexus_api_key_draft) {
                    Ok(()) => {
                        self.refresh_nexus_api_key_state();
                        self.set_status("Nexus API key saved to modde config".to_string());
                        self.nexus_status = None;
                    }
                    Err(e) => {
                        self.set_error(format!("Nexus key not saved: {e}"));
                        self.nexus_status = Some(NexusAuthStatus::Invalid(e.to_string()));
                    }
                }
            }
            Message::RemoveNexusConfigKey => {
                match modde_sources::nexus::auth::delete_config_api_key() {
                    Ok(()) => {
                        self.refresh_nexus_api_key_state();
                        self.set_status("Removed modde Nexus API key config".to_string());
                        self.nexus_status = None;
                    }
                    Err(e) => {
                        self.set_error(format!("Failed to remove modde Nexus key: {e}"));
                        self.nexus_status = Some(NexusAuthStatus::Invalid(e.to_string()));
                    }
                }
            }
            Message::GamePathDraftChanged(draft) => self.game_path_draft = draft,
            Message::DownloadDirDraftChanged(draft) => self.download_dir_draft = draft,
            Message::SetGamePath { game_id, path } => {
                let path_exists = path.is_dir();
                self.game_path_draft = path.display().to_string();
                self.settings
                    .set_game_path(&GameId::from(game_id.as_str()), path);
                if path_exists {
                    self.detected_games.insert(game_id.clone());
                } else {
                    self.detected_games.remove(&game_id);
                }
                self.set_status(format!("Game path set for {game_id}"));
                self.save_settings();
            }
            Message::SetDownloadDir(path) => {
                self.download_dir_draft = path.display().to_string();
                self.set_status(format!("Download directory set to {}", path.display()));
                self.settings.download_dir = Some(path);
                self.save_settings();
            }
            Message::BrowseGamePath => {
                let Some(game_id) = self.current_game_id().map(str::to_string) else {
                    self.set_error("Select a game before setting its install path");
                    return Task::none();
                };
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Game Directory")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    move |path| match path {
                        Some(p) => Message::SetGamePath {
                            game_id: game_id.clone(),
                            path: p,
                        },
                        None => Message::Noop,
                    },
                );
            }
            Message::BrowseDownloadDir => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Download Directory")
                            .pick_folder()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::SetDownloadDir(p),
                        None => Message::Noop,
                    },
                );
            }
            Message::SetTheme(name) => {
                self.theme_name = name.clone();
                self.settings.theme = name;
                self.set_status("Theme updated".to_string());
                self.save_settings();
            }
            Message::ValidateNexusKey => {
                self.nexus_status = Some(NexusAuthStatus::Checking);
                let api_key = self.nexus_api_key_draft.trim().to_string();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || -> Result<(String, bool), String> {
                            if api_key.is_empty() {
                                return Err("No API key set".to_string());
                            }
                            let client = reqwest::blocking::Client::new();
                            let resp = client
                                .get("https://api.nexusmods.com/v1/users/validate.json")
                                .header("apikey", &api_key)
                                .send()
                                .map_err(|e| e.to_string())?;
                            if !resp.status().is_success() {
                                return Err(format!("HTTP {}", resp.status()));
                            }
                            let body: serde_json::Value = resp.json().map_err(|e| e.to_string())?;
                            let name = body["name"].as_str().unwrap_or("Unknown").to_string();
                            let is_premium = body["is_premium"].as_bool().unwrap_or(false);
                            Ok((name, is_premium))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::NexusKeyValidated,
                );
            }
            Message::NexusKeyValidated(result) => match result {
                Ok((username, is_premium)) => {
                    self.nexus_status = Some(NexusAuthStatus::Valid {
                        username: username.clone(),
                        is_premium,
                    });
                    self.set_status(format!("Nexus: logged in as {username}"));
                }
                Err(e) => {
                    self.nexus_status = Some(NexusAuthStatus::Invalid(e.clone()));
                    self.set_error(format!("Nexus key invalid: {e}"));
                }
            },
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
