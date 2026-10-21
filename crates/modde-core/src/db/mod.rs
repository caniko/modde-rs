//! Persistent storage for modde, backed by `SQLite` (default) or `PostgreSQL`.
//!
//! The public [`ModdeDb`] API is identical across both backends and async
//! throughout. Each method is written once against the internal `Db` executor
//! using portable SQL (`?` placeholders, `RETURNING id`, `ON CONFLICT … DO
//! UPDATE … EXCLUDED`, `lower(name)`, `bool` columns, and a `{NOW}` token);
//! the executor rewrites placeholders/`now()` per dialect. The only genuinely
//! per-backend code is schema creation and migration.

mod backend;
mod migrate;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use smallvec::SmallVec;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};

use crate::bisect::{BisectResult, BisectSaveSafety, BisectSession, BisectStatus, BisectStep};
use crate::crash::CrashCorrelationReport;
use crate::doctor::{DoctorProfileModSnapshot, ProfileSnapshotRow};
use crate::error::{CoreError, Result};
use crate::installer::{InstallMethod, InstallPlan, InstallStatus, StagedFile};
use crate::nexus_id::{NexusFileId, NexusModId};
use crate::patcher::{
    PatcherStageKind, PatcherStageOutputRow, PatcherStageRow, PatcherStageSettings,
};
use crate::performance::{
    PerformanceModSnapshot, PerformanceSample, PerformanceSummary, mod_set_hash,
};
use crate::profile::{EnabledMod, LoadOrderLock, LockReason, Profile, ProfileSource};
use crate::resolver::{GameId, LoadOrderRule, ModId};
use crate::settings::{AppSettings, DatabaseSettings, DbBackend};

pub use backend::Val;
use backend::{Db, DbRow, DbTx, vals};

/// Summary view of a profile (without loading all mods).
#[derive(Debug, Clone, PartialEq)]
pub struct ProfileSummary {
    pub id: i64,
    pub name: String,
    pub game_id: GameId,
    pub mod_count: usize,
    pub source_type: String,
}

/// A save file/directory assigned to a profile.
#[derive(Debug, Clone)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub label: Option<String>,
    pub assigned_at: String,
}

/// Metadata for a stock game snapshot stored in the database.
#[derive(Debug, Clone)]
pub struct SnapshotMeta {
    pub game_id: GameId,
    pub snapshot_path: PathBuf,
    pub tree_hash: String,
    pub file_count: usize,
    pub created_at: String,
}

/// A hidden file entry — prevents a specific file from a mod from being deployed.
#[derive(Debug, Clone)]
pub struct HiddenFile {
    pub mod_id: String,
    pub rel_path: String,
}

/// A plugin entry in the plugin load order (independent of mod install priority).
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub plugin_name: String,
    pub sort_index: i64,
    pub enabled: bool,
}

/// A mod category for organizing the mod list.
#[derive(Debug, Clone)]
pub struct ModCategory {
    pub id: Option<i64>,
    pub name: String,
    pub color: Option<String>,
    pub sort_index: i64,
}

/// Per-game tool configuration stored in the database.
#[derive(Debug, Clone)]
pub struct ToolConfigRow {
    pub tool_id: String,
    pub enabled: bool,
    pub settings_json: String,
}

/// One versioned tool settings node in the per-tool DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSettingHistoryNode {
    pub node_id: String,
    pub game_id: String,
    pub tool_id: String,
    pub enabled: bool,
    pub settings_json: String,
    pub reason: String,
    pub created_at: String,
    pub is_current: bool,
}

/// One parent-child edge in the per-tool settings DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSettingHistoryEdge {
    pub parent_node_id: String,
    pub child_node_id: String,
}

/// A file applied by a tool to a game directory.
#[derive(Debug, Clone)]
pub struct ToolAppliedFileRow {
    pub tool_id: String,
    pub rel_path: String,
}

/// A named executable launch target for a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableConfigRow {
    pub game_id: String,
    pub name: String,
    pub executable_path: PathBuf,
    pub arguments_json: String,
    pub working_dir: Option<PathBuf>,
    pub environment_json: String,
    pub wine_dll_overrides: Option<String>,
    pub output_mod: String,
    pub enabled: bool,
}

/// New performance capture run to persist before launch.
#[derive(Debug, Clone)]
pub struct NewPerformanceRun {
    pub run_id: String,
    pub game_id: GameId,
    pub profile_id: Option<i64>,
    pub profile_name: String,
    pub mod_snapshot: Vec<PerformanceModSnapshot>,
    pub experiment_depth: usize,
    pub label: Option<String>,
}

/// Stored performance capture run with parsed summary metrics when available.
#[derive(Debug, Clone, PartialEq)]
pub struct PerformanceRunRow {
    pub run_id: String,
    pub game_id: GameId,
    pub profile_id: Option<i64>,
    pub profile_name: String,
    pub mod_set_hash: String,
    pub mod_snapshot_json: String,
    pub experiment_depth: usize,
    pub label: Option<String>,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub mangohud_csv_path: Option<PathBuf>,
    pub exit_status: Option<i64>,
    pub summary: PerformanceSummary,
}

pub use crate::bisect::{NewBisectSession, NewBisectStep};

/// A configured patcher pipeline stage for one profile.
pub type PatcherConfigRow = PatcherStageRow;

/// SQLite/PostgreSQL-backed persistent storage for modde.
#[derive(Debug, Clone)]
pub struct ModdeDb {
    db: Db,
}

/// Build resolved `PostgreSQL` connection options from settings plus env.
///
/// The injected `env` accessor keeps this pure and deterministic for callers
/// such as `config show` and unit tests. `MODDE_DATABASE_URL` wins over all
/// discrete fields; otherwise each discrete env var wins over its setting.
#[cfg(feature = "postgres")]
pub fn build_pg_options(
    settings: &DatabaseSettings,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<sqlx::postgres::PgConnectOptions> {
    use sqlx::postgres::PgConnectOptions;

    let url = env("MODDE_DATABASE_URL").or_else(|| settings.url.clone());
    if let Some(url) = url {
        return Ok(PgConnectOptions::from_str(&url)?);
    }

    let mut opts = PgConnectOptions::new();

    if let Some(host) = env("MODDE_DATABASE_HOST").or_else(|| settings.host.clone()) {
        opts = opts.host(&host);
    }

    let port = match env("MODDE_DATABASE_PORT") {
        Some(raw) => Some(raw.parse::<u16>().map_err(|e| {
            CoreError::Other(
                format!("invalid MODDE_DATABASE_PORT value '{raw}': expected 0-65535 ({e})").into(),
            )
        })?),
        None => settings.port,
    };
    if let Some(port) = port {
        opts = opts.port(port);
    }

    // Home Manager exports MODDE_DATABASE_NAME; the settings field remains
    // `dbname` to match PostgreSQL terminology and existing TOML shape.
    let dbname = env("MODDE_DATABASE_NAME")
        .or_else(|| settings.dbname.clone())
        .ok_or_else(|| {
            CoreError::Other("postgres backend selected but no database name configured".into())
        })?;
    opts = opts.database(&dbname);

    if let Some(user) = env("MODDE_DATABASE_USER").or_else(|| settings.user.clone()) {
        opts = opts.username(&user);
    }

    Ok(opts)
}

#[cfg(feature = "postgres")]
pub fn describe_pg_options(opts: &sqlx::postgres::PgConnectOptions) -> String {
    let endpoint = opts
        .get_socket()
        .map(|socket| format!("socket={}", socket.display()))
        .unwrap_or_else(|| format!("host={}", opts.get_host()));
    let database = opts.get_database().unwrap_or("<unset>");

    format!(
        "{endpoint}, port={}, dbname={database}, user={}",
        opts.get_port(),
        opts.get_username()
    )
}

#[cfg(feature = "postgres")]
fn read_pg_password_file(path: &Path) -> Result<String> {
    let pw = std::fs::read_to_string(path).map_err(|e| {
        CoreError::Other(
            format!(
                "failed to read MODDE_DB_PASSWORD_FILE/database.password_file {}: {e}",
                path.display()
            )
            .into(),
        )
    })?;
    Ok(pw.trim().to_string())
}

mod bisect;
mod connection;
mod crash;
mod exec_perf;
mod mods;
mod patcher;
mod profile;
mod rows;
mod saves;
mod tools;

pub use rows::decode_install_method;
#[cfg(test)]
use rows::encode_install_method;

#[cfg(test)]
mod tests;
