//! Saved launch preferences, separate from account ownership and mod profiles.
//!
//! Favorites follow the store entitlement; commands and sandbox permissions
//! belong to an exact installation. Missing settings use launcher defaults.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct LibraryPreferences {
    pub version: u32,
    pub favorites: BTreeSet<String>,
    pub launches: BTreeMap<String, LaunchSettings>,
    /// Installation IDs survive provider reassociation and known path aliases.
    pub installations: BTreeMap<String, InstallationIdentity>,
    /// Interrupted deployment must be replayed before another managed launch.
    pub needs_deploy: BTreeSet<String>,
    /// The unique existing installation keeps its legacy save vault. Additional
    /// installs receive independent active-profile and vault keys.
    pub legacy_save_bindings: BTreeMap<String, LegacySaveBinding>,
    /// Public account identifier only. API credentials are never persisted here.
    pub steam_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacySaveBinding {
    pub installation: String,
    pub save_directory: Option<PathBuf>,
}

impl Default for LibraryPreferences {
    fn default() -> Self {
        Self {
            version: 1,
            favorites: BTreeSet::new(),
            launches: BTreeMap::new(),
            installations: BTreeMap::new(),
            needs_deploy: BTreeSet::new(),
            legacy_save_bindings: BTreeMap::new(),
            steam_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct LaunchSettings {
    /// None delegates to the store. An absolute executable opts into direct launch.
    pub executable: Option<PathBuf>,
    /// Absolute Wine/umu runner. Native executables leave this empty.
    pub runner: Option<PathBuf>,
    pub prefix: Option<PathBuf>,
    /// Argument vector; never interpreted by a shell.
    pub arguments: Vec<String>,
    /// Outer-to-inner wrapper argument vectors (for example gamescope).
    pub wrappers: Vec<Vec<String>>,
    pub environment: BTreeMap<String, String>,
    pub working_directory: Option<PathBuf>,
    /// Stable PCI render node for Mesa game rendering. None uses the host default.
    pub gpu_render_node: Option<PathBuf>,
    /// Named mod profile, taking precedence over the active-profile fallback.
    pub profile: Option<String>,
    /// With no named profile, use the installation's active profile. Set false
    /// to explicitly skip profile deployment, save switching and capture.
    pub use_active_profile: bool,
    /// Explicit saves location for this installation (required for duplicate installs).
    pub save_directory: Option<PathBuf>,
    pub sandbox: SandboxSettings,
    /// A generated wrapper is installed at the store's actual command boundary.
    pub store_hook: bool,
    /// Operator assertion: Steam Cloud is disabled for this entitlement. Steam
    /// does not expose an authoritative offline per-game cloud policy API.
    pub steam_cloud_disabled: bool,
}

impl Default for LaunchSettings {
    fn default() -> Self {
        Self {
            executable: None,
            runner: None,
            prefix: None,
            arguments: Vec::new(),
            wrappers: Vec::new(),
            environment: BTreeMap::new(),
            working_directory: None,
            gpu_render_node: None,
            profile: None,
            use_active_profile: true,
            save_directory: None,
            sandbox: SandboxSettings::default(),
            store_hook: false,
            steam_cloud_disabled: false,
        }
    }
}

/// Profile definitions belong to a game; live state and vaults belong to an
/// installation/save destination. Never use the scope to load a profile.
#[derive(Debug, Clone)]
pub struct SaveContext {
    pub game_id: crate::GameId,
    pub scope: crate::GameId,
    pub directory: Option<PathBuf>,
}

impl SaveContext {
    pub fn legacy(game_id: &crate::GameId, directory: Option<&Path>) -> Self {
        Self {
            game_id: game_id.clone(),
            scope: game_id.clone(),
            directory: directory.map(Path::to_path_buf),
        }
    }
}

/// Persisted through preparation, observed descendant exit and save capture (or
/// explicit completion of an unobserved handoff). A crash must not permit the
/// next launch to replace live saves from an unfinished session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingSession {
    pub installation: String,
    pub name: String,
    pub game_id: Option<String>,
    pub scope: String,
    pub profile: Option<String>,
    pub save_directory: Option<PathBuf>,
    pub capture: bool,
    #[serde(default)]
    pub phase: SessionPhase,
    #[serde(default)]
    pub previous_profile: Option<(i64, String)>,
    #[serde(default)]
    pub switched_profile: bool,
    #[serde(default)]
    pub deployment_started: bool,
    #[serde(default)]
    pub data_directory: Option<PathBuf>,
    #[serde(default)]
    pub install_path: Option<PathBuf>,
    #[serde(default)]
    pub prefix: Option<PathBuf>,
    #[serde(default)]
    pub save_transition: Option<SaveTransition>,
    #[serde(default)]
    pub observation: Option<SessionObservation>,
    /// Private correlated diagnostics, allocated before preparation.
    #[serde(default)]
    pub diagnostics: Option<PathBuf>,
    /// Store handoff and one-run options, retained for interrupted completion.
    #[serde(default)]
    pub launch_request: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionObservation {
    pub directory: PathBuf,
    pub boot_id: String,
    pub unit: Option<String>,
}

pub mod diagnostics;
mod session;
pub use session::SessionPhase;
mod save_transition;
pub use save_transition::{ExperimentChange, SaveTransition};

pub fn mutation_lock() -> Result<File> {
    lock_file(&crate::paths::modde_config_dir().join("sessions/mutation.lock"))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct SandboxSettings {
    pub enabled: bool,
    pub network: bool,
    pub read_only: Vec<PathBuf>,
    pub writable: Vec<PathBuf>,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            network: true,
            read_only: Vec::new(),
            writable: Vec::new(),
        }
    }
}

impl LibraryPreferences {
    pub fn path() -> PathBuf {
        crate::paths::modde_data_dir().join("library.json")
    }

    pub fn load() -> Result<Self> {
        Self::load_at(&Self::path())
    }

    pub fn load_at(path: &Path) -> Result<Self> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error).context("reading library preferences"),
        };
        let settings: Self =
            serde_json::from_slice(&bytes).context("invalid library preferences")?;
        if settings.version != 1 {
            bail!(
                "unsupported library preferences version {}",
                settings.version
            );
        }
        Ok(settings)
    }

    pub fn launch_for(&self, id: &str) -> LaunchSettings {
        self.launches.get(id).cloned().unwrap_or_default()
    }

    pub fn update(edit: impl FnOnce(&mut Self)) -> Result<()> {
        Self::update_at(&Self::path(), edit)
    }

    pub fn update_at(path: &Path, edit: impl FnOnce(&mut Self)) -> Result<()> {
        Self::try_update_at(path, |settings| {
            edit(settings);
            Ok(())
        })
    }

    pub fn try_update_at<T>(path: &Path, edit: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let _guard = lock_file(&path.with_extension("lock"))?;
        let mut settings = Self::load_at(path)?;
        let before = settings.clone();
        let result = edit(&mut settings)?;
        if settings != before {
            atomic_json(path, &settings)?;
        }
        Ok(result)
    }
}

mod identity;
pub use identity::{InstallationIdentity, normalized_path};

/// Hash the exact byte representation, avoiding lossy path collisions and path
/// separators in persisted identifiers. Relocation intentionally needs rebinding.
pub fn installation_id(entitlement: &str, path: &Path) -> String {
    let normalized = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut hash = Sha256::new();
    hash.update(entitlement.as_bytes());
    hash.update([0]);
    hash.update(normalized.as_os_str().as_encoded_bytes());
    let mut id = String::from("install-");
    use std::fmt::Write as _;
    for byte in hash.finalize() {
        let _ = write!(id, "{byte:02x}");
    }
    id
}

/// Advisory resource lock. Never unlink a lock file: that would permit two
/// processes to lock different inodes for the same resource.
pub fn lock_file(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock()
        .map_err(|_| anyhow::anyhow!("resource is busy: {}", path.display()))?;
    Ok(file)
}

pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("JSON path has no parent")?;
    std::fs::create_dir_all(parent)?;
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| anyhow::anyhow!("random filename: {error}"))?;
    let temporary = parent.join(format!(".modde-{:x}.tmp", u128::from_ne_bytes(random)));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| -> Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        #[cfg(unix)]
        File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests;
