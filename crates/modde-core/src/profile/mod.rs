use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

pub use crate::db::ProfileSummary;
mod manager;
pub use manager::ProfileManager;

use crate::error::{CoreError, Result};
use crate::installer::{InstallMethod, InstallStatus};
use crate::nexus_id::{NexusFileId, NexusModId};
use crate::resolver::{GameId, LoadOrderRule};

/// A mod entry within a profile.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnabledMod {
    pub mod_id: String,
    /// Human-readable display name shown in UI (falls back to `mod_id` if None).
    #[serde(default)]
    pub display_name: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub version: Option<String>,
    /// Stored FOMOD declarative config (TOML), if this mod was installed via FOMOD.
    ///
    /// Contains a serialized `fomod_oxide::DeclarativeConfig` that can be
    /// re-applied during deployment to reproduce the same FOMOD selections.
    #[serde(default)]
    pub fomod_config: Option<String>,

    // ── Nexus metadata (V2) ──────────────────────────────────────
    #[serde(default)]
    pub nexus_mod_id: Option<NexusModId>,
    #[serde(default)]
    pub nexus_file_id: Option<NexusFileId>,
    #[serde(default)]
    pub nexus_game_domain: Option<String>,
    #[serde(default)]
    pub installed_timestamp: Option<i64>,

    // ── Organization (V2) ────────────────────────────────────────
    #[serde(default)]
    pub category_id: Option<i64>,
    #[serde(default)]
    pub notes: Option<String>,
    /// User-defined tags for filtering and export.
    #[serde(
        default,
        deserialize_with = "tags_serde::deserialize",
        serialize_with = "tags_serde::serialize",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tags: Vec<String>,

    // ── Load order lock (V7) ─────────────────────────────────────
    /// Per-mod lock: when `Some`, this mod's position cannot be changed via
    /// `Message::ReorderMod`. Other mods may still move around it. See the
    /// profile-level `load_order_lock` on `Profile` for the whole-profile
    /// lock that takes precedence.
    #[serde(default)]
    pub lock: Option<LockReason>,

    // ── Installer metadata (V8) ──────────────────────────────────
    /// The detected install method for this mod. `None` for mods that
    /// predate the installer pipeline or were installed via paths that
    /// bypass analysis (Wabbajack directives).
    #[serde(
        default,
        deserialize_with = "install_method_serde::deserialize",
        serialize_with = "install_method_serde::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub install_method: Option<InstallMethod>,

    /// xxh64 hex digest of the source archive. Lets uninstall and dossier
    /// dumps correlate a mod row back to the original download.
    #[serde(default)]
    pub source_archive_hash: Option<String>,

    /// Where this mod is in the install lifecycle. `None` means legacy
    /// (pre-V8) — treat as `Installed` for display purposes.
    #[serde(
        default,
        deserialize_with = "install_status_serde::deserialize",
        serialize_with = "install_status_serde::serialize",
        skip_serializing_if = "Option::is_none"
    )]
    pub install_status: Option<InstallStatus>,
}

mod install_status_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::installer::InstallStatus;

    #[allow(clippy::ref_option)] // serde serialize_with requires &Option<T>
    pub fn serialize<S>(status: &Option<InstallStatus>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match status {
            Some(status) => serializer.serialize_some(status.as_str()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<InstallStatus>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Option::<String>::deserialize(deserializer)?;
        match raw.as_deref() {
            None | Some("") => Ok(None),
            Some(value) => InstallStatus::parse(value).map(Some).ok_or_else(|| {
                serde::de::Error::custom(format!("unknown install_status: {value}"))
            }),
        }
    }
}

mod install_method_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    use crate::installer::InstallMethod;

    #[allow(clippy::ref_option)] // serde serialize_with requires &Option<T>
    pub fn serialize<S>(method: &Option<InstallMethod>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match method {
            Some(method) => {
                let encoded = toml::to_string(method).map_err(serde::ser::Error::custom)?;
                serializer.serialize_some(&encoded)
            }
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<InstallMethod>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Option::<String>::deserialize(deserializer)?;
        match raw.as_deref() {
            None | Some("") => Ok(None),
            Some(value) => toml::from_str::<InstallMethod>(value)
                .map(Some)
                .map_err(serde::de::Error::custom),
        }
    }
}

mod tags_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Tags {
        JsonString(String),
        List(Vec<String>),
    }

    pub fn serialize<S>(tags: &[String], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded = serde_json::to_string(tags).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&encoded)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
    where
        D: Deserializer<'de>,
    {
        match Tags::deserialize(deserializer)? {
            Tags::JsonString(value) if value.is_empty() => Ok(Vec::new()),
            Tags::JsonString(value) => {
                serde_json::from_str::<Vec<String>>(&value).map_err(serde::de::Error::custom)
            }
            Tags::List(tags) => Ok(tags),
        }
    }
}

/// Source from which a profile was created.
///
/// This is now *provenance-only* metadata. Load-order business logic
/// (preventing reorder of Wabbajack / Collection / TOML-imported profiles)
/// is driven by [`LoadOrderLock`] on the profile, not by this field.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProfileSource {
    #[default]
    Manual,
    NexusCollection {
        slug: String,
        version: String,
    },
    Wabbajack {
        manifest_hash: String,
    },
}

/// Why a profile's load order (or an individual mod) is locked.
///
/// Stored both at the profile level (inside [`LoadOrderLock`]) and, for
/// per-mod pins, directly on [`EnabledMod::lock`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum LockReason {
    /// Locked because the profile was installed from a Wabbajack modlist.
    /// `manifest_hash` identifies which manifest so scans can verify
    /// provenance.
    Wabbajack { manifest_hash: String },
    /// Locked because the profile was installed from a Nexus Collection.
    NexusCollection { slug: String, version: String },
    /// Locked because the profile was imported from an authoritative TOML
    /// file (e.g. shared between machines). `source_path` records where it
    /// came from at import time.
    TomlImport { source_path: String },
    /// Locked explicitly by the user via the UI or CLI.
    Manual {
        #[serde(default)]
        note: Option<String>,
    },
}

/// A profile-level load order lock.
///
/// When `Profile::load_order_lock` is `Some(_)`, the entire mod order is
/// frozen: reorder attempts are refused by the UI message handler and
/// reorder buttons are disabled in the views. The user must explicitly
/// unlock the profile (or fork it with `--unlock`) to make changes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LoadOrderLock {
    pub reason: LockReason,
    /// ISO-8601 UTC timestamp captured at lock time (e.g. `"2026-04-10T14:23:00Z"`).
    pub locked_at: String,
}

impl LoadOrderLock {
    /// Construct a new lock with `locked_at` set to the current UTC time.
    #[must_use]
    pub fn now(reason: LockReason) -> Self {
        Self {
            reason,
            locked_at: current_utc_timestamp(),
        }
    }
}

/// Return an ISO-8601 UTC timestamp for "now", suitable for
/// [`LoadOrderLock::locked_at`]. Uses only `std::time` + integer math so we
/// don't take a chrono dependency for a single field. Produces values like
/// `"2026-04-10T14:23:07Z"` — stable, sortable, and widely-parseable.
fn current_utc_timestamp() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);

    // Break into (date, time-of-day).
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400) as u32;
    let (h, rem) = (sod / 3600, sod % 3600);
    let (m, s) = (rem / 60, rem % 60);

    // Howard Hinnant's civil_from_days (days since 1970-01-01 → Y-M-D).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y_off = era * 400 + i64::from(yoe);
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m_civ = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m_civ <= 2 { y_off + 1 } else { y_off };

    format!("{y:04}-{m_civ:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

/// A modding profile containing an ordered list of mods.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Database row ID (None for profiles not yet persisted).
    #[serde(skip)]
    pub id: Option<i64>,
    pub name: String,
    pub game_id: GameId,
    pub source: ProfileSource,
    pub mods: Vec<EnabledMod>,
    pub overrides: PathBuf,
    /// Load order rules — typically 0–10 per profile.
    /// `SmallVec<[_; 4]>` keeps ≤4 rules inline (no heap allocation).
    #[serde(default)]
    pub load_order_rules: SmallVec<[LoadOrderRule; 4]>,
    /// Profile-level load order lock (V7). When `Some`, the entire mod
    /// order is frozen and reorder operations are refused until the user
    /// explicitly unlocks the profile. See [`LoadOrderLock`] for details.
    #[serde(default)]
    pub load_order_lock: Option<LoadOrderLock>,
}

// ── Reorder enforcement ──────────────────────────────────────────
//
// Pure logic shared by the UI handler and the CLI/test harnesses for
// attempting to move a mod one step up or down within a profile. All
// enforcement rules (profile-level lock, per-mod lock, adjacent pin,
// list boundary) live here so every caller refuses identically.

/// Direction for [`try_reorder`]. Mirrors the UI message variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReorderDirection {
    Up,
    Down,
}

/// Why [`try_reorder`] refused to move a mod. Callers render these into
/// status messages / CLI errors as they see fit — the enum carries enough
/// structure to explain *why* without string-matching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReorderError {
    /// The whole profile is locked (Wabbajack/Collection/TomlImport/Manual).
    ProfileLocked { reason: LockReason },
    /// The target mod itself carries a per-mod pin.
    ModPinned { mod_id: String, reason: LockReason },
    /// The `mod_id` does not exist in `profile.mods`.
    ModNotFound { mod_id: String },
    /// The swap partner (one step up/down) is pinned — moving would
    /// shift it, violating its per-mod pin contract.
    AdjacentPinned {
        neighbor_id: String,
        reason: LockReason,
    },
    /// The mod is already at the top/bottom of the list.
    AtBoundary,
}

/// Attempt to move `mod_id` one step `direction` within `profile.mods`.
///
/// On success, mutates `profile.mods` in place (swap with the adjacent
/// entry) and returns `Ok(())`. On refusal, returns a structured
/// [`ReorderError`] without touching the profile.
///
/// Enforcement precedence (short-circuits on the first match):
/// 1. Profile-level `load_order_lock` → `ProfileLocked`
/// 2. Mod not found → `ModNotFound`
/// 3. Target mod has `lock` → `ModPinned`
/// 4. Target direction goes out of bounds → `AtBoundary`
/// 5. Adjacent (swap-partner) mod has `lock` → `AdjacentPinned`
///
/// The UI handler and `modde profile reorder` CLI path (if/when added)
/// both call this; see `crates/modde-ui/src/app.rs::Message::ReorderMod`.
pub fn try_reorder(
    profile: &mut Profile,
    mod_id: &str,
    direction: ReorderDirection,
) -> std::result::Result<(), ReorderError> {
    if let Some(lock) = profile.load_order_lock.as_ref() {
        return Err(ReorderError::ProfileLocked {
            reason: lock.reason.clone(),
        });
    }

    let idx = profile
        .mods
        .iter()
        .position(|m| m.mod_id == mod_id)
        .ok_or_else(|| ReorderError::ModNotFound {
            mod_id: mod_id.to_string(),
        })?;

    if let Some(reason) = profile.mods[idx].lock.as_ref() {
        return Err(ReorderError::ModPinned {
            mod_id: mod_id.to_string(),
            reason: reason.clone(),
        });
    }

    let target_idx = match direction {
        ReorderDirection::Up if idx > 0 => idx - 1,
        ReorderDirection::Down if idx + 1 < profile.mods.len() => idx + 1,
        _ => return Err(ReorderError::AtBoundary),
    };

    if let Some(reason) = profile.mods[target_idx].lock.as_ref() {
        return Err(ReorderError::AdjacentPinned {
            neighbor_id: profile.mods[target_idx].mod_id.clone(),
            reason: reason.clone(),
        });
    }

    profile.mods.swap(idx, target_idx);
    Ok(())
}

/// Validate that a profile name is safe for use as a filesystem directory
/// and is not empty or excessively long.
pub fn validate_profile_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CoreError::Validation("profile name cannot be empty".into()));
    }
    if name == "." || name == ".." {
        return Err(CoreError::Validation(
            format!("profile name cannot be '{name}'").into(),
        ));
    }
    if name.len() > 255 {
        return Err(CoreError::Validation(
            "profile name too long (max 255 characters)".into(),
        ));
    }
    // Check for filesystem-unsafe characters
    if name.contains(['/', '\\', '\0', ':', '*', '?', '"', '<', '>', '|']) {
        return Err(CoreError::Validation(
            "profile name contains invalid characters (/ \\ NUL : * ? \" < > |)".into(),
        ));
    }
    Ok(())
}

/// SQLite-backed profile manager.
#[derive(Debug, Clone, Copy, Default)]
pub struct ForkOptions {
    /// If `true`, strip both `Profile::load_order_lock` and every
    /// `EnabledMod.lock` from the new profile. The source is untouched.
    /// Use this for the "fork to diverge" workflow where the user wants
    /// to freely reorder a clone of a Wabbajack/Collection profile.
    pub unlock: bool,
}

/// Information about the currently active profile.
#[derive(Debug)]
pub struct ActiveProfileInfo {
    pub profile: Profile,
    pub experiment_depth: usize,
}

/// Result of activating a profile.
#[derive(Debug)]
pub enum ActivateResult {
    /// Profile was activated successfully.
    Activated,
    /// Existing saves need adoption before activation can proceed.
    AdoptionRequired { save_count: usize },
}
