//! Logical user paths shared by native save discovery and the sandbox.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use modde_core::{library::LaunchSettings, paths};

#[derive(Debug, Clone)]
pub(super) struct RuntimePaths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub data: PathBuf,
    pub cache: PathBuf,
}

impl RuntimePaths {
    pub fn host() -> Self {
        Self { home: paths::home_dir(), config: paths::user_config_dir(), data: paths::data_dir(), cache: paths::cache_dir() }
    }

    pub fn resolve(settings: &LaunchSettings) -> Result<Self> {
        Self::host().with_settings(settings)
    }

    fn with_settings(&self, settings: &LaunchSettings) -> Result<Self> {
        let path = |key: &str, fallback: PathBuf| -> Result<PathBuf> {
            let path = settings.environment.get(key).map_or(fallback, PathBuf::from);
            if !path.is_absolute() || path.parent().is_none()
                || path.components().any(|part| matches!(part, std::path::Component::ParentDir)) {
                bail!("{key} must be an absolute directory below the filesystem root without '..'");
            }
            Ok(path)
        };
        let home = path("HOME", self.home.clone())?;
        // Preserve non-default inherited XDG roots, but move HOME-relative
        // defaults when a launch explicitly selects a different HOME.
        let default = |current: &Path, suffix: &str| {
            if current == self.home.join(suffix) { home.join(suffix) } else { current.to_path_buf() }
        };
        Ok(Self {
            config: path("XDG_CONFIG_HOME", default(&self.config, ".config"))?,
            data: path("XDG_DATA_HOME", default(&self.data, ".local/share"))?,
            cache: path("XDG_CACHE_HOME", default(&self.cache, ".cache"))?,
            home,
        })
    }

    /// Relocate only automatically discovered native paths. Explicit save
    /// directories and Wine-prefix paths already name their physical targets.
    pub fn relocate(&self, host: &Self, path: &Path) -> PathBuf {
        let mut roots = [(&host.config, &self.config), (&host.data, &self.data), (&host.cache, &self.cache), (&host.home, &self.home)];
        roots.sort_by_key(|(source, _)| std::cmp::Reverse(source.components().count()));
        for (source, target) in roots {
            if let Ok(relative) = path.strip_prefix(source) { return target.join(relative); }
        }
        path.to_path_buf()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    fn host() -> RuntimePaths {
        RuntimePaths {
            home: "/home/player".into(), config: "/custom/config".into(),
            data: "/custom/data".into(), cache: "/custom/cache".into(),
        }
    }

    #[test]
    fn custom_xdg_roots_survive_sandbox_environment_resolution() {
        let host = host();
        let effective = host.with_settings(&LaunchSettings::default()).unwrap();
        let save = host.config.join("StardewValley/Saves");
        assert_eq!(effective.config, host.config);
        assert_eq!(effective.data, host.data);
        assert_eq!(effective.relocate(&host, &save), save);
    }

    #[test]
    fn explicit_native_environment_moves_discovered_saves() {
        let host = host();
        let mut settings = LaunchSettings::default();
        settings.environment.insert("XDG_CONFIG_HOME".into(), "/per-game/config".into());
        let effective = host.with_settings(&settings).unwrap();
        assert_eq!(effective.relocate(&host, &host.config.join("StardewValley/Saves")), Path::new("/per-game/config/StardewValley/Saves"));
        settings.environment.insert("XDG_DATA_HOME".into(), "relative".into());
        assert!(host.with_settings(&settings).is_err());
    }
}
