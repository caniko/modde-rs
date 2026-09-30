pub mod backup;
#[cfg(feature = "bethesda-archives")]
pub mod bethesda_archive;
pub mod bisect;
pub mod collision;
pub mod crash;
pub mod db;
pub mod diagnostics;
pub mod doctor;
pub mod error;
pub mod filter;
pub mod fs;
pub mod hash;
pub mod hot_deploy;
pub mod installer;
pub mod instance;
pub mod ipc;
pub mod library;
pub mod link;
pub mod lockfile;
pub mod manifest;
pub mod nexus_id;
pub mod patcher;
pub mod paths;
pub mod performance;
pub mod plugin;
pub mod profile;
pub mod resolver;
pub mod save;
pub mod scanner;
pub mod settings;
pub mod stock;
pub mod transaction;
pub mod update_check;
pub mod vfs;

pub use bisect::{
    BisectOracle, BisectResult, BisectSaveSafety, BisectSession, BisectStatus, BisectStep,
    CandidatePlan, apply_result as apply_bisect_result, next_candidate,
};
pub use collision::{CollisionClassifier, CollisionReport, CollisionSeverity, FileOrigin};
pub use db::{
    HiddenFile, ModCategory, ModdeDb, NewBisectSession, NewBisectStep, NewPerformanceRun,
    PatcherConfigRow, PerformanceRunRow, PluginEntry, ProfileSummary, SaveEntry, SnapshotMeta,
};
pub use diagnostics::{
    DangerousCollisionRule, DiagContext, DiagFix, Diagnostic, DiagnosticEngine, DiagnosticRule,
    Severity, ShadowedModRule,
};
pub use error::{CoreError, Result};
pub use hot_deploy::{HotDeployChange, HotDeployPatch};
pub use manifest::collection::CollectionManifest;
pub use manifest::wabbajack::{DownloadDirective, InstallDirective, WabbajackManifest};
pub use nexus_id::{NexusFileId, NexusIdError, NexusModId};
pub use patcher::{
    CommandSettings, PatcherStageKind, PatcherStageOutputRow, PatcherStageRow,
    PatcherStageSettings, SynthesisCliSettings,
};
pub use performance::{
    MangoHudParseResult, PerformanceModSnapshot, PerformanceSample, PerformanceSummary,
    mod_set_hash, mod_snapshot, parse_mangohud_csv_file, parse_mangohud_csv_file_with_warmup,
};
pub use profile::{
    ActivateResult, ActiveProfileInfo, EnabledMod, LoadOrderLock, LockReason, Profile,
    ProfileSource,
};
pub use resolver::{ConflictMap, GameId, ModId, ResolvedLoadOrder};
pub use save::{FingerprintCheck, SaveFingerprint, SaveSnapshot};
pub use transaction::{
    TransactionArtifact, TransactionError, TransactionPackage, TransactionPlan,
    TransactionProvenance, TransactionVersion,
};
