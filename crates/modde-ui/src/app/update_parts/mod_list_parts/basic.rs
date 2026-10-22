#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! basic update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_basic_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Mod list ─────────────────────────────────────────
            Message::ToggleMod { mod_id, enabled } => {
                if let Some(ref profile_name) = self.active_profile {
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let profile_name = profile_name.clone();
                    self.set_status(format!(
                        "{} '{mod_id}'...",
                        if enabled { "Enabling" } else { "Disabling" }
                    ));
                    return Task::perform(
                        toggle_mod_enabled(self.db.clone(), profile_name, mod_id.clone(), enabled),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::ToggleMod {
                                mod_id: mod_id.clone(),
                                enabled,
                            },
                            result,
                        },
                    );
                }
            }
            Message::FilterChanged(filter) => self.mod_filter = filter,
            Message::ToggleFilterMode => {
                self.filter_mode = self.filter_mode.toggle();
            }
            Message::CycleFilter(kind) => {
                if let Some(c) = self.filter_criteria.iter_mut().find(|c| c.kind == kind) {
                    c.state = c.state.cycle();
                }
            }
            Message::ClearFilters => {
                for c in &mut self.filter_criteria {
                    c.state = modde_core::filter::TriState::Ignore;
                }
            }
            Message::ToggleCompactModList => {
                self.compact_mod_list = !self.compact_mod_list;
            }
            Message::ToggleSeparator(cat_id) => {
                if !self.collapsed_categories.remove(&cat_id) {
                    self.collapsed_categories.insert(cat_id);
                }
            }
            Message::AddMod => {
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("Select Mod Archive or Directory")
                            .add_filter("Archives", &["zip", "7z", "rar"])
                            .pick_file()
                            .await
                            .map(|h| h.path().to_path_buf())
                    },
                    |path| match path {
                        Some(p) => Message::AddModFromPath(p),
                        None => Message::Noop,
                    },
                );
            }
            Message::AddModFromPath(path) => {
                if let Some(ref profile_name) = self.active_profile {
                    let mod_name = path.file_stem().map_or_else(
                        || "unknown-mod".to_string(),
                        |s| s.to_string_lossy().to_string(),
                    );
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let profile_name = profile_name.clone();
                    self.set_status(format!("Adding mod: {mod_name}..."));
                    return Task::perform(
                        add_mod_to_profile(self.db.clone(), profile_name, mod_name.clone()),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::AddMod {
                                mod_id: mod_name.clone(),
                            },
                            result,
                        },
                    );
                }
                self.set_status("No active profile — create one first".to_string());
            }
            Message::RemoveMod(index) => {
                if let Some(ref profile_name) = self.active_profile {
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let profile_name = profile_name.clone();
                    self.set_status("Removing mod...".to_string());
                    return Task::perform(
                        remove_mod_from_profile(self.db.clone(), profile_name, index),
                        move |result| Message::ProfileWriteDone {
                            generation,
                            kind: ProfileWriteKind::RemoveMod,
                            result,
                        },
                    );
                }
            }
            Message::SelectMod(index) => {
                self.selected_mod_index = Some(index);

                // Look up the selected mod and, if it carries Nexus metadata,
                // kick off an async fetch for its full details. Otherwise
                // clear the detail panel.
                //
                // Nexus game domains are canonically lowercase (e.g.
                // "cyberpunk2077"). Historical DB records came from URL path
                // segments and may be mixed-case, which causes Nexus v1 to
                // return 401 Unauthorized rather than 404 — so we lowercase
                // defensively before any API call.
                let nexus_info = self
                    .loaded_profile
                    .as_ref()
                    .and_then(|p| p.mods.get(index))
                    .and_then(|m| {
                        let nid = m.nexus_mod_id?;
                        let domain = m.nexus_game_domain.clone()?.to_lowercase();
                        Some((
                            nid,
                            domain,
                            m.display_name.clone().unwrap_or_else(|| m.mod_id.clone()),
                            m.version.clone().unwrap_or_default(),
                        ))
                    });

                match nexus_info {
                    Some((nexus_mod_id, game_domain, name, version)) => {
                        self.selected_mod_details =
                            Some(crate::views::mod_details::ModDetailsState::loading(
                                nexus_mod_id,
                                game_domain.clone(),
                                name,
                                version,
                            ));

                        return Task::perform(
                            async move {
                                let api_key = modde_sources::nexus::auth::load_api_key()
                                    .map_err(|e| e.to_string())?;
                                let client = reqwest::Client::new();
                                let api = modde_sources::nexus::api::NexusApi::new(client, api_key);
                                api.get_mod(&game_domain, nexus_mod_id)
                                    .await
                                    .map_err(|e| e.to_string())
                            },
                            move |result| Message::ModDetailsLoaded {
                                nexus_mod_id,
                                result,
                            },
                        );
                    }
                    None => {
                        self.selected_mod_details = None;
                    }
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
