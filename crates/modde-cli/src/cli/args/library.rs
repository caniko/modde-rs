use std::path::PathBuf;

use clap::Subcommand;

#[derive(Subcommand)]
pub(crate) enum LibraryAction {
    /// List installed and cached owned games, including titles without mod plugins
    List {
        #[arg(long)]
        json: bool,
    },
    /// Refresh Steam ownership (credential read from MODDE_STEAM_API_KEY)
    SyncSteam { steam_id: String },
    /// Launch an exact installation using saved settings
    Play { id: String },
    /// Confirm a store-launched game has exited and capture its saves
    Finish {
        /// Confirm exit when process evidence was lost (or for a legacy handoff)
        #[arg(long)]
        confirm_exited: bool,
        /// Leave performance/bisect results for manual handling after capturing saves
        #[arg(long)]
        skip_analysis: bool,
    },
    /// Show the persistent session and process evidence
    Status,
    /// Generate a Steam %command% / Heroic wrapper for this installation
    Hook { id: String },
    /// Enter the shared lifecycle at a store's actual game-command boundary
    Wrap {
        id: String,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Isolated process supervisor (invoked by modde)
    #[command(hide = true)]
    Supervise { request: PathBuf },
    /// Complete one observed session if its original CLI was interrupted
    #[command(hide = true)]
    CompleteObserved { observation: PathBuf },
    /// Command boundary used after modde-manager validates a native game launch
    #[command(hide = true)]
    ManagerWrap {
        id: String,
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        prefix: PathBuf,
        /// Names of the validated manager environment variables to preserve
        #[arg(long)]
        inherit_env: Vec<String>,
        #[arg(last = true, required = true)]
        command: Vec<std::ffi::OsString>,
    },
    /// Recover interrupted preparation before a game process was started
    Recover,
    /// Hand off installation to Steam or Heroic
    Install { id: String },
    /// Read or replace launch settings with a JSON file (see the playing guide)
    Configure {
        id: String,
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Save or remove a favorite (installation IDs resolve to their entitlement)
    Favorite {
        id: String,
        #[arg(long)]
        remove: bool,
    },
    /// Adopt existing saves for an installation without changing live files
    Adopt {
        id: String,
        #[arg(long)]
        profile: String,
    },
}
