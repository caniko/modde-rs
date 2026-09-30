#![allow(clippy::wildcard_imports)]
//! Command mutation classification and lazy update notices.

use super::args::*;

pub(super) fn command_mutates_state(cmd: &Commands) -> bool {
    match cmd {
        Commands::Library { action } => !matches!(
            action,
            LibraryAction::List { .. }
                | LibraryAction::Status
                | LibraryAction::Supervise { .. }
                | LibraryAction::Configure { file: None, .. }
        ),
        // Pure read paths.
        Commands::Dev { .. }
        | Commands::Detect
        | Commands::Diagnostics { .. }
        | Commands::Doctor {
            action: DoctorAction::Profile { .. },
        }
        | Commands::Export { .. }
        | Commands::Lock {
            action:
                LockAction::Export { .. }
                | LockAction::Verify { .. }
                | LockAction::Sign { .. }
                | LockAction::Keygen { .. }
                | LockAction::Import { dry_run: true, .. },
        }
        | Commands::Verify { .. }
        | Commands::Collisions { .. } => false,
        #[cfg(feature = "gui")]
        Commands::Gui => false,

        Commands::Config { action } => matches!(
            action,
            ConfigAction::SetDatabase { .. } | ConfigAction::ResetDatabase
        ),

        Commands::Game { action } => {
            matches!(
                action,
                GameAction::Add { .. }
                    | GameAction::Remove { .. }
                    | GameAction::Import { .. }
                    | GameAction::ImportProfile { .. }
            )
        }

        // `update check` is read-only; `update apply` mutates.
        Commands::Update { action } => matches!(action, UpdateAction::Apply { .. }),

        Commands::Perf { action } => {
            matches!(
                action,
                PerfAction::Run { .. } | PerfAction::Ingest { .. } | PerfAction::Sandbox { .. }
            )
        }

        // `instance list` is read-only; create/switch flip the active
        // data dir.
        Commands::Instance { action } => !matches!(action, InstanceAction::List),

        // Loot validate just reports, sort rewrites the load order.
        #[cfg(feature = "bethesda")]
        Commands::Loot { action } => matches!(action, LootAction::Sort { .. }),

        Commands::Patcher { action } => !matches!(
            action,
            PatcherAction::List { .. } | PatcherAction::Validate { .. }
        ),

        // Tool subcommands: queries are read-only, everything else
        // mutates per-game tool config rows.
        Commands::Tool { action } => !matches!(
            action,
            ToolAction::List { .. }
                | ToolAction::Status { .. }
                | ToolAction::Show { .. }
                | ToolAction::Diagnose { .. }
                | ToolAction::Doctor { .. }
                | ToolAction::Settings { .. }
                | ToolAction::Profiles { .. }
                | ToolAction::Sources { .. }
                | ToolAction::Preview { .. }
                | ToolAction::Releases { .. }
                | ToolAction::ListExecutables { .. }
        ),

        // Exec is the alias for tool's executable subset. List is a
        // read-only query; the others mutate or run a child process
        // whose overwrite-capture writes to the store.
        Commands::Exec { action } => !matches!(action, ExecAction::List { .. }),

        // Skill list/path are read-only; install writes files under the
        // user-global `.agents/skills` tree.
        Commands::Skill { action } => matches!(action, SkillAction::Install { .. }),

        // Nexus: `auth` writes the API key, `status` prints validity.
        // The status arm doesn't mutate, but the GUI surfaces auth
        // state — a refresh is harmless and cheap. Notify on either.
        Commands::Nexus { .. } => true,
        Commands::Crash { .. } => true,
        Commands::Doctor {
            action: DoctorAction::Crash { .. } | DoctorAction::Explain { .. },
        } => true,

        // `mod diagnose` only prints the dossier; remove mutates.
        Commands::Mod { action } => matches!(action, ModAction::Remove { .. }),

        // `wabbajack search` / `download` / `hm-snippet` / `assess` /
        // `missing-impact` / `manual-links` are read-only;
        // import/acquire commands write into the store.
        Commands::Wabbajack { action } => matches!(
            action,
            WabbajackAction::ImportArchive { .. } | WabbajackAction::AcquireMissing { .. }
        ),

        // Save commands cover read-only listing AND mutating
        // capture/restore/adopt; conservatively notify on all of
        // them — the cost is one socket round-trip per active GUI.
        Commands::Save { .. } => true,

        // `nxm install` registers the URI handler (system-side). We
        // still notify so a GUI showing nxm settings can reflect it.
        Commands::Nxm { .. } => true,

        // Stock snapshots affect deploy decisions; treat as mutating.
        Commands::Stock { .. } => true,

        Commands::HotDeploy { dry_run, .. } => !dry_run,

        // Backup capture/restore mutates the data dir.
        Commands::Backup { .. } => true,

        // Everything below is unambiguously mutating.
        Commands::Profile { .. }
        | Commands::Lock { .. }
        | Commands::Bisect { .. }
        | Commands::Scan { .. }
        | Commands::Import
        | Commands::Fomod { .. }
        | Commands::Install { .. }
        | Commands::Deploy { .. }
        | Commands::Rollback { .. }
        | Commands::Play { .. } => true,
    }
}

pub(super) fn command_runs_lazy_product_update_check(cmd: &Commands) -> bool {
    #[cfg(feature = "gui")]
    if matches!(cmd, Commands::Gui) {
        return false;
    }
    !matches!(
        cmd,
        Commands::Config { .. }
            | Commands::Library { .. }
            | Commands::Dev { .. }
            | Commands::Lock { .. }
            | Commands::Update {
                action: UpdateAction::Check { .. }
            }
    )
}

pub(super) fn maybe_print_product_update_notice() {
    let settings = modde_core::settings::AppSettings::load();
    if !modde_core::update_check::update_checks_enabled(&settings) {
        return;
    }

    let Ok(runtime) = tokio::runtime::Runtime::new() else {
        return;
    };
    match runtime.block_on(modde_core::update_check::check_latest_with_settings(
        &settings,
    )) {
        Ok(Some(update)) => {
            eprintln!(
                "modde update available: {} (current: {}) - {}",
                update.latest_version, update.current_version, update.release_url
            );
        }
        Ok(None) => {}
        Err(error) => tracing::debug!(%error, "product update check failed"),
    }
}

#[cfg(test)]
mod tests;
