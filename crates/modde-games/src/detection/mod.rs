//! Unified game detection across Steam and Heroic launchers.
//!
//! Scans all known launcher libraries and returns every detected game
//! installation, including launcher metadata. This allows the UI and CLI
//! to present a "pick your game" experience without manual path entry.

use std::path::{Path, PathBuf};
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
use std::process::Stdio;
use std::process::{Command, ExitStatus};
use std::sync::{LazyLock, RwLock};

use anyhow::{Context, Result};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::registry::{GameRegistration, launcher_games};
use modde_core::paths;
use modde_core::resolver::GameId;

static DETECTION_CACHE: LazyLock<RwLock<Option<Vec<DetectedGame>>>> =
    LazyLock::new(|| RwLock::new(None));

/// A game installation detected by scanning launcher libraries.
#[derive(Debug, Clone)]
pub struct DetectedGame {
    /// The modde `game_id` (e.g. "skyrim-se", "cyberpunk2077").
    pub game_id: &'static str,
    /// Human-readable display name.
    pub display_name: &'static str,
    /// Absolute path to the game's install directory.
    pub install_path: PathBuf,
    /// Which launcher owns this installation.
    pub source: LauncherSource,
}

/// Which launcher/store a detected game belongs to.
#[derive(Debug, Clone)]
pub enum LauncherSource {
    Steam {
        app_id: String,
        library_path: PathBuf,
    },
    HeroicGog {
        app_id: String,
    },
    HeroicEpic {
        app_id: String,
    },
    HeroicSideload {
        app_id: String,
    },
}

impl LauncherSource {
    fn label_and_id(&self) -> (&str, &str) {
        match self {
            LauncherSource::Steam { app_id, .. } => ("Steam", app_id),
            LauncherSource::HeroicGog { app_id } => ("Heroic/GOG", app_id),
            LauncherSource::HeroicEpic { app_id } => ("Heroic/Epic", app_id),
            LauncherSource::HeroicSideload { app_id } => ("Heroic/Sideload", app_id),
        }
    }

    /// Launch the game via its detected launcher.
    ///
    /// Returns `Ok(Some(ExitStatus))` if we could wait for the game process to exit
    /// (Heroic), or `Ok(None)` for fire-and-forget launchers (Steam).
    pub fn launch(&self) -> Result<Option<ExitStatus>> {
        self.launch_with_env(&[])
    }

    /// Launch the game with extra environment variables where the launcher
    /// supports an observable child process.
    ///
    /// Steam URI launches remain fire-and-forget; callers that require reliable
    /// post-run ingestion should treat `Ok(None)` as pending.
    pub fn launch_with_env(&self, env_vars: &[(String, String)]) -> Result<Option<ExitStatus>> {
        match self {
            LauncherSource::Steam { app_id, .. } => {
                let url = format!("steam://rungameid/{app_id}");
                info!(%url, "launching via Steam");
                open::that(&url)
                    .with_context(|| format!("failed to launch Steam via URI ({url})"))?;
                Ok(None)
            }
            LauncherSource::HeroicGog { app_id }
            | LauncherSource::HeroicEpic { app_id }
            | LauncherSource::HeroicSideload { app_id } => {
                let (bin, base_args) = heroic_command()
                    .context("Heroic Games Launcher not found (checked flatpak and PATH)")?;
                info!(%bin, %app_id, "launching via Heroic");
                let mut cmd = Command::new(&bin);
                for arg in &base_args {
                    cmd.arg(arg);
                }
                for (key, value) in env_vars {
                    cmd.env(key, value);
                }
                let status = cmd
                    .args(["--no-gui", "--launch", app_id])
                    .status()
                    .with_context(|| {
                        format!("failed to launch Heroic ({bin} --no-gui --launch {app_id})")
                    })?;
                Ok(Some(status))
            }
        }
    }
}

impl std::fmt::Display for LauncherSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (label, id) = self.label_and_id();
        write!(f, "{label} ({id})")
    }
}

/// Detect the Heroic Games Launcher binary.
///
/// - Linux: checks flatpak first, then native binary on `$PATH`
/// - macOS: checks `/Applications/Heroic.app`, then `$PATH`
/// - Windows: checks standard install path, then `%PATH%`
///
/// Returns `(binary, base_args)` — e.g. `("flatpak", ["run", "com.heroicgameslauncher.hgl"])`
/// or `("heroic", [])`.
fn heroic_command() -> Option<(String, Vec<String>)> {
    #[cfg(all(target_os = "linux", feature = "linux-integrations"))]
    {
        // Check flatpak first (common on NixOS / immutable distros)
        if Command::new("flatpak")
            .args(["info", "com.heroicgameslauncher.hgl"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
        {
            return Some((
                "flatpak".to_string(),
                vec!["run".to_string(), "com.heroicgameslauncher.hgl".to_string()],
            ));
        }

        // Check native binary on PATH
        if let Ok(path) = which::which("heroic") {
            return Some((path.to_string_lossy().to_string(), vec![]));
        }

        None
    }

    #[cfg(all(target_os = "macos", feature = "macos-integrations"))]
    {
        let app_path = "/Applications/Heroic.app/Contents/MacOS/Heroic";
        if std::path::Path::new(app_path).exists() {
            return Some((app_path.to_string(), vec![]));
        }
        if let Ok(path) = which::which("heroic") {
            return Some((path.to_string_lossy().to_string(), vec![]));
        }
        None
    }

    #[cfg(all(target_os = "windows", feature = "windows-integrations"))]
    {
        if let Some(exe) = modde_core::paths::heroic_exe_path() {
            return Some((exe.to_string_lossy().to_string(), vec![]));
        }
        if let Ok(path) = which::which("heroic") {
            return Some((path.to_string_lossy().to_string(), vec![]));
        }
        None
    }

    #[cfg(not(any(
        all(target_os = "linux", feature = "linux-integrations"),
        all(target_os = "macos", feature = "macos-integrations"),
        all(target_os = "windows", feature = "windows-integrations"),
    )))]
    {
        None
    }
}

/// Find a detected game by its modde `game_id`.
///
/// Convenience wrapper that returns the first match from the latest detection
/// scan, performing one if no cached result exists yet.
#[must_use]
pub fn find_detected_game(game_id: &GameId) -> Option<DetectedGame> {
    cached_installed_games()
        .into_iter()
        .find(|g| game_id.as_str() == g.game_id)
}

/// Scan all known launchers for installed games.
///
/// Returns every detected game with its install path and launcher source.
/// A game may appear multiple times if installed via different launchers.
#[must_use]
pub fn scan_installed_games() -> Vec<DetectedGame> {
    let mut detected = Vec::new();

    scan_steam_libraries(&mut detected);
    scan_heroic_stores(&mut detected);

    update_detection_cache(&detected);

    detected
}

fn cached_installed_games() -> Vec<DetectedGame> {
    if let Ok(cache) = DETECTION_CACHE.read()
        && let Some(detected) = cache.as_ref()
    {
        return detected.clone();
    }

    scan_installed_games()
}

fn update_detection_cache(detected: &[DetectedGame]) {
    if let Ok(mut cache) = DETECTION_CACHE.write() {
        *cache = Some(detected.to_vec());
    }
}

mod heroic;
pub(crate) mod steam;

#[cfg(test)]
mod tests;

use heroic::scan_heroic_stores;
use steam::scan_steam_libraries;

#[cfg(test)]
use heroic::*;
#[cfg(test)]
use steam::*;

/// Find the install path for a specific game by scanning all launchers.
///
/// This is used by `GamePlugin::detect_install()` implementations to check
/// all available sources instead of just hardcoded paths.
#[must_use]
pub fn find_game_install(game_id: &GameId) -> Option<PathBuf> {
    // Check settings override first
    let settings = modde_core::settings::AppSettings::load();
    if let Some(path) = settings.game_path(game_id)
        && path.is_dir()
    {
        return Some(path.clone());
    }

    // Use the latest scan when available to avoid repeatedly scanning every
    // launcher while the UI resolves supported games one by one.
    cached_installed_games()
        .into_iter()
        .find(|g| game_id.as_str() == g.game_id)
        .map(|g| g.install_path)
}
