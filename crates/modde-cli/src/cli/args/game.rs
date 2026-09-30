//! Game, diagnostics, update, performance, and related CLI actions.

use std::path::PathBuf;

use clap::Subcommand;

use crate::commands;

#[derive(Subcommand)]
pub(crate) enum CrashAction {
    /// Parse and correlate a local crash log with a profile's installed mods.
    Analyze {
        log_path: Option<PathBuf>,
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_enum, default_value_t = commands::crash::CrashFormatArg::Auto)]
        format: commands::crash::CrashFormatArg,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum DoctorAction {
    /// Diagnose common profile problems from modde's local database.
    Profile {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Parse and correlate a local crash log with a profile's installed mods.
    Crash {
        log_path: Option<PathBuf>,
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_enum, default_value_t = commands::crash::CrashFormatArg::Auto)]
        format: commands::crash::CrashFormatArg,
        #[arg(long)]
        json: bool,
    },
    /// Ask an OpenAI-compatible LLM for grounded, cited hypotheses.
    Explain {
        log_path: PathBuf,
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, value_enum, default_value_t = commands::doctor::DoctorProviderArg::Local)]
        provider: commands::doctor::DoctorProviderArg,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum BisectAction {
    /// Start a resumable bisect session for a bad source profile
    Start {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: String,
        #[arg(long, value_enum)]
        oracle: commands::bisect::BisectOracleArg,
        #[arg(long)]
        baseline_run: Option<String>,
        #[arg(long, default_value_t = modde_core::bisect::default_perf_p99_frame_time_percent())]
        perf_p99_frame_time_percent: u16,
        #[arg(long, default_value_t = modde_core::bisect::default_perf_one_percent_low_fps_percent())]
        perf_one_percent_low_fps_percent: u16,
        #[arg(long, default_value_t = modde_core::bisect::default_perf_alpha_micros())]
        perf_alpha_micros: u32,
        #[arg(long, default_value_t = modde_core::bisect::default_perf_min_samples())]
        perf_min_samples: usize,
        #[arg(long)]
        crash_dir: Option<PathBuf>,
        #[arg(long)]
        force_save_risk: bool,
        #[arg(long)]
        keep_profiles: bool,
    },
    /// Create/deploy/launch the next candidate profile
    Run { session_id: String },
    /// Relaunch the current pending candidate profile
    Retry { session_id: String },
    /// Mark the pending candidate result for manual or fire-and-forget runs
    Mark {
        session_id: String,
        #[arg(value_enum)]
        result: commands::bisect::BisectResultArg,
        #[arg(long)]
        notes: Option<String>,
    },
    /// Show bisect session progress
    Status { session_id: String },
    /// Show detailed step history and observed signals
    History { session_id: String },
    /// Abort a bisect session and clean candidate profiles unless kept
    Abort { session_id: String },
}

#[derive(Subcommand)]
pub(crate) enum GameAction {
    /// Add or overwrite a user-defined game registration
    Add {
        id: String,
        #[arg(long)]
        display_name: String,
        #[arg(long)]
        executable_dir: PathBuf,
        #[arg(long)]
        steam_app_id: Option<String>,
        #[arg(long)]
        install_dir_name: Option<String>,
        #[arg(long)]
        mod_dir: Option<PathBuf>,
        #[arg(long)]
        nexus_domain: Option<String>,
        #[arg(long = "proxy-dll")]
        proxy_dlls: Vec<String>,
        /// Overwrite an existing user-defined game TOML
        #[arg(long)]
        force: bool,
    },
    /// List user-defined games
    List,
    /// Remove a user-defined game registration
    Remove {
        id: String,
        /// Skip the confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    /// Detect executable-bearing directories under a game install
    Detect { install_path: PathBuf },
    /// Show a resolved game registration
    Show { id: String },
    /// Export a game registration to TOML
    Export {
        id: String,
        #[arg(long)]
        with_optiscaler: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Import a game registration from TOML
    Import {
        path: PathBuf,
        #[arg(long)]
        force: bool,
    },
    #[allow(clippy::doc_markdown)]
    /// Import OptiScaler profiles from TOML for a game
    ImportProfile {
        path: PathBuf,
        #[arg(long = "for")]
        game: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
pub(crate) enum ModAction {
    /// Remove an installed mod from a profile and unlink its staged
    /// files. Uses the V8 `installed_mod_files` manifest so removal is
    /// precise — no orphaned files, no collateral damage.
    Remove {
        /// Mod id as stored in the profile (usually
        /// `<domain>_<mod_id>_<file_id>` for Nexus installs).
        mod_id: String,
        /// Profile to remove from. Defaults to the active / unambiguous
        /// one.
        #[arg(long)]
        profile: Option<String>,
        /// Report save-game contamination risk without removing the mod.
        #[arg(long)]
        dry_run: bool,
        /// Remove even when active or vaulted saves depend on this mod.
        #[arg(long)]
        force_contaminate: bool,
    },
    /// Print the skill dossier path and inline prompt for a mod whose
    /// install type could not be detected. Handy for piping into
    /// `claude` or pasting into a chat manually.
    Diagnose { mod_id: String },
}

#[derive(Subcommand)]
pub(crate) enum StockAction {
    /// Create a vanilla game snapshot
    Snapshot { game_id: String },
    /// Verify snapshot integrity
    Verify { game_id: String },
}

#[derive(Subcommand)]
pub(crate) enum UpdateAction {
    /// Check for updates on Nexus Mods
    Check {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Time period to check: "1d", "1w", or "1m"
        #[arg(long, default_value = "1w")]
        period: String,
        /// Check Nexus-tracked profile mods instead of modde itself.
        #[arg(long)]
        mods: bool,
    },
    /// Download and install the latest MAIN file for any tracked mod
    /// in the profile that has a newer version on Nexus.
    ///
    /// Refuses to run on locked profiles (Wabbajack / Collection /
    /// TOML import) unless `--confirm-locked` is passed — the lock
    /// exists precisely to prevent the load order drifting away from
    /// its authoritative source. Mods whose semver major bumps are
    /// flagged "breaking" and require `--accept-breaking` plus a
    /// secondary y/N confirmation per mod.
    Apply {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Time period to scan: "1d", "1w", or "1m"
        #[arg(long, default_value = "1w")]
        period: String,
        /// Print the mods that would be updated without downloading.
        #[arg(long)]
        dry_run: bool,
        /// Acknowledge that the profile is locked (Wabbajack / Collection
        /// / TOML import) and that updating drifts it away from the
        /// authoritative source. Required for any locked profile.
        #[arg(long)]
        confirm_locked: bool,
        /// Permit applying updates that look like breaking semver bumps
        /// (major version change). Each breaking mod still requires an
        /// interactive y/N confirmation.
        #[arg(long)]
        accept_breaking: bool,
        /// Skip interactive prompts (assume "yes" to per-mod breaking
        /// confirmations). Refuses any breaking update unless
        /// `--accept-breaking` is also set.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
#[cfg(feature = "bethesda")]
pub(crate) enum LootAction {
    /// Sort plugins using LOOT masterlist rules
    Sort {
        #[arg(long)]
        game: String,
        /// Path to game Data directory (auto-detected if omitted)
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Validate plugins for Form 43 and missing master errors
    Validate {
        #[arg(long)]
        game: String,
    },
}

#[derive(Subcommand)]
pub(crate) enum PerfAction {
    /// Repeated paired sandbox-off/on runs for an exact direct installation
    Sandbox {
        id: String,
        #[arg(long)]
        profile: String,
        #[arg(long, default_value_t = 3)]
        pairs: usize,
        #[arg(long, default_value_t = 300)]
        duration: u64,
        #[arg(long, default_value_t = 30.0)]
        warmup_seconds: f64,
    },
    /// Launch a profile with per-run `MangoHud` CSV capture
    Run {
        /// Profile to activate and benchmark (uses active profile if omitted)
        profile: Option<String>,
        #[arg(long)]
        game: String,
        /// `MangoHud` log duration in seconds
        #[arg(long, default_value_t = 300)]
        duration: u64,
        /// Optional human label for the run
        #[arg(long)]
        label: Option<String>,
        /// Seconds of startup samples to skip when computing summary statistics
        #[arg(long, default_value_t = 30.0)]
        warmup_seconds: f64,
        /// Skip mod deployment before launch
        #[arg(long)]
        no_deploy: bool,
    },
    /// Ingest a `MangoHud` CSV for a pending run
    Ingest {
        #[arg(long = "run")]
        run_id: String,
        #[arg(long)]
        csv: PathBuf,
        /// Seconds of startup samples to skip when computing summary statistics
        #[arg(long, default_value_t = 30.0)]
        warmup_seconds: f64,
    },
    /// List captured runs for a game
    List {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show one captured run
    Show { run_id: String },
    /// Compare two captured runs
    Compare {
        #[arg(long)]
        baseline: String,
        #[arg(long)]
        candidate: String,
    },
}
