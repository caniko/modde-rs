//! Account catalogue and exact-install launch support, independent of mod plugins.
//! Store libraries include uninstalled and unregistered games. A mod plugin is
//! an optional capability, not a requirement for being in the Library.

pub mod context;
pub mod gpu;
pub mod launch;
pub mod observer;
mod operations;
mod providers;
mod runtime;
mod sandbox;

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Result;
use modde_core::library::installation_id;
use modde_core::settings::AppSettings;
use serde::{Deserialize, Serialize};

pub use providers::{SteamSnapshot, heroic_config, sync_steam};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Store {
    Steam,
    Gog,
    Epic,
    Sideload,
    Local,
}

impl Store {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::Gog => "GOG",
            Self::Epic => "Epic",
            Self::Sideload => "Heroic/Sideload",
            Self::Local => "Local",
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Gog => "gog",
            Self::Epic => "epic",
            Self::Sideload => "sideload",
            Self::Local => "local",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryGame {
    pub id: String,
    pub entitlement: String,
    pub name: String,
    pub game_id: Option<String>,
    pub store: Store,
    pub app_id: String,
    pub install_path: Option<PathBuf>,
}

impl LibraryGame {
    pub fn new(store: Store, app_id: String, name: String, install_path: Option<PathBuf>) -> Self {
        let entitlement = format!("{}:{app_id}", store.key());
        let id = install_path.as_ref().map_or_else(
            || entitlement.clone(),
            |path| installation_id(&entitlement, path),
        );
        let game_id = crate::registry::all_games()
            .iter()
            .find(|game| match store {
                Store::Steam => game.launcher.steam_app_id == Some(app_id.as_str()),
                Store::Gog => game.launcher.heroic_gog_app_id == Some(app_id.as_str()),
                Store::Epic => game.launcher.heroic_epic_app_id == Some(app_id.as_str()),
                Store::Local => game.game_id == app_id,
                Store::Sideload => false,
            })
            .map(|game| game.game_id.to_string());
        Self {
            id,
            entitlement,
            name,
            game_id,
            store,
            app_id,
            install_path,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    pub games: Vec<LibraryGame>,
    /// Provider coverage and stale/offline state; never infer ownership from disk.
    pub notices: Vec<String>,
}

pub fn catalogue(settings: &AppSettings) -> Result<Catalogue> {
    let preferences = modde_core::library::LibraryPreferences::load()?;
    let mut result = Catalogue::default();
    providers::steam(&mut result, preferences.steam_id.as_deref());
    providers::heroic(&mut result);
    // Preserve registered-game fallbacks (for example a known Steam common
    // directory with no manifest), without restricting the owned catalogue.
    for detected in crate::scan_installed_games() {
        let (store, app_id) = match detected.source {
            crate::detection::LauncherSource::Steam { app_id, .. } => (Store::Steam, app_id),
            crate::detection::LauncherSource::HeroicGog { app_id } => (Store::Gog, app_id),
            crate::detection::LauncherSource::HeroicEpic { app_id } => (Store::Epic, app_id),
            crate::detection::LauncherSource::HeroicSideload { app_id } => {
                (Store::Sideload, app_id)
            }
        };
        let mut entry = LibraryGame::new(
            store,
            app_id,
            detected.display_name.into(),
            Some(detected.install_path),
        );
        entry.game_id = Some(detected.game_id.into());
        if let Some(existing) = result.games.iter_mut().find(|game| game.id == entry.id) {
            existing.game_id = entry.game_id;
        } else {
            result.games.push(entry);
        }
    }
    for configured in &settings.game_paths {
        let game_id = configured.game_id.to_string();
        let name = crate::resolve_game_plugin(&game_id).map_or_else(
            || game_id.clone(),
            |plugin| plugin.display_name().to_string(),
        );
        result.games.push(LibraryGame::new(
            Store::Local,
            game_id,
            name,
            Some(configured.path.clone()),
        ));
    }
    modde_core::library::LibraryPreferences::try_update_at(
        &modde_core::library::LibraryPreferences::path(),
        |prefs| reconcile_installations(&mut result.games, prefs),
    )?;
    result.games = merge_games(result.games);
    Ok(result)
}

fn reconcile_installations(
    games: &mut [LibraryGame],
    preferences: &mut modde_core::library::LibraryPreferences,
) -> Result<()> {
    let mut groups: BTreeMap<PathBuf, Vec<usize>> = BTreeMap::new();
    for (index, game) in games.iter().enumerate() {
        if let Some(path) = &game.install_path {
            groups
                .entry(modde_core::library::normalized_path(path))
                .or_default()
                .push(index);
        }
    }
    for (path, indices) in groups {
        let paths: Vec<_> = indices
            .iter()
            .filter_map(|&i| games[i].install_path.clone())
            .collect();
        let mut entitlements: Vec<_> = indices
            .iter()
            .map(|&i| games[i].entitlement.clone())
            .collect();
        // A configured local row may already have disappeared from discovery.
        for &index in &indices {
            if let Some(game_id) = &games[index].game_id {
                entitlements.push(format!("local:{game_id}"));
            }
        }
        let old_ids: Vec<_> = entitlements
            .iter()
            .flat_map(|entitlement| paths.iter().map(move |p| installation_id(entitlement, p)))
            .collect();
        let id = preferences.bind_installation(&paths, &old_ids, &entitlements)?;
        let plugin = indices
            .iter()
            .find_map(|&index| games[index].game_id.clone());
        for index in indices {
            games[index].id.clone_from(&id);
            games[index].install_path = Some(path.clone());
            if games[index].game_id.is_none() {
                games[index].game_id.clone_from(&plugin);
            }
        }
    }
    Ok(())
}

fn merge_games(games: Vec<LibraryGame>) -> Vec<LibraryGame> {
    let installed: std::collections::BTreeSet<_> = games
        .iter()
        .filter(|game| game.install_path.is_some())
        .map(|game| game.entitlement.clone())
        .collect();
    let mut entries = BTreeMap::new();
    for game in games {
        if game.install_path.is_none() && installed.contains(&game.entitlement) {
            continue;
        }
        entries.entry(game.id.clone()).or_insert(game);
    }
    entries.into_values().collect()
}

#[cfg(test)]
mod tests;
