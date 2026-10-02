//! The Wabbajack install engine: drives the full pipeline of downloading
//! archives, extracting and patching files, repacking BSAs, and writing the
//! finished modlist into the install directory, with resumable apply state.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tracing::{info, warn};

use modde_core::manifest::wabbajack::{
    ArchiveInstallBatch, ArchiveState, DownloadDirective, InstallDirective, WabbajackManifest,
};

use crate::cache::{ByteCacheKey, ByteLruCache};
use crate::decompress::{ArchiveBatchExtractor, ArchiveInput, ArchiveRequest, ArchiveRequestKind};
use crate::traits::{AnySource, DownloadSource};

#[cfg(feature = "bethesda-archives")]
use super::bsa_repack;
use super::diagnostics::{
    ArchiveBatchRecord, ProcessSnapshot, ProgressEvent, WabbajackDiagnostics,
    cgroup_memory_pressure_high, current_process_snapshot,
};
use super::impact::{MissingArchiveImpact, MissingArchivePolicy};
use super::inline::InlineSource;
use super::patcher;
use super::staging::StagingStore;

/// Default maximum number of concurrent downloads.
const DEFAULT_CONCURRENCY: usize = 4;
const APPLY_STATE_VERSION: u32 = 1;
const DEFAULT_ARCHIVE_MEMORY_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// What to do with downloaded source archives once they have been applied:
/// keep them, prune the applied ones, or decide automatically.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArchiveRetentionPolicy {
    #[default]
    Keep,
    PruneApplied,
    Auto,
}

impl ArchiveRetentionPolicy {
    /// Read the policy from the `MODDE_ARCHIVE_RETENTION` environment variable,
    /// defaulting to [`ArchiveRetentionPolicy::Keep`].
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("MODDE_ARCHIVE_RETENTION")
            .ok()
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("prune-applied" | "prune" | "delete") => Self::PruneApplied,
            Some("auto") => Self::Auto,
            _ => Self::Keep,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ArchiveBatchSentinel {
    pipeline_version: u32,
    archive_hash: u64,
    archive_size_bytes: u64,
    directive_indices: Vec<usize>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CreateBsaSentinel {
    pipeline_version: u32,
    directive_index: usize,
    temp_id: String,
    to: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct VerifiedArchiveSidecar {
    pipeline_version: u32,
    archive_hash: u64,
    size_bytes: u64,
    modified_unix_ms: u128,
    verified_unix_ms: u128,
}

#[derive(Debug, Clone)]
enum TrustedArchive {
    Path(PathBuf),
    Bytes {
        label: String,
        bytes: Bytes,
        fallback_path: PathBuf,
    },
}

#[derive(Debug, Default, Clone, Copy)]
struct ArchiveTrustStats {
    sidecar_hit: bool,
    streamed_hash_bytes: u64,
    memory_archive_hit: bool,
    disk_fallback: bool,
}

#[derive(Debug, Default, Clone, Copy)]
struct StagingAdoptionSummary {
    archive_batches: usize,
    create_bsa: usize,
}

struct DiagnosticsHeartbeatGuard(Option<JoinHandle<()>>);

impl Drop for DiagnosticsHeartbeatGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

/// Progress update sent during installation.
#[derive(Debug, Clone)]
pub enum InstallProgress {
    Starting {
        total_downloads: usize,
    },
    Downloading {
        name: String,
        bytes: u64,
        total: u64,
    },
    DownloadComplete {
        name: String,
    },
    Verifying {
        name: String,
    },
    Applying {
        directive_index: usize,
        total: usize,
    },
    Patching {
        name: String,
    },
    CreatingBSA {
        name: String,
    },
    LauncherConfigured {
        report: modde_games::launcher::LauncherConfigurationReport,
    },
    InlineFile {
        name: String,
    },
    StagingAdopted {
        archive_batches: usize,
        create_bsa: usize,
    },
    Complete,
    Failed {
        error: String,
    },
}

/// Orchestrate a full Wabbajack install pipeline.
pub struct WabbajackInstaller {
    manifest: WabbajackManifest,
    /// Maps each archive hash to its index into `manifest.archives` (keep-first
    /// semantics, matching the legacy `.iter().find()` lookups).
    archives_by_hash: std::collections::HashMap<u64, usize>,
    /// Path to the `.wabbajack` zip file (needed for `InlineFile` and `PatchedFromArchive` data).
    wabbajack_path: PathBuf,
    store_dir: PathBuf,
    staging_dir: PathBuf,
    game_dir: Option<PathBuf>,
    sources: Arc<Vec<AnySource>>,
    concurrency: usize,
    /// When true, per-archive download and per-directive apply failures are logged
    /// rather than fatal. Used for very large modlists where a handful of archives
    /// require manual intervention (deleted Nexus mods, rate-limited cloud hosts)
    /// but the rest of the modlist should still install.
    continue_on_error: bool,
    byte_cache: Arc<ByteLruCache>,
    diagnostics: Option<WabbajackDiagnostics>,
    archive_retention: ArchiveRetentionPolicy,
    missing_archive_policy: MissingArchivePolicy,
    archive_memory_max_bytes: u64,
    #[cfg(test)]
    apply_memory_provider: Option<memory_admission::provider::SharedMemoryProvider>,
}

impl WabbajackInstaller {
    /// Create an installer for `manifest`, reading archive data from
    /// `wabbajack_path` and using `store_dir` and `staging_dir` as the download
    /// store and working staging area.
    #[must_use]
    pub fn new(
        manifest: WabbajackManifest,
        wabbajack_path: PathBuf,
        store_dir: PathBuf,
        staging_dir: PathBuf,
    ) -> Self {
        let mut archives_by_hash = std::collections::HashMap::new();
        for (idx, archive) in manifest.archives.iter().enumerate() {
            archives_by_hash.entry(archive.hash).or_insert(idx);
        }
        Self {
            manifest,
            archives_by_hash,
            wabbajack_path,
            store_dir,
            staging_dir,
            game_dir: None,
            sources: Arc::new(Vec::new()),
            concurrency: DEFAULT_CONCURRENCY,
            continue_on_error: false,
            byte_cache: Arc::new(ByteLruCache::from_env()),
            diagnostics: None,
            archive_retention: ArchiveRetentionPolicy::from_env(),
            missing_archive_policy: MissingArchivePolicy::Fail,
            archive_memory_max_bytes: std::env::var("MODDE_ARCHIVE_MEMORY_MAX_BYTES")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(DEFAULT_ARCHIVE_MEMORY_MAX_BYTES),
            #[cfg(test)]
            apply_memory_provider: None,
        }
    }

    /// When set, log per-archive failures rather than aborting the install.
    pub fn set_continue_on_error(&mut self, value: bool) {
        self.continue_on_error = value;
    }

    /// Attach a diagnostics sink for heartbeat and progress reporting.
    pub fn set_diagnostics(&mut self, diagnostics: WabbajackDiagnostics) {
        self.diagnostics = Some(diagnostics);
    }

    /// Set the retention policy applied to downloaded archives after install.
    pub fn set_archive_retention(&mut self, policy: ArchiveRetentionPolicy) {
        self.archive_retention = policy;
    }

    /// Set how missing source archives are handled during install.
    pub fn set_missing_archive_policy(&mut self, policy: MissingArchivePolicy) {
        self.missing_archive_policy = policy;
    }

    /// Set the game install directory used by Wabbajack game-file sources.
    pub fn set_game_dir(&mut self, game_dir: PathBuf) {
        self.game_dir = Some(game_dir);
    }

    /// Register a download source implementation.
    pub fn add_source(&mut self, source: AnySource) {
        Arc::get_mut(&mut self.sources)
            .expect("add_source must be called before install")
            .push(source);
    }

    /// Set the maximum number of concurrent downloads.
    pub fn set_concurrency(&mut self, concurrency: usize) {
        self.concurrency = concurrency.max(1);
    }
}

mod admission;
mod archive_batch;
mod archive_batch_nested;
mod archive_io;
mod game_file;
mod install;
mod outputs;
mod preflight;
mod state;
mod trust;
mod weights;

pub(crate) use archive_io::archive_path;
use archive_io::{
    archive_patch_chunk_size, archive_path_is_memory_supported, extract_archive_path_requests,
    extract_nested_archive_requests, extract_trusted_archive_requests, metadata_modified_unix_ms,
    unix_ms, verified_sidecar_path, write_bytes_maybe_zstd,
};
#[cfg(test)]
use game_file::validate_zip_entry;
use game_file::{
    GameFileSourcePath, game_file_source_is_whole_file, is_game_file_archive, normalize_path,
    read_game_file_source, validate_archive_entry,
};
use weights::{
    ArchiveBatchWeights, DirectiveWeights, archive_batch_has_patch, estimate_archive_batch_weight,
    estimate_directive_weight, find_path_case_insensitive, trim_process_allocator,
};
#[cfg(test)]
use weights::{extract_from_zip, find_entry_in_archive};

#[cfg(test)]
mod tests;
