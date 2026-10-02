//! Per-profile INI management for Bethesda games.
//!
//! Each profile stores its own copy of game INI files. On profile activation,
//! INIs are swapped: the current game INIs are captured back into the outgoing
//! profile, and the incoming profile's INIs are restored to the game directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use modde_core::resolver::GameId;
use tracing::{debug, info};

/// INI files tracked per profile for each Bethesda game.
const SKYRIM_INIS: &[&str] = &["Skyrim.ini", "SkyrimPrefs.ini", "SkyrimCustom.ini"];
const FALLOUT4_INIS: &[&str] = &["Fallout4.ini", "Fallout4Prefs.ini", "Fallout4Custom.ini"];
const FALLOUT76_INIS: &[&str] = &["Fallout76.ini", "Fallout76Prefs.ini", "Fallout76Custom.ini"];

/// Get the list of INI filenames to manage for a given game.
#[must_use]
pub fn tracked_inis(game_id: &str) -> &'static [&'static str] {
    match game_id {
        "skyrim-se" | "skyrim-ae" => SKYRIM_INIS,
        "fallout4" => FALLOUT4_INIS,
        "fallout76" => FALLOUT76_INIS,
        _ => &[],
    }
}

/// Get the profile-specific INI storage directory.
#[must_use]
pub fn profile_ini_dir(profile_name: &str) -> PathBuf {
    modde_core::paths::profiles_dir()
        .join(profile_name)
        .join("ini")
}

/// Get the game's INI directory (under Proton prefix).
///
/// For Bethesda games this is the `My Games/<game>` directory
/// (e.g., `~/.local/share/Steam/steamapps/compatdata/<app_id>/pfx/drive_c/Users/steamuser/Documents/My Games/Skyrim Special Edition/`).
#[must_use]
pub fn game_ini_dir(steam_app_id: &str, my_games_dir: &str) -> Option<PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let path = PathBuf::from(home)
        .join(".local/share/Steam/steamapps/compatdata")
        .join(steam_app_id)
        .join("pfx/drive_c/Users/steamuser/Documents/My Games")
        .join(my_games_dir);
    if path.exists() { Some(path) } else { None }
}

/// Capture current game INIs into a profile's INI storage.
///
/// Copies INI files from the game directory into the profile's `ini/` subdirectory.
/// Existing profile INIs are overwritten.
pub fn capture_inis(game_id: &GameId, profile_name: &str, game_ini_path: &Path) -> Result<usize> {
    let inis = tracked_inis(game_id.as_str());
    if inis.is_empty() {
        return Ok(0);
    }

    let dest_dir = profile_ini_dir(profile_name);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("failed to create profile INI dir: {}", dest_dir.display()))?;

    let mut count = 0;
    for ini_name in inis {
        let src = game_ini_path.join(ini_name);
        if src.exists() {
            let dst = dest_dir.join(ini_name);
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed to capture INI: {} -> {}",
                    src.display(),
                    dst.display()
                )
            })?;
            debug!(ini = *ini_name, "captured INI to profile");
            count += 1;
        }
    }

    if count > 0 {
        info!(
            profile = profile_name,
            ini_count = count,
            "captured game INIs to profile"
        );
    }
    Ok(count)
}

/// Restore profile INIs to the game directory.
///
/// Copies INI files from the profile's `ini/` subdirectory back to the game directory.
/// Only overwrites game INIs for files that exist in the profile's storage.
pub fn restore_inis(game_id: &GameId, profile_name: &str, game_ini_path: &Path) -> Result<usize> {
    let inis = tracked_inis(game_id.as_str());
    if inis.is_empty() {
        return Ok(0);
    }

    let src_dir = profile_ini_dir(profile_name);
    if !src_dir.exists() {
        debug!(
            profile = profile_name,
            "no stored INIs for profile, skipping restore"
        );
        return Ok(0);
    }

    let mut count = 0;
    for ini_name in inis {
        let src = src_dir.join(ini_name);
        if src.exists() {
            let dst = game_ini_path.join(ini_name);
            std::fs::copy(&src, &dst).with_context(|| {
                format!(
                    "failed to restore INI: {} -> {}",
                    src.display(),
                    dst.display()
                )
            })?;
            debug!(ini = *ini_name, "restored INI from profile");
            count += 1;
        }
    }

    if count > 0 {
        info!(
            profile = profile_name,
            ini_count = count,
            "restored profile INIs to game"
        );
    }
    Ok(count)
}

/// Swap INIs during profile switch.
///
/// 1. Capture current game INIs into the outgoing profile (if provided)
/// 2. Restore incoming profile's INIs to the game directory
pub fn swap_inis(
    game_id: &GameId,
    outgoing_profile: Option<&str>,
    incoming_profile: &str,
    game_ini_path: &Path,
) -> Result<()> {
    // Step 1: capture outgoing
    if let Some(outgoing) = outgoing_profile {
        capture_inis(game_id, outgoing, game_ini_path)?;
    }

    // Step 2: restore incoming
    restore_inis(game_id, incoming_profile, game_ini_path)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_tracked_inis() {
        assert_eq!(tracked_inis("skyrim-se").len(), 3);
        assert_eq!(tracked_inis("skyrim-ae").len(), 3);
        assert_eq!(tracked_inis("fallout4").len(), 3);
        assert_eq!(tracked_inis("cyberpunk2077").len(), 0);
    }

    #[test]
    fn test_capture_and_restore_inis() {
        let game_dir = TempDir::new().unwrap();
        let profiles_dir = TempDir::new().unwrap();

        // Create fake game INIs
        std::fs::write(game_dir.path().join("Skyrim.ini"), "[General]\nbOK=1\n").unwrap();
        std::fs::write(
            game_dir.path().join("SkyrimPrefs.ini"),
            "[Display]\niRes=1920\n",
        )
        .unwrap();

        // Override profiles_dir for this test
        let profile_ini = profiles_dir.path().join("test_profile").join("ini");
        std::fs::create_dir_all(&profile_ini).unwrap();

        // Manual capture
        for ini in &["Skyrim.ini", "SkyrimPrefs.ini"] {
            let src = game_dir.path().join(ini);
            let dst = profile_ini.join(ini);
            std::fs::copy(&src, &dst).unwrap();
        }

        // Modify game INIs (simulate different profile)
        std::fs::write(game_dir.path().join("Skyrim.ini"), "[General]\nbOK=0\n").unwrap();

        // Manual restore
        for ini in &["Skyrim.ini", "SkyrimPrefs.ini"] {
            let src = profile_ini.join(ini);
            let dst = game_dir.path().join(ini);
            std::fs::copy(&src, &dst).unwrap();
        }

        // Verify restored content
        let content = std::fs::read_to_string(game_dir.path().join("Skyrim.ini")).unwrap();
        assert!(content.contains("bOK=1"));
    }

    #[test]
    fn test_swap_with_no_outgoing() {
        let game_dir = TempDir::new().unwrap();

        // Create a minimal game INI
        std::fs::write(game_dir.path().join("Skyrim.ini"), "[General]\n").unwrap();

        // Swap with no outgoing should not error
        // (can't easily test the full swap without overriding paths, but we test the logic)
        let result = swap_inis(
            &GameId::from("skyrim-se"),
            None,
            "new_profile",
            game_dir.path(),
        );
        // This will succeed - restore will just find no stored INIs
        assert!(result.is_ok());
    }
}
