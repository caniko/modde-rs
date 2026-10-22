#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! nexus update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_nexus_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ModDetailsLoaded {
                nexus_mod_id,
                result,
            } => {
                // Guard against stale responses for a previous selection.
                let matches = self
                    .selected_mod_details
                    .as_ref()
                    .is_some_and(|s| s.nexus_mod_id == nexus_mod_id);
                if !matches {
                    return Task::none();
                }

                match result {
                    Ok(nexus_mod) => {
                        let picture_url = nexus_mod.picture_url.clone();
                        let game_domain = self
                            .selected_mod_details
                            .as_ref()
                            .map(|s| s.game_domain.clone())
                            .unwrap_or_default();

                        if let Some(ref mut s) = self.selected_mod_details {
                            s.loading = false;
                            s.name = nexus_mod.name;
                            s.author = nexus_mod.author;
                            s.version = nexus_mod.version;
                            s.summary = nexus_mod.summary;
                            s.endorse_status = nexus_mod
                                .endorsement
                                .as_ref()
                                .map(|e| e.endorse_status.clone());
                            s.endorsement_count = nexus_mod.endorsement_count;
                            if let Some(ref url) = picture_url {
                                s.gallery = vec![url.clone()];
                                s.gallery_index = 0;
                            }
                        }

                        // Fire follow-up fetches: (a) primary picture bytes,
                        // (b) full gallery via GraphQL. Both are best-effort.
                        // Key is loaded via `auth::load_api_key` inside the
                        // task so OAuth/keyring/env are all honored.
                        let mut tasks: Vec<Task<Message>> = Vec::new();

                        if let Some(url) = picture_url {
                            tasks.push(Task::perform(
                                async move {
                                    let api_key =
                                        modde_sources::nexus::auth::load_api_key().ok()?;
                                    let client = reqwest::Client::new();
                                    let api =
                                        modde_sources::nexus::api::NexusApi::new(client, api_key);
                                    api.fetch_bytes(&url).await.ok()
                                },
                                move |bytes_opt| match bytes_opt {
                                    Some(bytes) => Message::ModThumbnailLoaded {
                                        nexus_mod_id,
                                        gallery_index: 0,
                                        bytes,
                                    },
                                    None => Message::Noop,
                                },
                            ));
                        }

                        if !game_domain.is_empty() {
                            let domain = game_domain.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let api_key = modde_sources::nexus::auth::load_api_key()
                                        .unwrap_or_default();
                                    if api_key.is_empty() {
                                        return Vec::new();
                                    }
                                    let client = reqwest::Client::new();
                                    let api =
                                        modde_sources::nexus::api::NexusApi::new(client, api_key);
                                    api.get_mod_media(&domain, nexus_mod_id)
                                        .await
                                        .unwrap_or_default()
                                },
                                move |urls| Message::ModGalleryLoaded { nexus_mod_id, urls },
                            ));
                        }

                        // Check whether the current user is tracking this
                        // mod. The v1 endpoint is not filterable by mod_id,
                        // so we download the full tracked list and filter.
                        // Best-effort — on failure, the Track button stays
                        // disabled (is_tracked = None).
                        if !game_domain.is_empty() {
                            let domain = game_domain.clone();
                            tasks.push(Task::perform(
                                async move {
                                    let api_key =
                                        modde_sources::nexus::auth::load_api_key().ok()?;
                                    let client = reqwest::Client::new();
                                    let api =
                                        modde_sources::nexus::api::NexusApi::new(client, api_key);
                                    let list = api.get_tracked_mods().await.ok()?;
                                    let target = nexus_mod_id;
                                    Some(list.iter().any(|t| {
                                        t.mod_id == target
                                            && t.domain_name.eq_ignore_ascii_case(&domain)
                                    }))
                                },
                                move |is_tracked_opt| match is_tracked_opt {
                                    Some(is_tracked) => Message::ModTrackedSetLoaded {
                                        nexus_mod_id,
                                        is_tracked,
                                    },
                                    None => Message::Noop,
                                },
                            ));
                        }

                        return Task::batch(tasks);
                    }
                    Err(e) => {
                        if let Some(ref mut s) = self.selected_mod_details {
                            s.loading = false;
                            s.error = Some(e);
                        }
                    }
                }
            }
            Message::ModGalleryLoaded { nexus_mod_id, urls } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id {
                    return Task::none();
                }
                if urls.is_empty() {
                    return Task::none();
                }

                // Merge: keep the existing picture_url (gallery[0]) as the
                // first entry so the already-fetched thumbnail stays valid,
                // then append any gallery URLs not already in the list.
                let mut merged: Vec<String> = s.gallery.clone();
                for url in urls {
                    if !merged.contains(&url) {
                        merged.push(url);
                    }
                }
                s.gallery = merged;
            }
            Message::ModThumbnailLoaded {
                nexus_mod_id,
                gallery_index,
                bytes,
            } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id || s.gallery_index != gallery_index {
                    return Task::none();
                }
                s.thumbnail = Some(resize_thumbnail_bytes(&bytes));
            }
            Message::ModGalleryNext => {
                let (nexus_mod_id, next_index, url) = {
                    let Some(ref mut s) = self.selected_mod_details else {
                        return Task::none();
                    };
                    if s.gallery.len() < 2 {
                        return Task::none();
                    }
                    s.gallery_index = (s.gallery_index + 1) % s.gallery.len();
                    s.thumbnail = None;
                    let url = s.gallery[s.gallery_index].clone();
                    (s.nexus_mod_id, s.gallery_index, url)
                };
                return Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key().ok()?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        api.fetch_bytes(&url).await.ok()
                    },
                    move |bytes_opt| match bytes_opt {
                        Some(bytes) => Message::ModThumbnailLoaded {
                            nexus_mod_id,
                            gallery_index: next_index,
                            bytes,
                        },
                        None => Message::Noop,
                    },
                );
            }
            Message::OpenModPage => {
                if let Some(ref s) = self.selected_mod_details {
                    let url = s.mod_page_url.clone();
                    // Surface the URL in the status bar so the user can
                    // verify exactly what's being passed to the browser
                    // (useful for diagnosing case-sensitivity issues with
                    // historical capitalized DB records).
                    self.set_status(format!("Opening: {url}"));
                    tracing::info!(url = %url, "opening mod page in browser");
                    // Spawn on blocking pool — `open::that` forks xdg-open
                    // and usually returns quickly, but we don't want any
                    // chance of stalling the UI event loop.
                    return Task::perform(
                        async move {
                            let _ = tokio::task::spawn_blocking(move || {
                                let _ = open::that(&url);
                            })
                            .await;
                        },
                        |()| Message::Noop,
                    );
                }
            }
            Message::ModEndorseToggle => {
                // Snapshot what we need from state and optimistically
                // flip the UI before the API request returns.
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.action_pending {
                    return Task::none();
                }
                let nexus_mod_id = s.nexus_mod_id;
                let game_domain = s.game_domain.clone();
                let version = s.version.clone();
                let was_endorsed = s.endorse_status.as_deref() == Some("Endorsed");
                let new_status = if was_endorsed {
                    "Abstained"
                } else {
                    "Endorsed"
                };
                s.endorse_status = Some(new_status.to_string());
                // Adjust the visible total: +1 when going to Endorsed, -1
                // when leaving it. `saturating_sub` guards against weirdness
                // if the count happens to be 0.
                if was_endorsed {
                    s.endorsement_count = s.endorsement_count.saturating_sub(1);
                } else {
                    s.endorsement_count = s.endorsement_count.saturating_add(1);
                }
                s.action_pending = true;

                let target_status = new_status.to_string();
                return Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        if was_endorsed {
                            api.abstain_mod(&game_domain, nexus_mod_id, &version)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            api.endorse_mod(&game_domain, nexus_mod_id, &version)
                                .await
                                .map_err(|e| e.to_string())
                        }
                    },
                    move |result| Message::ModEndorseResult {
                        nexus_mod_id,
                        new_status: target_status.clone(),
                        result,
                    },
                );
            }
            Message::ModEndorseResult {
                nexus_mod_id,
                new_status,
                result,
            } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id {
                    return Task::none();
                }
                s.action_pending = false;
                match result {
                    Ok(()) => {
                        // Optimistic state already matches — nothing to do.
                        self.set_status(if new_status == "Endorsed" {
                            "Endorsed on Nexus".to_string()
                        } else {
                            "Endorsement withdrawn".to_string()
                        });
                    }
                    Err(e) => {
                        // Roll back the optimistic update.
                        let reverted = if new_status == "Endorsed" {
                            "Abstained"
                        } else {
                            "Endorsed"
                        };
                        s.endorse_status = Some(reverted.to_string());
                        if new_status == "Endorsed" {
                            s.endorsement_count = s.endorsement_count.saturating_sub(1);
                        } else {
                            s.endorsement_count = s.endorsement_count.saturating_add(1);
                        }
                        self.set_error(format!("Endorse failed: {e}"));
                    }
                }
            }
            Message::ModTrackToggle => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.action_pending {
                    return Task::none();
                }
                let nexus_mod_id = s.nexus_mod_id;
                let game_domain = s.game_domain.clone();
                // If is_tracked is None (not yet fetched), assume not
                // tracked — clicking Track will try to track, and the
                // result is idempotent enough on the server side.
                let was_tracked = s.is_tracked.unwrap_or(false);
                let new_tracked = !was_tracked;
                s.is_tracked = Some(new_tracked);
                s.action_pending = true;

                return Task::perform(
                    async move {
                        let api_key = modde_sources::nexus::auth::load_api_key()
                            .map_err(|e| e.to_string())?;
                        let client = reqwest::Client::new();
                        let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                        if was_tracked {
                            api.untrack_mod(&game_domain, nexus_mod_id)
                                .await
                                .map_err(|e| e.to_string())
                        } else {
                            api.track_mod(&game_domain, nexus_mod_id)
                                .await
                                .map_err(|e| e.to_string())
                        }
                    },
                    move |result| Message::ModTrackResult {
                        nexus_mod_id,
                        new_tracked,
                        result,
                    },
                );
            }
            Message::ModTrackResult {
                nexus_mod_id,
                new_tracked,
                result,
            } => {
                let Some(ref mut s) = self.selected_mod_details else {
                    return Task::none();
                };
                if s.nexus_mod_id != nexus_mod_id {
                    return Task::none();
                }
                s.action_pending = false;
                match result {
                    Ok(()) => {
                        self.set_status(if new_tracked {
                            "Now tracking on Nexus".to_string()
                        } else {
                            "Stopped tracking".to_string()
                        });
                    }
                    Err(e) => {
                        // Roll back the optimistic flip.
                        s.is_tracked = Some(!new_tracked);
                        self.set_error(format!("Track toggle failed: {e}"));
                    }
                }
            }
            Message::ModTrackedSetLoaded {
                nexus_mod_id,
                is_tracked,
            } => {
                if let Some(ref mut s) = self.selected_mod_details
                    && s.nexus_mod_id == nexus_mod_id
                {
                    s.is_tracked = Some(is_tracked);
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
