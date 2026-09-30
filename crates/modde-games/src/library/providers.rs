use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use modde_core::{library::atomic_json, paths};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Catalogue, LibraryGame, Store};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamSnapshot {
    pub account_id: String,
    pub refreshed_unix: u64,
    pub games: Vec<LibraryGame>,
}

fn steam_cache() -> PathBuf {
    paths::modde_data_dir().join("steam-owned.json")
}

/// Explicit network refresh. No credential is stored or included in errors.
pub async fn sync_steam(account_id: &str, api_key: &str) -> Result<usize> {
    if account_id.len() != 17 || !account_id.bytes().all(|byte| byte.is_ascii_digit()) {
        bail!("Steam account ID must be a 17-digit SteamID64");
    }
    if api_key.trim().is_empty() {
        bail!("set MODDE_STEAM_API_KEY to sync Steam ownership");
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()?;
    let response = client
        .get("https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/")
        .query(&[
            ("key", api_key),
            ("steamid", account_id),
            ("include_appinfo", "true"),
            ("include_played_free_games", "true"),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("Steam library request failed; cached ownership retained"))?;
    if !response.status().is_success() {
        bail!("Steam rejected the library request ({})", response.status());
    }
    let value: Value = response
        .json()
        .await
        .map_err(|_| anyhow::anyhow!("invalid Steam library response"))?;
    let games = parse_owned_steam(&value)?;
    let count = games.len();
    atomic_json(
        &steam_cache(),
        &SteamSnapshot {
            account_id: account_id.to_string(),
            refreshed_unix: SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),
            games,
        },
    )?;
    modde_core::library::LibraryPreferences::update(|preferences| {
        preferences.steam_id = Some(account_id.to_string());
    })?;
    Ok(count)
}

pub(super) fn parse_owned_steam(value: &Value) -> Result<Vec<LibraryGame>> {
    let response = value
        .get("response")
        .context("Steam response has no ownership data")?;
    let count = response
        .get("game_count")
        .and_then(Value::as_u64)
        .context("Steam ownership is unavailable (check account visibility and API credentials)")?;
    if count == 0 && response.get("games").is_none() {
        return Ok(Vec::new());
    }
    let games = response
        .get("games")
        .and_then(Value::as_array)
        .context("Steam games missing")?;
    if games.len() as u64 != count {
        bail!("Steam returned an incomplete ownership list");
    }
    games
        .iter()
        .map(|game| {
            let app_id = game
                .get("appid")
                .and_then(Value::as_u64)
                .context("Steam app ID missing")?
                .to_string();
            let name = game
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&app_id)
                .to_string();
            Ok(LibraryGame::new(Store::Steam, app_id, name, None))
        })
        .collect()
}

pub(super) fn steam(result: &mut Catalogue, account_id: Option<&str>) {
    match std::fs::read(steam_cache())
        .ok()
        .and_then(|bytes| serde_json::from_slice::<SteamSnapshot>(&bytes).ok())
    {
        Some(snapshot) if account_id == Some(snapshot.account_id.as_str()) => {
            result.notices.push(format!("Steam: cached owned library (synced at Unix {}); local installs refreshed separately.", snapshot.refreshed_unix));
            result.games.extend(snapshot.games);
        }
        _ => result.notices.push(
            "Steam: local installations only. Sync an account to include uninstalled owned games."
                .into(),
        ),
    }
    let mut libraries = paths::steam_library_folders();
    if let Some(default) = paths::steam_common().parent().and_then(Path::parent) {
        libraries.push(default.to_path_buf());
    }
    libraries.sort();
    libraries.dedup();
    for library in libraries {
        let steamapps = if library.join("steamapps").is_dir() {
            library.join("steamapps")
        } else {
            library
        };
        let Ok(files) = std::fs::read_dir(&steamapps) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if !crate::detection::steam::is_steam_appmanifest(&path) {
                continue;
            }
            let Some(manifest) = std::fs::read_to_string(&path)
                .ok()
                .and_then(|text| crate::detection::steam::parse_steam_appmanifest(&text))
            else {
                continue;
            };
            // A manifest cannot escape steamapps/common through an absolute or parent path.
            if Path::new(&manifest.installdir)
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
            {
                result.notices.push(format!(
                    "Steam: invalid install directory in {}",
                    path.display()
                ));
                continue;
            }
            let install = steamapps.join("common").join(manifest.installdir);
            if install.is_dir() {
                result.games.push(LibraryGame::new(
                    Store::Steam,
                    manifest.appid,
                    manifest.name,
                    Some(install),
                ));
            }
        }
    }
}

pub(super) fn heroic_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    roots.push(paths::user_config_dir().join("heroic"));
    #[cfg(target_os = "linux")]
    roots.push(paths::home_dir().join(".var/app/com.heroicgameslauncher.hgl/config/heroic"));
    roots.retain(|path| path.is_dir());
    roots
}

pub(super) fn heroic(result: &mut Catalogue) {
    let roots = heroic_roots();
    if roots.is_empty() {
        result.notices.push(
            "GOG/Epic: connect accounts in Heroic and refresh its libraries to import owned games."
                .into(),
        );
    }
    for root in roots {
        for (store, cache, key) in [
            (Store::Gog, "gog_library.json", "games"),
            (Store::Epic, "legendary_library.json", "library"),
        ] {
            let path = root.join("store_cache").join(cache);
            match read_json(&path).and_then(|value| parse_heroic_library(&value, store, key)) {
                Ok(games) => {
                    result.games.extend(games);
                    let age = path
                        .metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|time| SystemTime::now().duration_since(time).ok())
                        .map_or_else(
                            || "age unknown".into(),
                            |age| format!("{} hours old", age.as_secs() / 3600),
                        );
                    result.notices.push(format!("{}: cached Heroic ownership ({age}) from {}. Refresh/sign in through Heroic for account changes.", store.label(), root.display()));
                }
                Err(error) => result
                    .notices
                    .push(format!("{} ownership unavailable: {error}", store.label())),
            }
        }
        for (store, file) in [
            (Store::Gog, "gog_store/installed.json"),
            (Store::Epic, "legendaryConfig/legendary/installed.json"),
            (Store::Epic, "legendary_store/installed.json"),
            (Store::Sideload, "sideload_apps/installed.json"),
        ] {
            let path = root.join(file);
            if !path.exists() {
                continue;
            }
            match read_json(&path).and_then(|value| parse_heroic_installed(&value, store)) {
                Ok(games) => result.games.extend(games),
                Err(error) => result.notices.push(format!("{}: {error}", path.display())),
            }
        }
    }
}

fn read_json(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("invalid JSON in {}", path.display()))
}

/// Prefix selection comes from the matching installed provider record only.
/// The command itself still comes from Heroic's authenticated launch boundary.
pub fn heroic_config(game: &LibraryGame) -> Result<Option<PathBuf>> {
    heroic_config_at(game, &heroic_roots())
}

fn heroic_config_at(game: &LibraryGame, roots: &[PathBuf]) -> Result<Option<PathBuf>> {
    let files: &[&str] = match game.store {
        Store::Gog => &["gog_store/installed.json"],
        Store::Epic => &[
            "legendary_store/installed.json",
            "legendaryConfig/legendary/installed.json",
        ],
        Store::Sideload => &["sideload_apps/installed.json"],
        _ => return Ok(None),
    };
    if game.app_id.is_empty()
        || Path::new(&game.app_id).components().count() != 1
        || !matches!(
            Path::new(&game.app_id).components().next(),
            Some(Component::Normal(_))
        )
    {
        bail!("invalid Heroic game ID for configuration lookup");
    }
    let Some(install) = game.install_path.as_ref() else {
        return Ok(None);
    };
    let mut candidates = Vec::new();
    for root in roots {
        let installed = files
            .iter()
            .filter_map(|file| read_json(&root.join(file)).ok())
            .filter_map(|value| parse_heroic_installed(&value, game.store).ok())
            .flatten()
            .any(|entry| {
                entry.app_id == game.app_id
                    && entry
                        .install_path
                        .as_deref()
                        .map(modde_core::library::normalized_path)
                        == Some(modde_core::library::normalized_path(install))
            });
        if !installed {
            continue;
        }
        let config = root
            .join("GamesConfig")
            .join(format!("{}.json", game.app_id));
        if config.is_file() {
            candidates.push(config.canonicalize()?);
        }
    }
    candidates.sort();
    candidates.dedup();
    if candidates.len() > 1 {
        bail!(
            "multiple Heroic configurations target this installation; select one provider configuration before installing a hook"
        );
    }
    Ok(candidates.pop())
}

pub(super) fn heroic_prefix(game: &LibraryGame) -> Result<Option<PathBuf>> {
    let Some(path) = heroic_config(game)? else {
        return Ok(None);
    };
    let config = read_json(&path)?;
    let entry = config
        .get(&game.app_id)
        .context("Heroic game config missing")?;
    let prefix = heroic_prefix_from_settings(entry);
    if let Some(prefix) = &prefix {
        if !prefix.is_absolute() || (prefix.exists() && !prefix.is_dir()) {
            bail!("Heroic has an invalid Wine prefix in {}", path.display());
        }
    }
    Ok(prefix)
}

fn heroic_prefix_from_settings(entry: &Value) -> Option<PathBuf> {
    let root = entry
        .get("winePrefix")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    root.map(|root| {
        // Proton reads pfx even when drive_c also exists at the root. UMU can
        // retain an existing raw-Proton pfx directory, or alias pfx to its root.
        // Resolve that alias before save discovery and command validation.
        if entry["wineVersion"]["type"].as_str() == Some("proton")
            && (entry["disableUMU"].as_bool() == Some(true) || root.join("pfx").is_dir())
        {
            modde_core::library::normalized_path(&root.join("pfx"))
        } else {
            root
        }
    })
}

pub(super) fn parse_heroic_library(
    value: &Value,
    store: Store,
    key: &str,
) -> Result<Vec<LibraryGame>> {
    let games = value
        .get(key)
        .and_then(Value::as_array)
        .context("unrecognized Heroic ownership cache")?;
    games
        .iter()
        .map(|game| {
            let id = game
                .get("app_name")
                .and_then(Value::as_str)
                .context("Heroic app_name missing")?;
            let title = game.get("title").and_then(Value::as_str).unwrap_or(id);
            Ok(LibraryGame::new(
                store,
                id.to_string(),
                title.to_string(),
                None,
            ))
        })
        .collect()
}

pub(super) fn parse_heroic_installed(value: &Value, store: Store) -> Result<Vec<LibraryGame>> {
    let entries: Vec<(Option<&str>, &Value)> =
        if let Some(array) = value.get("installed").and_then(Value::as_array) {
            array.iter().map(|entry| (None, entry)).collect()
        } else if let Some(object) = value.as_object() {
            if !object.is_empty()
                && !object
                    .values()
                    .any(|entry| entry.get("install_path").is_some())
            {
                bail!("unrecognized Heroic installed catalogue");
            }
            object
                .iter()
                .map(|(key, entry)| (Some(key.as_str()), entry))
                .collect()
        } else {
            bail!("unrecognized Heroic installed catalogue");
        };
    let mut games = Vec::new();
    for (key, entry) in entries {
        let id = entry
            .get("appName")
            .or_else(|| entry.get("app_name"))
            .and_then(Value::as_str)
            .or(key);
        let Some(id) = id else {
            continue;
        };
        let Some(path) = entry
            .get("install_path")
            .and_then(Value::as_str)
            .map(PathBuf::from)
        else {
            continue;
        };
        if !path.is_absolute() || !path.is_dir() {
            continue;
        }
        let title = entry.get("title").and_then(Value::as_str).unwrap_or(id);
        games.push(LibraryGame::new(
            store,
            id.to_string(),
            title.to_string(),
            Some(path),
        ));
    }
    Ok(games)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heroic_proton_prefix_selection_follows_runner_and_existing_layout() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        let mut entry = serde_json::json!({"winePrefix": prefix, "wineVersion": {"type": "proton"}, "disableUMU": true});
        assert_eq!(
            heroic_prefix_from_settings(&entry),
            Some(prefix.join("pfx"))
        );
        entry["disableUMU"] = false.into();
        assert_eq!(heroic_prefix_from_settings(&entry), Some(prefix.clone()));
        std::fs::create_dir_all(prefix.join("pfx/drive_c")).unwrap();
        assert_eq!(
            heroic_prefix_from_settings(&entry),
            Some(prefix.join("pfx"))
        );
        // UMU does not replace a raw-Proton pfx directory with an alias, even
        // when an older Wine launch left a second drive_c at the container root.
        std::fs::create_dir(prefix.join("drive_c")).unwrap();
        assert_eq!(
            heroic_prefix_from_settings(&entry),
            Some(prefix.join("pfx"))
        );
        entry["wineVersion"]["type"] = "wine".into();
        assert_eq!(heroic_prefix_from_settings(&entry), Some(prefix));
    }

    #[cfg(unix)]
    #[test]
    fn heroic_umu_alias_preserves_the_first_run_save_destination() {
        let root = tempfile::tempdir().unwrap();
        let prefix = root.path().join("prefix");
        let entry = serde_json::json!({"winePrefix": prefix, "wineVersion": {"type": "proton"}, "disableUMU": false});
        let before = heroic_prefix_from_settings(&entry);
        std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
        std::os::unix::fs::symlink(".", prefix.join("pfx")).unwrap();
        assert_eq!(heroic_prefix_from_settings(&entry), before);
    }

    #[test]
    fn heroic_array_and_legendary_map_resolve_the_same_installed_copy() {
        let root = tempfile::tempdir().unwrap();
        let array = serde_json::json!({"installed": [{"appName": "game", "install_path": root.path(), "title": "Game"}]});
        let map = serde_json::json!({"game": {"install_path": root.path(), "title": "Game"}});
        assert_eq!(
            parse_heroic_installed(&array, Store::Epic).unwrap(),
            parse_heroic_installed(&map, Store::Epic).unwrap()
        );
        let missing =
            serde_json::json!({"game": {"install_path": root.path().join("uninstalled")}});
        assert!(
            parse_heroic_installed(&missing, Store::Epic)
                .unwrap()
                .is_empty()
        );
        assert!(
            parse_heroic_installed(&serde_json::json!({"unsupported": []}), Store::Epic).is_err()
        );
    }

    #[test]
    fn hook_lookup_matches_store_and_physical_installation_and_rejects_ambiguity() {
        let fixture = tempfile::tempdir().unwrap();
        let install = fixture.path().join("game");
        std::fs::create_dir(&install).unwrap();
        let roots = [
            fixture.path().join("native"),
            fixture.path().join("flatpak"),
        ];
        for root in &roots {
            atomic_json(
                &root.join("gog_store/installed.json"),
                &serde_json::json!({"installed": [{"appName": "42", "install_path": install}]}),
            )
            .unwrap();
            atomic_json(
                &root.join("GamesConfig/42.json"),
                &serde_json::json!({"42": {"winePrefix": fixture.path().join("new-prefix")}}),
            )
            .unwrap();
        }
        let game = LibraryGame::new(Store::Gog, "42".into(), "Game".into(), Some(install));
        assert_eq!(
            heroic_config_at(&game, &roots[..1]).unwrap(),
            Some(roots[0].join("GamesConfig/42.json").canonicalize().unwrap())
        );
        assert!(heroic_config_at(&game, &roots).is_err());
        let epic = LibraryGame {
            store: Store::Epic,
            ..game.clone()
        };
        assert!(heroic_config_at(&epic, &roots).unwrap().is_none());
        let other = LibraryGame {
            install_path: Some(fixture.path().join("other-copy")),
            ..game
        };
        assert!(heroic_config_at(&other, &roots).unwrap().is_none());
    }
}
