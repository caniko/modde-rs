//! One installation selection and save-vault resolution for every consumer.

use anyhow::{Context, Result, bail};
use modde_core::GameId;
use modde_core::db::ModdeDb;
use modde_core::library::{
    LaunchSettings, LegacySaveBinding, LibraryPreferences, SaveContext, normalized_path,
};
use modde_core::settings::AppSettings;
use std::path::{Path, PathBuf};

use super::{LibraryGame, Store, catalogue};

pub struct InstallationContext {
    pub game: LibraryGame,
    pub launch: LaunchSettings,
    pub saves: SaveContext,
    pub prefix: Option<PathBuf>,
}

impl InstallationContext {
    pub fn require_save_management(&self) -> Result<()> {
        if crate::resolve_game_plugin(self.saves.game_id.as_str())
            .is_some_and(|plugin| plugin.supports_save_profiles())
            && self.saves.directory.is_none()
        {
            bail!("configure this installation's save_directory before switching profiles");
        }
        Ok(())
    }
}

/// Game-oriented commands use the configured installation, or the sole copy.
/// Ambiguity is an error; callers must not fall back to a game-wide vault.
pub async fn for_game(
    settings: &AppSettings,
    game_id: &str,
    db: &ModdeDb,
) -> Result<InstallationContext> {
    let games = catalogue(settings)?.games;
    let configured = settings
        .game_paths
        .iter()
        .find(|entry| entry.game_id.as_str() == game_id);
    let candidates: Vec<_> = games
        .iter()
        .filter(|game| game.game_id.as_deref() == Some(game_id) && game.install_path.is_some())
        .collect();
    let game = if let Some(configured) = configured {
        candidates.iter().copied().find(|game| {
            game.install_path
                .as_deref()
                .is_some_and(|path| normalized_path(path) == normalized_path(&configured.path))
        })
    } else if candidates.len() == 1 {
        Some(candidates[0])
    } else {
        None
    };
    let game = game.context("select an installation in Library → Manage Mods (or configure its game path) before accessing installation state")?;
    for_installation(game, &games, db).await
}

pub fn effective_settings(
    game: &LibraryGame,
    games: &[LibraryGame],
    preferences: &LibraryPreferences,
) -> Result<LaunchSettings> {
    effective_settings_with_prefix(game, games, preferences, None)
}

fn effective_settings_with_prefix(
    game: &LibraryGame,
    games: &[LibraryGame],
    preferences: &LibraryPreferences,
    runtime_prefix: Option<&Option<PathBuf>>,
) -> Result<LaunchSettings> {
    let settings = settings_with_paths(game, games, preferences, runtime_prefix)?;
    if let Some(save) = &settings.save_directory {
        if !save.is_absolute() {
            bail!("save_directory must be absolute");
        }
        for other in games
            .iter()
            .filter(|other| other.id != game.id && other.install_path.is_some())
        {
            let other_settings = settings_with_paths(other, games, preferences, None)?;
            if overlapping_paths(save, other_settings.save_directory.as_deref()) {
                bail!(
                    "another installation resolves to an overlapping save directory; configure separate save directories"
                );
            }
        }
        for (id, other) in &preferences.launches {
            if id != &game.id && overlapping_paths(save, other.save_directory.as_deref()) {
                bail!(
                    "another installation uses an overlapping save directory; configure a separate save directory"
                );
            }
        }
        for binding in preferences.legacy_save_bindings.values() {
            if binding.installation != game.id
                && overlapping_paths(save, binding.save_directory.as_deref())
            {
                bail!("this save directory overlaps another installation's legacy vault");
            }
        }
    }
    Ok(settings)
}

fn settings_with_paths(
    game: &LibraryGame,
    games: &[LibraryGame],
    preferences: &LibraryPreferences,
    runtime_prefix: Option<&Option<PathBuf>>,
) -> Result<LaunchSettings> {
    let mut settings = preferences.launch_for(&game.id);
    if let Some(prefix) = runtime_prefix {
        settings.prefix.clone_from(prefix);
    } else if settings.prefix.is_none()
        && (settings.runner.is_some() || (settings.executable.is_none() && settings.store_hook))
    {
        settings.prefix = super::providers::heroic_prefix(game)?;
    }
    let native_boundary = runtime_prefix.is_some_and(Option::is_none)
        || (settings.executable.is_some()
            && settings.runner.is_none()
            && settings.prefix.is_none());
    let Some(game_id) = &game.game_id else {
        return Ok(settings);
    };
    if settings.save_directory.is_none() {
        let plugin = crate::resolve_game_plugin(game_id).context("game plugin unavailable")?;
        if plugin.supports_save_profiles() {
            let prefix = match runtime_prefix {
                Some(prefix) => prefix.clone(),
                None => installation_prefix(game, &settings)?,
            };
            settings.save_directory = game
                .install_path
                .as_deref()
                .and_then(|install| plugin.save_directory_at(install, prefix.as_deref()));
            // Old plugin discovery is allowed only for a single default install.
            if settings.save_directory.is_none()
                && settings.prefix.is_none()
                && runtime_prefix.is_none()
                && !native_boundary
                && games
                    .iter()
                    .filter(|other| other.game_id == game.game_id && other.install_path.is_some())
                    .count()
                    == 1
                && game.install_path.as_deref().is_some_and(|install| {
                    install.parent() == Some(modde_core::paths::steam_common().as_path())
                })
                && plugin.detect_install().as_deref().map(normalized_path)
                    == game.install_path.as_deref().map(normalized_path)
            {
                settings.save_directory = plugin.save_directory();
            }
            if prefix.is_none() && settings.runner.is_none() {
                let host = super::runtime::RuntimePaths::host();
                let runtime = super::runtime::RuntimePaths::resolve(&settings)?;
                settings.save_directory = settings
                    .save_directory
                    .map(|path| runtime.relocate(&host, &path));
            }
        }
    }
    if settings.save_directory.is_none()
        && settings.prefix.is_none()
        && !native_boundary
        && !settings.environment.keys().any(|key| {
            matches!(
                key.as_str(),
                "HOME" | "XDG_CONFIG_HOME" | "XDG_DATA_HOME" | "XDG_CACHE_HOME"
            )
        })
        && let Some(binding) = preferences.legacy_save_bindings.get(game_id)
        && binding.installation == game.id
    {
        settings.save_directory.clone_from(&binding.save_directory);
    }
    Ok(settings)
}

fn same_path(a: Option<&Path>, b: Option<&Path>) -> bool {
    a.map(normalized_path) == b.map(normalized_path)
}

fn overlapping_paths(a: &Path, b: Option<&Path>) -> bool {
    let Some(b) = b else {
        return false;
    };
    let a = normalized_path(a);
    let b = normalized_path(b);
    a.starts_with(&b) || b.starts_with(&a)
}

pub fn save_scope(
    game: &LibraryGame,
    settings: &LaunchSettings,
    preferences: &LibraryPreferences,
) -> GameId {
    if let Some(game_id) = &game.game_id
        && let Some(binding) = preferences.legacy_save_bindings.get(game_id)
        && binding.installation == game.id
        && same_path(
            binding.save_directory.as_deref(),
            settings.save_directory.as_deref(),
        )
    {
        return GameId::from(game_id.as_str());
    }
    GameId::from(settings.save_directory.as_deref().map_or_else(
        || game.id.clone(),
        |path| modde_core::library::installation_id(&game.id, &normalized_path(path)),
    ))
}

pub async fn for_installation(
    game: &LibraryGame,
    games: &[LibraryGame],
    db: &ModdeDb,
) -> Result<InstallationContext> {
    resolve_context(game, games, db, LibraryPreferences::load()?, None).await
}

/// Resolve transient provider/benchmark settings before deriving the save scope.
/// The saved preferences remain unchanged; the journal pins the effective paths.
pub async fn with_launch_settings(
    game: &LibraryGame,
    games: &[LibraryGame],
    db: &ModdeDb,
    settings: LaunchSettings,
) -> Result<InstallationContext> {
    let mut preferences = LibraryPreferences::load()?;
    preferences.launches.insert(game.id.clone(), settings);
    resolve_context(game, games, db, preferences, None).await
}

/// The actual store command is authoritative about native versus Wine execution.
/// A native boundary must not rediscover an old compatdata directory from disk.
pub async fn with_store_settings(
    game: &LibraryGame,
    games: &[LibraryGame],
    db: &ModdeDb,
    settings: LaunchSettings,
    prefix: Option<PathBuf>,
) -> Result<InstallationContext> {
    let mut preferences = LibraryPreferences::load()?;
    preferences.launches.insert(game.id.clone(), settings);
    resolve_context(game, games, db, preferences, Some(prefix)).await
}

async fn resolve_context(
    game: &LibraryGame,
    games: &[LibraryGame],
    db: &ModdeDb,
    mut preferences: LibraryPreferences,
    runtime_prefix: Option<Option<PathBuf>>,
) -> Result<InstallationContext> {
    let settings =
        effective_settings_with_prefix(game, games, &preferences, runtime_prefix.as_ref())?;
    if let Some(game_id) = &game.game_id {
        let legacy_key = GameId::from(game_id.as_str());
        let legacy_exists = db.get_active_profile(&legacy_key).await?.is_some()
            || modde_core::paths::save_vault_dir(&legacy_key).exists();
        let legacy_save =
            crate::resolve_game_plugin(game_id).and_then(|plugin| plugin.save_directory());
        let unique = games
            .iter()
            .filter(|other| other.game_id == game.game_id && other.install_path.is_some())
            .count()
            == 1;
        let scoped = save_scope(game, &settings, &preferences);
        // Adoption may already have created independent state. Never later bind
        // that installation to a different vault because a legacy slot appeared.
        let scoped_exists = db.get_active_profile(&scoped).await?.is_some()
            || modde_core::paths::save_vault_dir(&scoped).exists();
        if unique
            && legacy_exists
            && !scoped_exists
            && settings.prefix.is_none()
            && same_path(settings.save_directory.as_deref(), legacy_save.as_deref())
        {
            LibraryPreferences::update(|saved| {
                saved
                    .legacy_save_bindings
                    .entry(game_id.clone())
                    .or_insert_with(|| LegacySaveBinding {
                        installation: game.id.clone(),
                        save_directory: settings.save_directory.clone(),
                    });
            })?;
            preferences = LibraryPreferences::load()?;
        }
    }
    let scope = save_scope(game, &settings, &preferences);
    let save_managed = game
        .game_id
        .as_deref()
        .and_then(crate::resolve_game_plugin)
        .is_some_and(|plugin| plugin.supports_save_profiles());
    Ok(InstallationContext {
        prefix: match runtime_prefix {
            Some(prefix) => prefix,
            None => installation_prefix(game, &settings)?,
        },
        saves: SaveContext {
            game_id: GameId::from(game.game_id.as_deref().unwrap_or("unmanaged")),
            scope,
            directory: if save_managed {
                settings.save_directory.clone()
            } else {
                None
            },
        },
        game: game.clone(),
        launch: settings,
    })
}

pub fn installation_prefix(
    game: &LibraryGame,
    settings: &LaunchSettings,
) -> Result<Option<PathBuf>> {
    if settings.prefix.is_some() || (settings.executable.is_some() && settings.runner.is_none()) {
        return Ok(settings.prefix.clone());
    }
    Ok(super::providers::heroic_prefix(game)?.or_else(|| {
        (game.store == Store::Steam)
            .then(|| steam_prefix(game.install_path.as_deref()?, &game.app_id))
            .flatten()
            .filter(|path| path.is_dir())
    }))
}

/// Known Steam-user layout with target-tree case matching. Unknown Wine user
/// layouts can be supplied explicitly through LaunchSettings::save_directory.
/// Prefix inference belongs to installation_prefix; None here means native.
pub fn steam_user_path(
    _install: &Path,
    prefix: Option<&Path>,
    _app_id: &str,
    relative: &Path,
) -> Option<PathBuf> {
    modde_core::fs::case_match_path(
        prefix?,
        &Path::new("drive_c/users/steamuser").join(relative),
    )
    .ok()
}

pub fn steam_prefix(install: &Path, app_id: &str) -> Option<PathBuf> {
    let common = install.parent()?;
    if common.file_name()? != "common" {
        return None;
    }
    let steamapps = common.parent()?;
    if steamapps.file_name()? != "steamapps" {
        return None;
    }
    Some(steamapps.join("compatdata").join(app_id).join("pfx"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wine_save_discovery_requires_the_resolved_runtime_prefix() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("steamapps/common/Game");
        let prefix = root.path().join("steamapps/compatdata/1/pfx");
        std::fs::create_dir_all(&install).unwrap();
        std::fs::create_dir_all(prefix.join("drive_c/users/steamuser/Documents/Saves")).unwrap();
        let relative = Path::new("Documents/Saves");
        assert!(steam_user_path(&install, None, "1", relative).is_none());
        assert_eq!(
            steam_user_path(&install, Some(&prefix), "1", relative),
            Some(prefix.join("drive_c/users/steamuser/Documents/Saves"))
        );
    }

    #[test]
    fn nested_save_roots_are_rejected_in_both_directions() {
        let root = tempfile::tempdir().unwrap();
        let a = LibraryGame::new(
            Store::Local,
            "unknown-a".into(),
            "A".into(),
            Some(root.path().join("a")),
        );
        let b = LibraryGame::new(
            Store::Local,
            "unknown-b".into(),
            "B".into(),
            Some(root.path().join("b")),
        );
        let games = [a.clone(), b.clone()];
        let mut preferences = LibraryPreferences::default();
        for (game, path) in [
            (&a, root.path().join("saves")),
            (&b, root.path().join("saves/character")),
        ] {
            preferences.launches.insert(
                game.id.clone(),
                LaunchSettings {
                    save_directory: Some(path),
                    ..Default::default()
                },
            );
        }
        for game in &games {
            assert!(effective_settings(game, &games, &preferences).is_err());
        }
        preferences.launches.get_mut(&b.id).unwrap().save_directory =
            Some(root.path().join("saves-other"));
        assert!(effective_settings(&a, &games, &preferences).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn save_overlap_checks_resolve_aliases_and_missing_parent_segments() {
        let root = tempfile::tempdir().unwrap();
        let saves = root.path().join("saves");
        std::fs::create_dir(&saves).unwrap();
        let alias = root.path().join("alias");
        std::os::unix::fs::symlink(&saves, &alias).unwrap();
        assert!(overlapping_paths(&saves, Some(&alias.join("child"))));
        assert!(overlapping_paths(
            &saves,
            Some(&alias.join("not-created/../child"))
        ));
        assert!(overlapping_paths(
            &saves,
            Some(&root.path().join("not-created/../saves/child"))
        ));
    }
}
