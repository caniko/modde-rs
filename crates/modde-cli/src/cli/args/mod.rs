//! CLI argument definitions and subcommand action enums.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

mod game;
mod install;
mod profile;
mod system;
mod tools;

pub(crate) use game::*;
pub(crate) use install::*;
pub(crate) use profile::*;
pub(crate) use system::*;
pub(crate) use tools::*;

#[derive(Parser)]
#[command(name = "modde", version, about = "NixOS-native game mod manager")]
pub(crate) struct Cli {
    /// Override data directory (default: ~/.local/share/modde or $`MODDE_DATA_DIR`)
    #[arg(long, global = true, env = "MODDE_DATA_DIR")]
    pub(crate) data_dir: Option<PathBuf>,

    /// Write a DHAT heap profile. Requires the `heap-profile` cargo feature.
    #[arg(long, global = true, env = "MODDE_HEAP_PROFILE")]
    pub(crate) heap_profile: Option<PathBuf>,

    /// Panic after startup to smoke test remote telemetry crash capture.
    #[cfg(feature = "remote-telemetry")]
    #[arg(long, global = true, hide = true)]
    pub(crate) debug_panic: bool,

    #[command(subcommand)]
    pub(crate) command: Commands,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    #[command(hide = true)]
    Dev {
        #[command(subcommand)]
        action: DevAction,
    },
    /// Manage profiles
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// Export, sign, verify, and import portable modde.lock files
    Lock {
        #[command(subcommand)]
        action: LockAction,
    },
    /// Inspect and set modde configuration (including the database backend)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Switch profile, deploy mods, and launch the game
    Play {
        /// Profile to activate (uses active profile if omitted)
        profile: Option<String>,
        #[arg(long)]
        game: String,
        /// Skip mod deployment (just switch + launch)
        #[arg(long)]
        no_deploy: bool,
        /// Skip profile switch (just deploy + launch active)
        #[arg(long)]
        no_switch: bool,
        /// Skip save auto-capture after game exit
        #[arg(long)]
        no_capture: bool,
    },
    /// Capture and compare local performance telemetry
    Perf {
        #[command(subcommand)]
        action: PerfAction,
    },
    /// Deploy mods for the active or specified profile
    Deploy {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Patch one cosmetic mod into the live VFS without a full redeploy
    HotDeploy {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        #[arg(long = "mod")]
        mod_id: String,
        #[arg(long)]
        enable: bool,
        #[arg(long)]
        disable: bool,
        #[arg(long)]
        dry_run: bool,
        /// Apply even if the game process appears to be running.
        #[arg(long)]
        force: bool,
    },
    /// Rollback to the previous deployment
    Rollback {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Install mods from various sources
    Install {
        #[command(subcommand)]
        source: InstallSource,
    },
    /// Manage individual mods (remove, diagnose unknown-type dossiers)
    Mod {
        #[command(subcommand)]
        action: ModAction,
    },
    /// Verify installed file integrity
    Verify {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
    },
    /// Nexus Mods account management
    Nexus {
        #[command(subcommand)]
        action: NexusAction,
    },
    /// Manage vanilla game snapshots
    Stock {
        #[command(subcommand)]
        action: StockAction,
    },
    /// FOMOD installer utilities
    Fomod {
        #[command(subcommand)]
        action: FomodAction,
    },
    /// Manage save file assignments
    Save {
        #[command(subcommand)]
        action: SaveAction,
    },
    /// Check for mod updates from Nexus
    Update {
        #[command(subcommand)]
        action: UpdateAction,
    },
    /// LOOT masterlist integration (Bethesda plugin sorting)
    #[cfg(feature = "bethesda")]
    Loot {
        #[command(subcommand)]
        action: LootAction,
    },
    /// Configure and run profile-scoped patcher pipeline stages
    Patcher {
        #[command(subcommand)]
        action: PatcherAction,
    },
    /// Run external tools with overwrite capture
    Tool {
        #[command(subcommand)]
        action: ToolAction,
    },
    /// Handle nxm:// download links from Nexus Mods
    Nxm {
        #[command(subcommand)]
        action: NxmAction,
    },
    /// Browse, download, and generate Home Manager snippets for Wabbajack modlists
    Wabbajack {
        #[command(subcommand)]
        action: WabbajackAction,
    },
    /// Scan game directory for installed mods
    Scan {
        #[arg(long)]
        game: String,
        /// Path to game installation (auto-detected if omitted)
        #[arg(long)]
        game_dir: Option<PathBuf>,
        /// Path to .wabbajack file for manifest matching
        #[arg(long)]
        manifest: Option<PathBuf>,
        /// Import discovered mods into this profile
        #[arg(long)]
        import_to: Option<String>,
        /// Minimum file presence fraction (0.0-1.0)
        #[arg(long, default_value = "0.5")]
        threshold: f32,
        /// Report only, don't write to database
        #[arg(long)]
        dry_run: bool,
        /// Before merging, remove any pre-existing filesystem-scanner
        /// rows (cet/*, reds/*, tweak/*, archive/*, redmod/*) from the
        /// target profile that the manifest already covers. Requires
        /// `--manifest` and `--import-to`. See `modde profile dedup`
        /// for the standalone equivalent.
        #[arg(long)]
        prune_duplicates: bool,
    },
    /// Analyse mod collisions and suggest optimisations
    Collisions {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Show all collisions including cosmetic ones
        #[arg(long)]
        all: bool,
        /// Suggest hide commands for redundant files
        #[arg(long)]
        suggest_hides: bool,
    },
    /// Diagnose profiles and crash logs with grounded evidence
    Doctor {
        #[command(subcommand)]
        action: DoctorAction,
    },
    /// Deprecated alias: use `modde doctor profile`
    #[command(hide = true)]
    Diagnostics {
        #[arg(long)]
        game: String,
        #[arg(long)]
        profile: Option<String>,
    },
    /// Deprecated alias: use `modde doctor crash`
    #[command(hide = true)]
    Crash {
        #[command(subcommand)]
        action: CrashAction,
    },
    /// Binary-search enabled mods to isolate a crash or performance regression
    Bisect {
        #[command(subcommand)]
        action: BisectAction,
    },
    /// Export mod list to CSV
    Export {
        #[arg(long)]
        profile: Option<String>,
        #[arg(long)]
        game: Option<String>,
        /// Comma-separated columns
        #[arg(long)]
        columns: Option<String>,
        /// Output file (stdout if omitted)
        #[arg(long)]
        output: Option<String>,
    },
    /// Manage mod and plugin order backups
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },
    /// Detect installed games across Steam and Heroic launchers
    Detect,
    /// Manage user-defined games
    Game {
        #[command(subcommand)]
        action: GameAction,
    },
    /// Manage modde instances (multiple data directories)
    Instance {
        #[command(subcommand)]
        action: InstanceAction,
    },
    /// Manage named launch targets (`xEdit`, `BodySlide`, `FNIS`, etc.)
    ///
    /// Thin alias for the executable subset of `modde tool`. The
    /// underlying storage is shared, so `modde exec list` and
    /// `modde tool list-executables` print the same rows.
    Exec {
        #[command(subcommand)]
        action: ExecAction,
    },
    /// Install and update modde-maintained Codex skills
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Import existing TOML profiles into the database
    Import,
    /// Launch the graphical user interface
    #[cfg(feature = "gui")]
    Gui,
}
