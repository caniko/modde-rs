#![allow(clippy::wildcard_imports)]
pub mod backup;
pub mod bisect;
pub mod collisions;
pub mod completions;
pub mod config;
pub mod crash;
pub mod deploy;
pub mod detect;
pub mod doctor;
pub mod export;
pub mod fomod;
pub mod game;
pub mod hot_deploy;
pub mod import;
pub mod install;
pub mod instance;
pub mod library;
pub mod lockfile;
#[cfg(feature = "bethesda")]
pub mod loot;
pub mod nexus;
pub mod nix_schema;
pub mod nxm;
pub mod patcher;
pub mod perf;
pub mod play;
pub mod profile;
pub mod rollback;
pub mod save;
pub mod scan;
pub mod skill;
pub mod stock;
pub mod tool;
pub mod uninstall;
pub mod update;
pub mod verify;
#[cfg(feature = "screenshot")]
pub mod visual;
pub mod wabbajack;

use std::path::PathBuf;

use anyhow::Result;

use modde_core::PluginEntry;
use modde_core::profile::{Profile, ProfileManager};
use modde_core::resolver::GameId;
use modde_core::save::SaveFingerprint;

pub(super) async fn installation_context(
    game_id: &str,
    pm: &ProfileManager,
) -> Result<modde_games::library::context::InstallationContext> {
    modde_games::library::context::for_game(
        &modde_core::settings::AppSettings::load(),
        game_id,
        pm.db(),
    )
    .await
}

/// Resolve the game's save directory via the `GamePlugin` trait.
///
/// Shared across profile and save commands to avoid duplication.
pub fn resolve_save_dir(game_id: &str) -> Option<PathBuf> {
    let plugin = modde_games::resolve_game_plugin(game_id)?;
    plugin
        .supports_save_profiles()
        .then(|| plugin.save_directory())
        .flatten()
}

/// Whether this game supports modde's per-profile save layer.
pub fn supports_save_profiles(game_id: &str) -> Result<bool> {
    let plugin = modde_games::resolve_game_plugin(game_id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown game '{}'. Supported games: {}",
            game_id,
            modde_games::supported_game_ids().join(", ")
        )
    })?;
    Ok(plugin.supports_save_profiles())
}

/// Resolve the game's save directory, returning an error if not found.
#[cfg(test)]
pub fn require_save_dir(game_id: &str) -> Result<PathBuf> {
    // First check the game ID is valid at all
    if modde_games::resolve_game_plugin(game_id).is_none() {
        anyhow::bail!(
            "unknown game '{}'. Supported games: {}",
            game_id,
            modde_games::supported_game_ids().join(", ")
        );
    }
    if !supports_save_profiles(game_id)? {
        anyhow::bail!(
            "save profiles are not supported for game '{game_id}'. \
             This title does not use modde's per-profile save layer."
        );
    }
    resolve_save_dir(game_id).ok_or_else(|| {
        anyhow::anyhow!(
            "save directory not found for game '{game_id}'. \
             The game may not be installed, or save tracking is not supported for this title."
        )
    })
}

/// Compute a save fingerprint for a profile by classifying its mods via the game plugin.
pub async fn compute_fingerprint(
    pm: &ProfileManager,
    name: &str,
    game_id: &str,
) -> Option<SaveFingerprint> {
    if !supports_save_profiles(game_id).ok()? {
        return None;
    }
    let profile = pm.load(name, Some(&GameId::from(game_id))).await.ok()?;
    Some(modde_games::save_fingerprint(&profile))
}

/// Load a profile by name (optional) and game (optional), falling back to
/// the first available profile when no name is given.
pub async fn load_profile_or_default(
    pm: &ProfileManager,
    name: Option<&str>,
    game_id: Option<&str>,
) -> Result<Profile> {
    if let Some(name) = name {
        Ok(pm.load(name, game_id.map(GameId::from).as_ref()).await?)
    } else {
        let profiles = pm.list().await?;
        let first = profiles.first().ok_or_else(|| {
            anyhow::anyhow!(
                "no profiles found. Create one with: modde profile create <name> --game <id>"
            )
        })?;
        Ok(pm.load(&first.name, Some(&first.game_id)).await?)
    }
}

/// Load the real plugin order for a profile, preferring the DB and falling back
/// to the game's native `plugins.txt` when the DB has not been populated yet.
pub async fn load_plugin_order(pm: &ProfileManager, profile: &Profile) -> Result<Vec<PluginEntry>> {
    let mut plugins = match profile.id {
        Some(profile_id) => pm.db().get_plugin_order(profile_id).await?,
        None => Vec::new(),
    };

    if plugins.is_empty() {
        plugins =
            modde_games::read_native_plugin_order(profile.game_id.as_str()).unwrap_or_default();
        if !plugins.is_empty()
            && let Some(profile_id) = profile.id
        {
            pm.db().set_plugin_order(profile_id, &plugins).await?;
        }
    }

    Ok(plugins)
}

/// Persist plugin order to both the DB and the game's native `plugins.txt`
/// when the current game supports a writable plugin list.
pub async fn persist_plugin_order(
    pm: &ProfileManager,
    profile: &Profile,
    plugins: &[PluginEntry],
) -> Result<()> {
    validate_native_record_references(profile, plugins)?;

    if let Some(profile_id) = profile.id {
        pm.db().set_plugin_order(profile_id, plugins).await?;
    }

    if modde_games::resolve_game_plugin(profile.game_id.as_str())
        .is_some_and(modde_games::GamePlugin::has_plugin_system)
    {
        modde_games::write_native_plugin_order(profile.game_id.as_str(), plugins)?;
    }

    Ok(())
}

#[cfg(feature = "bethesda")]
fn validate_native_record_references(profile: &Profile, plugins: &[PluginEntry]) -> Result<()> {
    if !matches!(
        profile.game_id.as_str(),
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" | "starfield"
    ) {
        return Ok(());
    }

    let active_plugins = plugins
        .iter()
        .filter(|plugin| plugin.enabled)
        .map(|plugin| plugin.plugin_name.as_str())
        .collect::<Vec<_>>();
    if active_plugins.is_empty() {
        return Ok(());
    }

    let staging = ProfileManager::staging_dir(&profile.name);
    let report = modde_games::bethesda::records::validate_record_references(
        &staging,
        &active_plugins,
        profile.game_id.as_str(),
    );
    if report.is_empty() {
        return Ok(());
    }

    let issue_count = report.unresolved.len() + report.parse_errors.len();
    let mut message = format!(
        "refusing to write native plugin order because validation found {issue_count} issue(s). Run `modde diagnostics {}` for details.",
        profile.game_id
    );
    for reference in report.unresolved.iter().take(5) {
        message.push_str(&format!("\n  [ERROR] {reference}"));
    }
    for failure in report.parse_errors.iter().take(5) {
        message.push_str(&format!(
            "\n  [ERROR] {}: {}",
            failure.plugin, failure.error
        ));
    }
    if issue_count > 10 {
        message.push_str(&format!("\n  ... and {} more", issue_count - 10));
    }
    if !report.unsupported_record_types.is_empty() {
        message.push_str(&format!(
            "\n  [INFO] Record validation is not exhaustive; unsupported record types present: {}",
            report.unsupported_record_types.join(", ")
        ));
    }
    anyhow::bail!(message)
}

#[cfg(not(feature = "bethesda"))]
fn validate_native_record_references(profile: &Profile, _plugins: &[PluginEntry]) -> Result<()> {
    anyhow::ensure!(
        !matches!(
            profile.game_id.as_str(),
            "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" | "starfield"
        ),
        "native record validation requires the bethesda feature"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_save_profiles_errors_for_unknown_game() {
        let err = supports_save_profiles("not-a-game").unwrap_err();
        assert!(err.to_string().contains("unknown game 'not-a-game'"));
    }

    #[test]
    fn stellar_blade_without_save_dir_reports_missing_directory() {
        assert!(resolve_save_dir("stellar-blade").is_none());
        let err = require_save_dir("stellar-blade").unwrap_err();
        if cfg!(feature = "ue4") {
            assert!(supports_save_profiles("stellar-blade").unwrap());
            assert!(
                err.to_string()
                    .contains("save directory not found for game 'stellar-blade'")
            );
        } else {
            assert!(
                supports_save_profiles("stellar-blade")
                    .unwrap_err()
                    .to_string()
                    .contains("unknown game 'stellar-blade'")
            );
            assert!(err.to_string().contains("unknown game 'stellar-blade'"));
        }
    }

    #[test]
    fn enabled_game_without_save_dir_reports_missing_directory() {
        if cfg!(feature = "bethesda") {
            assert!(supports_save_profiles("skyrim-se").unwrap());
            if resolve_save_dir("skyrim-se").is_none() {
                let err = require_save_dir("skyrim-se").unwrap_err();
                assert!(
                    err.to_string()
                        .contains("save directory not found for game 'skyrim-se'")
                );
            }
        } else {
            assert!(
                supports_save_profiles("skyrim-se")
                    .unwrap_err()
                    .to_string()
                    .contains("unknown game 'skyrim-se'")
            );
            assert!(resolve_save_dir("skyrim-se").is_none());
            let err = require_save_dir("skyrim-se").unwrap_err();
            assert!(err.to_string().contains("unknown game 'skyrim-se'"));
        }
    }
}
