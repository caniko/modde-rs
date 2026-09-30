//! Loads user-defined games from TOML specs in modde's data directory,
//! validating and deduplicating each [`GameSpec`] before turning it into a
//! [`GenericGame`]-backed [`GameRegistration`] for the game registry.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use tracing::warn;

use crate::generic::GenericGame;
use crate::registry::{EngineFamily, GameRegistration, LauncherIds};
use crate::traits::{GamePlugin, HotDeploySupport};

use super::leak::str as leak_str;
use super::spec::GameSpec;

#[must_use]
pub fn load_user_games() -> Vec<GameRegistration> {
    let games_dir = modde_core::paths::modde_data_dir().join("games");
    let Ok(entries) = fs::read_dir(&games_dir) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
    paths.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut registrations = Vec::new();

    for path in paths {
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }

        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.ends_with(".optiscaler.toml"))
        {
            continue;
        }

        let content = match fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                warn!(path = %path.display(), error = %error, "skipping user game spec");
                continue;
            }
        };

        let spec = match toml::from_str::<GameSpec>(&content) {
            Ok(spec) => spec,
            Err(error) => {
                warn!(path = %path.display(), error = %error, "skipping user game spec");
                continue;
            }
        };

        if let Err(error) = spec.validate() {
            warn!(path = %path.display(), error = %error, "skipping user game spec");
            continue;
        }

        if !seen_ids.insert(spec.id.clone()) {
            warn!(
                path = %path.display(),
                game_id = %spec.id,
                "skipping user game spec with duplicate id"
            );
            continue;
        }

        let game_id = leak_str(spec.id.clone());
        let display_name = leak_str(spec.display_name.clone());
        let nexus_domain = spec.nexus_domain.clone().map(leak_str);
        let steam_app_id = spec.steam_app_id.clone().map(leak_str);
        let steam_dir = spec.install_dir_name.clone().map(leak_str);

        let plugin: &'static dyn GamePlugin = Box::leak(Box::new(GenericGame::from_spec(spec)));

        registrations.push(GameRegistration {
            game_id,
            display_name,
            engine: EngineFamily::Generic,
            launcher: LauncherIds {
                steam_app_id,
                steam_dir,
                heroic_gog_app_id: None,
                heroic_epic_app_id: None,
            },
            wabbajack_names: &[],
            nexus_domain,
            nexus_game_id: None,
            supports_save_profiles: false,
            plugin,
            scanner: None,
            save_tracker: None,
            save_dependency_analyzer: None,
            collision_classifier: Some(crate::registry::generic_collision_classifier),
            hot_deploy: HotDeploySupport::Unsupported,
            optiscaler_profiles: &[],
        });
    }

    registrations
}

pub fn reload_user_games() {
    crate::registry::reload_registry();
}
