#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! Library catalogue and persisted exact-install launch handlers.

use std::path::Path;

use super::*;
use crate::views::library::{
    LaunchDraft, LibraryLoadResult, build_catalogue_entries, build_manager_entries,
    merge_library_entries, parse_manager_list_json,
};
use modde_core::library::{LibraryPreferences, PendingSession};

impl Modde {
    pub(super) fn handle_library_update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LibrarySessionTick => {
                self.library.refresh_ticks = self.library.refresh_ticks.wrapping_add(1);
                if self.library.refresh_ticks >= 30 {
                    self.library.refresh_ticks = 0;
                    if matches!(self.active_view, View::Library)
                        && !self.library.loading
                        && !self.library.saving
                        && !self.library.editing
                    {
                        return self.start_library_load();
                    }
                }
                if self.library.coordination.polling {
                    return Task::none();
                }
                self.library.coordination.polling = true;
                let revision = self.library.coordination.revision;
                return Task::perform(
                    async {
                        tokio::task::spawn_blocking(|| {
                            PendingSession::load_blocking().map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    move |result| Message::LibrarySessionLoaded { revision, result },
                );
            }
            Message::LibrarySessionLoaded { revision, result } => {
                self.library.coordination.polling = false;
                if revision != self.library.coordination.revision {
                    return Task::none();
                }
                match result {
                    Ok(pending) => {
                        self.library.pending = pending;
                        self.library.coordination.revision =
                            self.library.coordination.revision.wrapping_add(1);
                    }
                    Err(error) => self.library.load_error = Some(error),
                }
            }
            Message::LibraryHookChanged(enabled) => {
                self.library.edit_draft(|draft| draft.store_hook = enabled);
            }
            Message::LibraryBrowseLaunch(field) => {
                let Some(draft) = &self.library.draft else {
                    return Task::none();
                };
                let id = draft.id.clone();
                let revision = self.library.draft_revision;
                return Task::perform(
                    async move {
                        let dialog = rfd::AsyncFileDialog::new().set_title("Choose launch path");
                        let file = if matches!(
                            field,
                            crate::views::library::LaunchField::Executable
                                | crate::views::library::LaunchField::Runner
                        ) {
                            dialog.pick_file().await
                        } else {
                            dialog.pick_folder().await
                        };
                        file.map(|file| file.path().to_path_buf())
                    },
                    move |path| Message::LibraryPathPicked {
                        id: id.clone(),
                        revision,
                        field,
                        path,
                    },
                );
            }
            Message::LibraryPathPicked {
                id,
                revision,
                field,
                path,
            } => {
                if self.library.draft_matches(&id, revision)
                    && let Some(path) = path
                {
                    self.library
                        .edit_draft(|draft| draft.set(field, path.to_string_lossy().into_owned()));
                }
            }
            Message::LibraryImportLaunch => {
                let Some(draft) = &self.library.draft else {
                    return Task::none();
                };
                let id = draft.id.clone();
                let revision = self.library.draft_revision;
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .add_filter("Launch settings JSON", &["json"])
                            .pick_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        serde_json::from_slice(&file.read().await)
                            .map(Some)
                            .map_err(|error| format!("Invalid launch settings: {error}"))
                    },
                    move |result| Message::LibraryLaunchImported {
                        id: id.clone(),
                        revision,
                        result,
                    },
                );
            }
            Message::LibraryLaunchImported {
                id,
                revision,
                result,
            } => {
                if !self.library.draft_matches(&id, revision)
                    || self.library.saving
                    || !self.library.launching.is_empty()
                    || self.library.pending.is_some()
                {
                    return Task::none();
                }
                match result {
                    Ok(Some(settings)) => {
                        let mut draft = LaunchDraft::new(id, settings);
                        if let Some(previous) = &self.library.draft {
                            draft.profiles.clone_from(&previous.profiles);
                        }
                        self.library.replace_draft(Some(draft));
                        self.library.editing = true;
                        self.status_message = "Imported draft. Review and save to apply it.".into();
                    }
                    Err(error) => self.status_message = error,
                    _ => {}
                }
            }
            Message::LibraryExportLaunch => {
                let Some(draft) = &self.library.draft else {
                    return Task::none();
                };
                let settings = match draft.settings() {
                    Ok(settings) => settings,
                    Err(error) => {
                        self.status_message = error;
                        return Task::none();
                    }
                };
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .set_file_name("launch-settings.json")
                            .save_file()
                            .await
                        else {
                            return Ok("Export cancelled".into());
                        };
                        let path = file.path().to_owned();
                        tokio::task::spawn_blocking(move || {
                            modde_core::library::atomic_json(&path, &settings)
                                .map(|()| "Launch settings exported".into())
                                .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    |result| Message::LibraryPlayComplete {
                        id: String::new(),
                        hd: false,
                        result,
                    },
                );
            }
            Message::LibraryInstallHook => {
                if self.library.pending.is_some()
                    || !self.library.launching.is_empty()
                    || self.library.saving
                {
                    return Task::none();
                }
                let Some(id) = self.library.selected_id.clone() else {
                    return Task::none();
                };
                if self.library.launch_settings_dirty(&id) {
                    self.status_message =
                        "Save launch settings before setting up the store wrapper".into();
                    return Task::none();
                }
                self.library.saving = true;
                self.library.draft_revision = self.library.draft_revision.wrapping_add(1);
                let revision = self.library.draft_revision;
                let target = id.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || run_library_command(&["hook", &target]))
                            .await
                            .map_err(|error| error.to_string())?
                    },
                    move |result| Message::LibraryHookInstalled {
                        id: id.clone(),
                        revision,
                        result,
                    },
                );
            }
            Message::LibraryHookInstalled {
                id,
                revision,
                result,
            } => {
                self.library.saving = false;
                match result {
                    Ok(note) => {
                        if let Ok(preferences) = LibraryPreferences::load() {
                            self.library.preferences = preferences;
                        }
                        if self.library.draft_matches(&id, revision) {
                            let saved = self.library.preferences.launch_for(&id);
                            let mut draft = LaunchDraft::new(id, saved);
                            if let Some(previous) = &self.library.draft {
                                draft.profiles.clone_from(&previous.profiles);
                            }
                            self.library.replace_draft(Some(draft));
                        }
                        self.status_message = note.clone();
                        let value = note
                            .strip_prefix("Paste into this game's Steam Launch Options: ")
                            .unwrap_or(&note)
                            .to_owned();
                        // Supersede an in-flight pre-hook catalogue load before
                        // it can replace the settings we just read from disk.
                        let refresh = self.start_library_load();
                        self.status_message = note;
                        return Task::batch([iced::clipboard::write(value), refresh]);
                    }
                    Err(error) => self.status_message = error,
                }
            }
            Message::LibraryProfilesLoaded { id, result } => {
                if let Some(draft) = &mut self.library.draft
                    && draft.id == id
                {
                    match result {
                        Ok(names) => draft.profiles = names,
                        Err(error) => {
                            self.status_message = format!("Could not load profiles: {error}");
                        }
                    }
                }
            }
            Message::LibraryEditLaunch => {
                self.library.editing = !self.library.editing;
                if self.library.editing
                    && let Some(entry) = self.library.selected_entry()
                    && let Some(game_id) = entry.game_id()
                {
                    let game = GameId::from(game_id);
                    let id = entry.id.clone();
                    let pm = ProfileManager::with_db(self.db.clone());
                    return Task::perform(
                        async move {
                            pm.list_for_game(&game)
                                .await
                                .map(|profiles| {
                                    profiles.into_iter().map(|profile| profile.name).collect()
                                })
                                .map_err(|error| error.to_string())
                        },
                        move |result| Message::LibraryProfilesLoaded {
                            id: id.clone(),
                            result,
                        },
                    );
                }
            }
            Message::LibraryAdoptSaves => {
                if !self.library.launching.is_empty()
                    || self.library.pending.is_some()
                    || self.library.saving
                {
                    return Task::none();
                }
                let Some(draft) = &self.library.draft else {
                    return Task::none();
                };
                let settings = self.library.preferences.launch_for(&draft.id);
                if draft.settings().ok().as_ref() != Some(&settings) {
                    self.status_message = "Save launch settings before adopting saves".into();
                    return Task::none();
                }
                let Some(profile) = settings.profile else {
                    self.status_message =
                        "Choose and save a mod profile before adopting saves".into();
                    return Task::none();
                };
                let id = draft.id.clone();
                let task_id = id.clone();
                self.library.launching.insert(id.clone());
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            run_library_command(&["adopt", &task_id, "--profile", &profile])
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    move |result| Message::LibraryPlayComplete {
                        id: id.clone(),
                        hd: false,
                        result,
                    },
                );
            }
            Message::LibraryCategoryChanged(category) => self.library.category = category,
            Message::LibrarySteamIdChanged(value) => self.library.steam.account = value,
            Message::LibrarySyncSteam => {
                if self.library.steam.running {
                    return Task::none();
                }
                self.library.steam.running = true;
                let id = self.library.steam.account.trim().to_string();
                return Task::perform(
                    async move {
                        let key = std::env::var("MODDE_STEAM_API_KEY").map_err(|_| {
                            "Set MODDE_STEAM_API_KEY before syncing Steam".to_string()
                        })?;
                        modde_games::library::sync_steam(&id, &key)
                            .await
                            .map_err(|error| error.to_string())
                    },
                    Message::LibrarySteamSynced,
                );
            }
            Message::LibrarySteamSynced(result) => {
                self.library.steam.running = false;
                match result {
                    Ok(_) => return self.start_library_load(),
                    Err(error) => self.status_message = format!("Steam sync failed: {error}"),
                }
            }
            Message::LibraryLaunchFieldChanged(field, value) => {
                self.library.edit_draft(|draft| draft.set(field, value));
            }
            Message::LibrarySandboxChanged(enabled) => {
                self.library
                    .edit_draft(|draft| draft.sandbox.enabled = enabled);
            }
            Message::LibraryNetworkChanged(enabled) => {
                self.library
                    .edit_draft(|draft| draft.sandbox.network = enabled);
            }
            Message::LibraryActiveProfileChanged(enabled) => {
                self.library
                    .edit_draft(|draft| draft.use_active_profile = enabled);
            }
            Message::LibrarySaveLaunch => {
                if self.library.saving
                    || !self.library.launching.is_empty()
                    || self.library.pending.is_some()
                {
                    return Task::none();
                }
                let Some(draft) = self.library.draft.clone() else {
                    return Task::none();
                };
                let settings = match draft.settings() {
                    Ok(settings) => settings,
                    Err(error) => {
                        self.status_message = error;
                        return Task::none();
                    }
                };
                let app_settings = self.settings.clone();
                let manager = self
                    .library
                    .selected_entry()
                    .filter(|entry| entry.id == draft.id && entry.manager_instance().is_some())
                    .cloned();
                self.library.saving = true;
                self.library.draft_revision = self.library.draft_revision.wrapping_add(1);
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || -> Result<(), String> {
                        let _guard = modde_core::library::mutation_lock().map_err(|error| error.to_string())?;
                        if PendingSession::load_blocking().map_err(|error| error.to_string())?.is_some() { return Err("End the game session before changing launch settings".into()); }
                        let catalogue = modde_games::library::catalogue(&app_settings).map_err(|error| error.to_string())?;
                        if let Some(manager) = manager {
                            if settings.executable.is_some() || settings.runner.is_some() || settings.prefix.is_some() || settings.profile.is_some() || settings.store_hook {
                                return Err("Manager owns the executable, runner, prefix and profile. Configure wrappers, environment and sandbox grants here.".into());
                            }
                            if !manager.install_path.as_ref().is_some_and(|path| path.is_dir()) { return Err("Manager installation is missing".into()); }
                        } else {
                            let game = catalogue.games.iter().find(|game| game.id == draft.id).ok_or("Installation is no longer available")?;
                            modde_games::library::launch::validate(game, &settings, &catalogue.games).map_err(|error| error.to_string())?;
                            let mut preferences = LibraryPreferences::load().map_err(|error| error.to_string())?;
                            preferences.launches.insert(draft.id.clone(), settings.clone());
                            modde_games::library::context::effective_settings(game, &catalogue.games, &preferences).map_err(|error| error.to_string())?;
                        }
                        LibraryPreferences::update(|preferences| { preferences.launches.insert(draft.id, settings); }).map_err(|error| error.to_string())
                    }).await.map_err(|error| error.to_string())?
                    },
                    Message::LibraryPreferenceSaved,
                );
            }
            Message::LibraryFavorite(id) => {
                if self.library.saving {
                    return Task::none();
                }
                let Some(entry) = self.library.entries.iter().find(|entry| entry.id == id) else {
                    return Task::none();
                };
                let key = entry.entitlement.clone();
                let favorite = !entry.favorite;
                self.library.saving = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            LibraryPreferences::update(|preferences| {
                                if favorite {
                                    preferences.favorites.insert(key);
                                } else {
                                    preferences.favorites.remove(&key);
                                }
                            })
                            .map_err(|error| error.to_string())
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    Message::LibraryPreferenceSaved,
                );
            }
            Message::LibraryPreferenceSaved(result) => {
                self.library.saving = false;
                match result {
                    Ok(()) => return self.start_library_load(),
                    Err(error) => self.status_message = format!("Settings not saved: {error}"),
                }
            }
            Message::LibraryInstall(id) => {
                if !self.library.launching.is_empty() || self.library.pending.is_some() {
                    return Task::none();
                }
                let Some(entry) = self.library.entries.iter().find(|entry| entry.id == id) else {
                    return Task::none();
                };
                let crate::views::library::LibraryEntryKind::Game { game } = &entry.kind else {
                    return Task::none();
                };
                let game = game.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || modde_games::library::launch::install(&game)
                        .map(|()| "Installation requested in the store. Refresh when it completes.".to_string())
                        .map_err(|error| error.to_string())).await.map_err(|error| error.to_string())?
                    },
                    move |result| Message::LibraryPlayComplete {
                        id: id.clone(),
                        hd: false,
                        result,
                    },
                );
            }
            Message::LibraryFinishSession | Message::LibrarySkipAnalysis => {
                if !self.library.launching.is_empty() {
                    return Task::none();
                }
                let skip = matches!(message, Message::LibrarySkipAnalysis);
                if skip
                    && self.library.pending.as_ref().is_none_or(|session| {
                        session.phase != modde_core::library::SessionPhase::Captured
                    })
                {
                    return Task::none();
                }
                let command = if self
                    .library
                    .pending
                    .as_ref()
                    .is_some_and(|session| session.phase.is_preparation())
                {
                    "recover"
                } else {
                    "finish"
                };
                let id = self
                    .library
                    .pending
                    .as_ref()
                    .map(|session| session.installation.clone())
                    .unwrap_or_default();
                self.library.launching.insert(id.clone());
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            run_library_command(if skip {
                                &["finish", "--skip-analysis", "--confirm-exited"]
                            } else if command == "finish" {
                                &["finish", "--confirm-exited"]
                            } else {
                                &["recover"]
                            })
                        })
                        .await
                        .map_err(|error| error.to_string())?
                    },
                    move |result| Message::LibraryPlayComplete {
                        id: id.clone(),
                        hd: false,
                        result,
                    },
                );
            }
            Message::LibraryFilterChanged(filter) => {
                self.library.filter = filter;
            }
            Message::LibrarySelectEntry(id) => {
                self.library.editing = false;
                self.library.replace_draft(Some(LaunchDraft::new(
                    id.clone(),
                    self.library.preferences.launch_for(&id),
                )));
                self.library.selected_id = Some(id);
            }
            Message::LibraryRefresh => {
                return self.start_library_load();
            }
            Message::LibraryLoaded {
                generation,
                session_revision,
                result,
            } => {
                if generation != self.library.generation {
                    return Task::none();
                }
                self.library.loading = false;
                match result {
                    Ok(loaded) => {
                        let keep_draft = self
                            .library
                            .selected_id
                            .as_ref()
                            .is_some_and(|id| self.library.launch_settings_dirty(id));
                        self.library.load_error = None;
                        self.library.manager_config = loaded.manager_config;
                        self.library.manager_error = loaded.manager_error;
                        self.library.notices = loaded.notices;
                        if session_revision == self.library.coordination.revision {
                            self.library.coordination.revision =
                                self.library.coordination.revision.wrapping_add(1);
                            self.library.pending = loaded.pending;
                        }
                        self.library.preferences = loaded.preferences;
                        if !self.library.steam.running {
                            self.library.steam.account = self
                                .library
                                .preferences
                                .steam_id
                                .clone()
                                .unwrap_or_default();
                        }
                        let selected_still_visible =
                            self.library.selected_id.as_ref().is_some_and(|id| {
                                loaded.entries.iter().any(|entry| &entry.id == id)
                            });
                        if !selected_still_visible {
                            self.library.selected_id =
                                loaded.entries.first().map(|entry| entry.id.clone());
                        }
                        if !keep_draft || !selected_still_visible {
                            self.library
                                .replace_draft(self.library.selected_id.as_ref().map(|id| {
                                    LaunchDraft::new(
                                        id.clone(),
                                        self.library.preferences.launch_for(id),
                                    )
                                }));
                        }
                        self.library.entries = loaded.entries;
                        if self.status_message == "Loading library..." {
                            self.status_message = "Library refreshed".to_string();
                        }
                    }
                    Err(err) => {
                        self.library.load_error = Some(err.clone());
                        self.status_message = format!("Library refresh failed: {err}");
                    }
                }
            }
            Message::LibraryPlay { id, hd } => {
                return self.start_library_launch(id, hd);
            }
            Message::LibraryPlayComplete { id, hd: _, result } => {
                self.library.launching.remove(&id);
                self.library.coordination.revision =
                    self.library.coordination.revision.wrapping_add(1);
                match PendingSession::load_blocking() {
                    Ok(pending) => self.library.pending = pending,
                    Err(error) => self.library.load_error = Some(error.to_string()),
                }
                match result {
                    Ok(note) => self.status_message = note,
                    Err(err) => self.status_message = format!("Launch failed: {err}"),
                }
            }
            Message::LibraryManageGame(game_id) => {
                if !self.library.launching.is_empty() || self.library.pending.is_some() {
                    self.status_message = "Finish the game session before changing mods".into();
                    return Task::none();
                }
                let previous_game = self.selected_game.clone();
                if let Some(path) = self
                    .library
                    .selected_entry()
                    .filter(|entry| entry.game_id() == Some(game_id.as_str()))
                    .and_then(|entry| entry.install_path.clone())
                {
                    self.settings
                        .set_game_path(&GameId::from(game_id.as_str()), path);
                    self.settings.save();
                }
                let task = self.accept_game_selection(game_id, previous_game);
                self.active_view = View::ModList;
                return task;
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }

    pub(super) fn start_library_load(&mut self) -> Task<Message> {
        self.library.loading = true;
        self.library.load_error = None;
        self.library.generation = self.library.generation.wrapping_add(1);
        let generation = self.library.generation;
        let session_revision = self.library.coordination.revision;
        self.status_message = "Loading library...".to_string();
        let settings = self.settings.clone();
        let manager_bin = manager_binary();
        let manager_config = manager_config_path();
        let db = self.db.clone();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || {
                    load_library_blocking(&settings, &manager_bin, manager_config.as_deref(), db)
                })
                .await
                .map_err(|err| err.to_string())?
            },
            move |result| Message::LibraryLoaded {
                generation,
                session_revision,
                result,
            },
        )
    }

    fn start_library_launch(&mut self, id: String, hd: bool) -> Task<Message> {
        if self.library.launch_settings_dirty(&id) {
            self.status_message = "Save the launch settings before Play".into();
            return Task::none();
        }
        if !self.library.launching.is_empty()
            || self.library.pending.is_some()
            || self.library.saving
        {
            self.status_message = "A session or settings save is already in progress".to_string();
            return Task::none();
        }
        let Some(entry) = self
            .library
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .cloned()
        else {
            self.status_message = "Selected library entry is no longer available".to_string();
            return Task::none();
        };
        if !entry.launchable {
            self.status_message = entry
                .unavailability_reason
                .clone()
                .unwrap_or_else(|| "Launch not configured".to_string());
            return Task::none();
        }
        self.library.launching.insert(id.clone());
        self.library.draft_revision = self.library.draft_revision.wrapping_add(1);
        self.library.selected_id = Some(id.clone());
        let mode_note = if hd { " (HD)" } else { "" };
        self.status_message = format!("Launching {}{}...", entry.display_name, mode_note);
        match entry.kind {
            crate::views::library::LibraryEntryKind::Game { game } => Task::perform(
                async move {
                    tokio::task::spawn_blocking(move || run_library_command(&["play", &game.id]))
                        .await
                        .map_err(|err| err.to_string())?
                },
                move |result| Message::LibraryPlayComplete {
                    id: id.clone(),
                    hd,
                    result,
                },
            ),
            crate::views::library::LibraryEntryKind::Manager { instance } => {
                let manager_bin = manager_binary();
                let Some(manager_config) = self
                    .library
                    .manager_config
                    .clone()
                    .or_else(manager_config_path)
                else {
                    self.library.launching.remove(&id);
                    self.status_message =
                        "Manager config is not set (MODDE_MANAGER_CONFIG)".to_string();
                    return Task::none();
                };
                Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            launch_manager_blocking(&manager_bin, &manager_config, &instance, hd)
                        })
                        .await
                        .map_err(|err| err.to_string())?
                    },
                    move |result| Message::LibraryPlayComplete {
                        id: id.clone(),
                        hd,
                        result,
                    },
                )
            }
        }
    }
}

/// Manager binary override for tests and custom installs; defaults to PATH.
fn manager_binary() -> PathBuf {
    std::env::var_os("MODDE_MANAGER_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("modde-manager"))
}

/// Manager config for `list` and `onboard launch`. `None` means the manager
/// section degrades to a hint while detected games still load.
fn manager_config_path() -> Option<PathBuf> {
    std::env::var_os("MODDE_MANAGER_CONFIG").map(PathBuf::from)
}

/// Blocking library scan: detected Steam/Heroic installs plus configured
/// game paths, then best-effort manager instances. Manager failures never
/// fail the game list — they surface as `manager_error`.
fn load_library_blocking(
    settings: &AppSettings,
    manager_bin: &Path,
    manager_config: Option<&Path>,
    db: modde_core::db::ModdeDb,
) -> Result<LibraryLoadResult, String> {
    let catalogue = modde_games::library::catalogue(settings).map_err(|error| error.to_string())?;
    let preferences = LibraryPreferences::load().map_err(|error| error.to_string())?;
    let pm = ProfileManager::with_db(db);
    let profiles = crate::app::block_on(pm.list()).map_err(|error| error.to_string())?;
    let managed = profiles
        .into_iter()
        .map(|profile| profile.game_id.to_string())
        .collect();
    let games = build_catalogue_entries(&catalogue.games, &preferences, &managed);

    let (managers, manager_config_out, manager_error) = match manager_config {
        Some(config) => match query_manager_instances(manager_bin, config) {
            Ok(instances) => (
                build_manager_entries(instances),
                Some(config.to_path_buf()),
                None,
            ),
            Err(err) => (Vec::new(), Some(config.to_path_buf()), Some(err)),
        },
        None => (Vec::new(), None, None),
    };
    let mut managers = managers;
    for entry in &mut managers {
        entry.favorite = preferences.favorites.contains(&entry.entitlement);
    }
    Ok(LibraryLoadResult {
        entries: merge_library_entries(games, managers),
        manager_config: manager_config_out,
        manager_error,
        preferences,
        notices: catalogue.notices,
        pending: PendingSession::load_blocking().map_err(|error| error.to_string())?,
    })
}

fn query_manager_instances(
    manager_bin: &Path,
    manager_config: &Path,
) -> Result<Vec<crate::views::library::ManagerListInstance>, String> {
    let output = std::process::Command::new(manager_bin)
        .arg("--config")
        .arg(manager_config)
        .arg("list")
        .arg("--json")
        .output()
        .map_err(|err| format!("could not run {}: {err}", manager_bin.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "manager list failed ({}): {}",
            output.status,
            stderr.trim()
        ));
    }
    let stdout =
        String::from_utf8(output.stdout).map_err(|err| format!("invalid manager output: {err}"))?;
    parse_manager_list_json(&stdout)
}

/// Keep potentially unbounded game output on disk. Read only the final 8 KiB
/// for an error/status message, never buffer a whole gaming session in the GUI.
fn run_library_command(args: &[&str]) -> Result<String, String> {
    use std::io::{Read, Seek, SeekFrom};
    let binary = library_cli_binary()?;
    let logs = modde_core::paths::modde_data_dir().join("logs");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = logs.join(format!("library-{}-{stamp}.log", std::process::id()));
    let log = std::fs::File::create(&path).map_err(|error| error.to_string())?;
    let status = std::process::Command::new(binary)
        .arg("--config-dir")
        .arg(modde_core::paths::config_dir())
        .arg("--data-dir")
        .arg(modde_core::paths::modde_data_dir())
        .arg("library")
        .args(args)
        .stdout(log.try_clone().map_err(|error| error.to_string())?)
        .stderr(log)
        .status()
        .map_err(|error| error.to_string())?;
    let mut file = std::fs::File::open(&path).map_err(|error| error.to_string())?;
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(8192)))
        .map_err(|error| error.to_string())?;
    let mut tail = Vec::new();
    file.take(8192)
        .read_to_end(&mut tail)
        .map_err(|error| error.to_string())?;
    let note = String::from_utf8_lossy(&tail).trim().to_string();
    if status.success() {
        Ok(note
            .lines()
            .last()
            .unwrap_or("Session complete")
            .to_string())
    } else {
        Err(format!("{status}: {note} (log: {})", path.display()))
    }
}

fn library_cli_binary() -> Result<PathBuf, String> {
    if let Some(binary) = std::env::var_os("MODDE_BIN") {
        let binary = PathBuf::from(binary);
        return if binary.is_relative() && binary.components().count() > 1 {
            std::path::absolute(binary).map_err(|error| error.to_string())
        } else {
            Ok(binary)
        };
    }
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    // The standalone GUI is modde-ui; re-executing it would open another GUI
    // instead of invoking the lifecycle command. Nix wrappers are siblings too.
    let name = if cfg!(windows) { "modde.exe" } else { "modde" };
    if let Some(parent) = current.parent() {
        let sibling = parent.join(name);
        if sibling.is_file() {
            return Ok(sibling);
        }
    }
    Ok(PathBuf::from(name))
}

/// Launch-only manager start through the existing native launch path. No
/// prepare/install/reconcile here — failures (including HD-not-ready) surface
/// as errors.
fn launch_manager_blocking(
    manager_bin: &Path,
    manager_config: &Path,
    instance: &str,
    hd: bool,
) -> Result<String, String> {
    if PendingSession::load_blocking()
        .map_err(|error| error.to_string())?
        .is_some()
    {
        return Err("Finish the current game session first".into());
    }
    let mode = if hd { "hd" } else { "vanilla" };
    let logs = modde_core::paths::modde_data_dir().join("logs");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let path = logs.join(format!("manager-{}-{stamp}.log", std::process::id()));
    let log = std::fs::File::create(&path).map_err(|error| error.to_string())?;
    let status = std::process::Command::new(manager_bin)
        .env(
            "MODDE_LIBRARY_LAUNCH_ID",
            crate::views::library::manager_entry_id(instance),
        )
        .env(
            "MODDE_LIBRARY_DATA_DIR",
            std::path::absolute(modde_core::paths::modde_data_dir())
                .map_err(|error| error.to_string())?,
        )
        .env(
            "MODDE_LIBRARY_CONFIG_DIR",
            std::path::absolute(modde_core::paths::config_dir())
                .map_err(|error| error.to_string())?,
        )
        .env("MODDE_BIN", library_cli_binary()?)
        .arg("--config")
        .arg(manager_config)
        .arg("onboard")
        .arg("launch")
        .arg("--instance")
        .arg(instance)
        .arg("--target")
        .arg("game")
        .arg("--mode")
        .arg(mode)
        .stdout(log.try_clone().map_err(|error| error.to_string())?)
        .stderr(log)
        .status()
        .map_err(|err| format!("could not run {}: {err}", manager_bin.display()))?;
    if status.success() {
        Ok(format!(
            "{instance}: session complete ({mode} mode). Log: {}",
            path.display()
        ))
    } else {
        Err(format!(
            "{instance}: launch exited with {status}; log: {}",
            path.display()
        ))
    }
}
