use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::resolver::GameId;

static DATA_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();
static CONFIG_DIR_OVERRIDE: OnceLock<PathBuf> = OnceLock::new();

/// Set a custom data directory. Must be called before any path functions.
pub fn set_data_dir(path: PathBuf) {
    // First caller wins. In production this is invoked once at startup, so a
    // repeat is benign; making it idempotent also stops parallel tests — each
    // of which installs its own tempdir data dir — from racing on the OnceLock
    // and panicking the loser.
    let _ = DATA_DIR_OVERRIDE.set(path);
}

/// Override the base configuration directory before initializing any services.
/// Embedders and test harnesses can isolate configuration and session leases
/// without changing process-wide environment variables. First caller wins.
pub fn set_config_dir(path: PathBuf) {
    let _ = CONFIG_DIR_OVERRIDE.set(path);
}

/// Platform-aware base data directory.
///
/// - Linux: `$XDG_DATA_HOME` or `~/.local/share`
/// - macOS: `~/Library/Application Support`
/// - Windows: `%APPDATA%` (e.g. `C:\Users\X\AppData\Roaming`)
#[must_use]
pub fn data_dir() -> PathBuf {
    // Honor XDG override on Linux/BSD
    #[cfg(target_os = "linux")]
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        return PathBuf::from(xdg);
    }

    dirs::data_dir().unwrap_or_else(|| home_dir().join(".local/share"))
}

/// Platform-aware config directory.
///
/// - Linux: `$XDG_CONFIG_HOME` or `~/.config`
/// - macOS: `~/Library/Application Support`
/// - Windows: `%APPDATA%`
#[must_use]
pub fn config_dir() -> PathBuf {
    if let Some(dir) = CONFIG_DIR_OVERRIDE.get() {
        return dir.clone();
    }
    user_config_dir()
}

/// The desktop user's configuration root. A modde --config-dir override must
/// not redirect game saves, provider discovery, or audio authentication.
#[must_use]
pub fn user_config_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg);
    }

    dirs::config_dir().unwrap_or_else(|| home_dir().join(".config"))
}

/// Platform-aware cache directory.
///
/// - Linux: `$XDG_CACHE_HOME` or `~/.cache`
/// - macOS: `~/Library/Caches`
/// - Windows: `%LOCALAPPDATA%`
#[must_use]
pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg);
    }

    dirs::cache_dir().unwrap_or_else(|| home_dir().join(".cache"))
}

/// Root of modde's non-essential cache files: `<cache_dir>/modde/`.
#[must_use]
pub fn modde_cache_dir() -> PathBuf {
    cache_dir().join("modde")
}

/// Root of all modde data: `<data_dir>/modde/` or the overridden path.
pub fn modde_data_dir() -> PathBuf {
    if let Some(dir) = DATA_DIR_OVERRIDE.get() {
        return dir.clone();
    }
    active_instance_data_dir().unwrap_or_else(default_modde_data_dir)
}

fn default_modde_data_dir() -> PathBuf {
    data_dir().join("modde")
}

fn active_instance_data_dir() -> Option<PathBuf> {
    crate::instance::InstanceRegistry::load()
        .active_data_dir()
        .map(Path::to_path_buf)
}

/// Mod file store: `<modde_data>/store/`.
#[must_use]
pub fn store_dir() -> PathBuf {
    modde_data_dir().join("store")
}

/// Staging scratch space: `<modde_data>/staging/`.
#[must_use]
pub fn staging_dir() -> PathBuf {
    modde_data_dir().join("staging")
}

/// Profiles root: `<modde_data>/profiles/`.
#[must_use]
pub fn profiles_dir() -> PathBuf {
    modde_data_dir().join("profiles")
}

/// Downloads directory: `<modde_data>/downloads/`.
#[must_use]
pub fn downloads_dir() -> PathBuf {
    modde_data_dir().join("downloads")
}

/// Root of generated patcher outputs: `<modde_data>/generated/`.
#[must_use]
pub fn generated_dir() -> PathBuf {
    modde_data_dir().join("generated")
}

/// Stock game snapshots: `<modde_data>/stock/`.
#[must_use]
pub fn stock_dir() -> PathBuf {
    modde_data_dir().join("stock")
}

/// Root of all save vaults: `<modde_data>/saves/`.
#[must_use]
pub fn save_vaults_dir() -> PathBuf {
    modde_data_dir().join("saves")
}

/// Content-addressed cache of `.wabbajack` manifest source files.
/// See [`crate::manifest::wabbajack::cache_wabbajack_file`].
#[must_use]
pub fn wabbajack_cache_dir() -> PathBuf {
    modde_data_dir().join("wabbajack_cache")
}

/// Path to a cached `.wabbajack` file keyed by its `manifest_hash`.
#[must_use]
pub fn wabbajack_cache_path(manifest_hash: &str) -> PathBuf {
    wabbajack_cache_dir().join(format!("{manifest_hash}.wabbajack"))
}

/// Save vault (git repo) for a specific game: `<modde_data>/saves/<game_id>/`.
#[must_use]
pub fn save_vault_dir(game_id: &GameId) -> PathBuf {
    save_vaults_dir().join(game_id.as_str())
}

/// Default Steam install directory (platform-aware).
fn steam_install_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        home_dir().join(".local/share/Steam")
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library/Application Support/Steam")
    }
    #[cfg(target_os = "windows")]
    {
        steam_install_dir_windows()
    }
}

/// Read Steam install path from Windows registry, with fallback.
#[cfg(target_os = "windows")]
fn steam_install_dir_windows() -> PathBuf {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey(r"Software\Valve\Steam") {
        if let Ok(path) = key.get_value::<String, _>("SteamPath") {
            return PathBuf::from(path);
        }
    }
    PathBuf::from(r"C:\Program Files (x86)\Steam")
}

/// Default Steam common library path.
#[must_use]
pub fn steam_common() -> PathBuf {
    steam_install_dir().join("steamapps/common")
}

/// Parse Steam's `libraryfolders.vdf` and return all library paths.
///
/// Steam supports multiple library folders (e.g. separate drives).
/// Each entry has a `"path"` key pointing to the Steam library root;
/// game installs live under `<path>/steamapps/common/<game>/`.
#[must_use]
pub fn steam_library_folders() -> Vec<PathBuf> {
    let vdf_path = steam_install_dir().join("steamapps/libraryfolders.vdf");
    parse_library_folders_vdf(&vdf_path)
}

/// Parse a VDF-format `libraryfolders.vdf` file.
///
/// The format is Valve's `KeyValues` (not JSON). We do a lightweight parse
/// that extracts `"path"` values from numbered entries.
fn parse_library_folders_vdf(path: &Path) -> Vec<PathBuf> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return vec![],
    };

    let mut paths = Vec::new();
    // Match lines like:   "path"		"/data/nvme0/can/games/steamapps"
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("\"path\"") {
            // Value is the next quoted string
            let rest = rest.trim();
            if let Some(val) = extract_vdf_string(rest) {
                let lib_path = PathBuf::from(val);
                if lib_path.exists() {
                    paths.push(lib_path);
                }
            }
        }
    }

    // Ensure the default library is always included
    let default_lib = steam_install_dir();
    if !paths.iter().any(|p| p == &default_lib) && default_lib.exists() {
        paths.insert(0, default_lib);
    }

    paths
}

/// Extract a quoted string value from VDF: `"value"` → `value`
fn extract_vdf_string(s: &str) -> Option<&str> {
    let s = s.trim();
    let s = s.strip_prefix('"')?;
    let end = s.find('"')?;
    Some(&s[..end])
}

/// Heroic Games Launcher config directory (platform-aware).
///
/// - Linux: `~/.config/heroic`
/// - macOS: `~/Library/Application Support/heroic`
/// - Windows: `%APPDATA%\heroic`
#[must_use]
pub fn heroic_config_dir() -> Option<PathBuf> {
    let dir = config_dir().join("heroic");
    dir.is_dir().then_some(dir)
}

/// Heroic Games Launcher binary location (Windows only).
#[cfg(target_os = "windows")]
pub fn heroic_exe_path() -> Option<PathBuf> {
    let local_app = dirs::data_local_dir()?;
    let exe = local_app.join(r"Programs\heroic\Heroic.exe");
    exe.exists().then_some(exe)
}

/// `SQLite` database path: `<modde_data>/modde.db`.
#[must_use]
pub fn db_path() -> PathBuf {
    modde_data_dir().join("modde.db")
}

/// Modde config directory: `<config_dir>/modde/`.
#[must_use]
pub fn modde_config_dir() -> PathBuf {
    config_dir().join("modde")
}

#[must_use]
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| {
        #[cfg(unix)]
        {
            PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()))
        }
        #[cfg(windows)]
        {
            PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Temp".to_string()))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_registry_can_override_default_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let instance_dir = tmp.path().join("instance-data");

        let mut registry = crate::instance::InstanceRegistry::default();
        registry.instances.push(crate::instance::Instance {
            name: "portable".to_string(),
            data_dir: instance_dir.clone(),
            is_default: true,
        });
        registry.active = Some("portable".to_string());

        let resolved = registry.active_data_dir().map(Path::to_path_buf);
        assert_eq!(resolved, Some(instance_dir));
    }

    fn write_vdf(dir: &std::path::Path, content: &str) -> PathBuf {
        let path = dir.join("libraryfolders.vdf");
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn vdf_single_library() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("steam");
        std::fs::create_dir_all(&lib).unwrap();

        let vdf = format!(
            r#""libraryfolders"
{{
    "0"
    {{
        "path"		"{}"
    }}
}}"#,
            lib.display()
        );
        let vdf_path = write_vdf(tmp.path(), &vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        assert!(paths.contains(&lib), "expected {lib:?} in {paths:?}");
    }

    #[test]
    fn vdf_multiple_libraries() {
        let tmp = tempfile::tempdir().unwrap();
        let lib1 = tmp.path().join("lib1");
        let lib2 = tmp.path().join("lib2");
        std::fs::create_dir_all(&lib1).unwrap();
        std::fs::create_dir_all(&lib2).unwrap();

        // Use the canonical indented format (each "path" key on its own line)
        let vdf = format!(
            "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}",
            lib1.display(),
            lib2.display()
        );
        let vdf_path = write_vdf(tmp.path(), &vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        assert!(paths.contains(&lib1), "expected {lib1:?} in {paths:?}");
        assert!(paths.contains(&lib2), "expected {lib2:?} in {paths:?}");
    }

    #[test]
    fn vdf_nonexistent_paths_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        // Write a VDF pointing to a path that doesn't exist
        let vdf = r#""libraryfolders"
{
    "0" { "path"		"/nonexistent/path/to/steam" }
}"#;
        let vdf_path = write_vdf(tmp.path(), vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        // Nonexistent paths should not appear
        assert!(
            !paths
                .iter()
                .any(|p| p.to_string_lossy().contains("nonexistent"))
        );
    }

    #[test]
    fn vdf_missing_file_returns_empty() {
        let paths =
            parse_library_folders_vdf(std::path::Path::new("/nonexistent/libraryfolders.vdf"));
        // Should return empty vec, not panic
        assert!(paths.is_empty());
    }

    #[test]
    fn vdf_extra_fields_ignored() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = tmp.path().join("steam");
        std::fs::create_dir_all(&lib).unwrap();

        let vdf = format!(
            r#""libraryfolders"
{{
    "0"
    {{
        "path"		"{}"
        "label"		""
        "contentid"		"12345"
        "totalsize"		"0"
        "apps"
        {{
            "730"		"12345"
        }}
    }}
}}"#,
            lib.display()
        );
        let vdf_path = write_vdf(tmp.path(), &vdf);
        let paths = parse_library_folders_vdf(&vdf_path);
        // Should only contain the path, not contentid/totalsize/app IDs
        assert!(paths.contains(&lib));
        for p in &paths {
            assert!(p.exists(), "all returned paths must exist");
        }
    }

    #[test]
    fn extract_vdf_string_basic() {
        assert_eq!(extract_vdf_string(r#""hello""#), Some("hello"));
        assert_eq!(
            extract_vdf_string(r#"  "with spaces"  "#),
            Some("with spaces")
        );
        assert_eq!(
            extract_vdf_string(r#""/path/to/dir""#),
            Some("/path/to/dir")
        );
    }

    #[test]
    fn extract_vdf_string_no_quotes() {
        assert_eq!(extract_vdf_string("no quotes here"), None);
    }

    #[test]
    fn extract_vdf_string_unclosed_quote() {
        assert_eq!(extract_vdf_string(r#""unclosed"#), None);
    }
}
