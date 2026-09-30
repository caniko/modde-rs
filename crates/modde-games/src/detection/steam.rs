#![allow(clippy::wildcard_imports)]
use super::*;

/// Scan all Steam library folders for known games.
pub(super) fn scan_steam_libraries(detected: &mut Vec<DetectedGame>) {
    let libraries = paths::steam_library_folders();

    for lib_path in &libraries {
        scan_steam_library(lib_path, detected);
    }
}

pub(super) fn scan_steam_library(lib_path: &Path, detected: &mut Vec<DetectedGame>) {
    for steamapps_dir in steamapps_dir_candidates(lib_path) {
        scan_steam_appmanifests(lib_path, &steamapps_dir, detected);
        scan_steam_common_fallback(lib_path, &steamapps_dir, detected);
    }
}

pub(super) fn steamapps_dir_candidates(lib_path: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    push_unique_existing_dir(&mut candidates, lib_path.join("steamapps"));
    push_unique_existing_dir(&mut candidates, lib_path.to_path_buf());
    candidates
}

pub(super) fn push_unique_existing_dir(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_dir() && !paths.iter().any(|p| p == &path) {
        paths.push(path);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SteamAppManifest {
    pub(crate) appid: String,
    pub(crate) name: String,
    pub(crate) installdir: String,
}

pub(super) fn scan_steam_appmanifests(
    library_path: &Path,
    steamapps_dir: &Path,
    detected: &mut Vec<DetectedGame>,
) {
    let manifests = match std::fs::read_dir(steamapps_dir) {
        Ok(entries) => entries,
        Err(e) => {
            debug!(error = %e, path = %steamapps_dir.display(), "failed to read Steam library");
            return;
        }
    };

    for entry in manifests.flatten() {
        let path = entry.path();
        if !is_steam_appmanifest(&path) {
            continue;
        }
        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(e) => {
                debug!(error = %e, path = %path.display(), "failed to read Steam appmanifest");
                continue;
            }
        };
        let Some(manifest) = parse_steam_appmanifest(&content) else {
            debug!(path = %path.display(), "failed to parse Steam appmanifest");
            continue;
        };
        let Some(game) = launcher_games()
            .find(|game| game.launcher.steam_app_id == Some(manifest.appid.as_str()))
        else {
            continue;
        };
        let install_path = steamapps_dir.join("common").join(&manifest.installdir);
        if install_path.is_dir() {
            push_steam_detected_game(
                detected,
                game,
                install_path,
                manifest.appid,
                library_path.to_path_buf(),
                "detected Steam game from appmanifest",
            );
        } else {
            debug!(
                game_id = game.game_id,
                app_id = %manifest.appid,
                name = %manifest.name,
                path = %install_path.display(),
                "Steam appmanifest install path does not exist"
            );
        }
    }
}

pub(crate) fn is_steam_appmanifest(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.starts_with("appmanifest_") && file_name.ends_with(".acf")
}

pub(crate) fn parse_steam_appmanifest(content: &str) -> Option<SteamAppManifest> {
    let mut appid = None;
    let mut name = None;
    let mut installdir = None;

    for line in content.lines() {
        let Some((key, value)) = parse_vdf_key_value(line) else {
            continue;
        };
        match key {
            "appid" => appid = Some(value.to_string()),
            "name" => name = Some(value.to_string()),
            "installdir" => installdir = Some(value.to_string()),
            _ => {}
        }
    }

    Some(SteamAppManifest {
        appid: appid?,
        name: name?,
        installdir: installdir?,
    })
}

pub(super) fn parse_vdf_key_value(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    let rest = line.strip_prefix('"')?;
    let key_end = rest.find('"')?;
    let key = &rest[..key_end];
    let rest = rest[key_end + 1..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let value_end = rest.find('"')?;
    Some((key, &rest[..value_end]))
}

pub(super) fn scan_steam_common_fallback(
    library_path: &Path,
    steamapps_dir: &Path,
    detected: &mut Vec<DetectedGame>,
) {
    let common_dir = steamapps_dir.join("common");
    if !common_dir.is_dir() {
        return;
    }

    for game in launcher_games() {
        let Some(steam_dir) = game.launcher.steam_dir else {
            continue;
        };

        let install_path = common_dir.join(steam_dir);
        if install_path.is_dir() {
            push_steam_detected_game(
                detected,
                game,
                install_path,
                game.launcher.steam_app_id.unwrap_or("unknown").to_string(),
                library_path.to_path_buf(),
                "detected Steam game from common directory fallback",
            );
        }
    }
}

pub(super) fn push_steam_detected_game(
    detected: &mut Vec<DetectedGame>,
    game: &GameRegistration,
    install_path: PathBuf,
    app_id: String,
    library_path: PathBuf,
    message: &'static str,
) {
    if detected
        .iter()
        .any(|detected| detected.game_id == game.game_id && detected.install_path == install_path)
    {
        return;
    }

    debug!(
        game_id = game.game_id,
        path = %install_path.display(),
        message
    );
    detected.push(DetectedGame {
        game_id: game.game_id,
        display_name: game.display_name,
        install_path,
        source: LauncherSource::Steam {
            app_id,
            library_path,
        },
    });
}
