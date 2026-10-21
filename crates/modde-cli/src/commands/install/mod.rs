use std::path::Path;

use anyhow::{Context, Result, bail};
use tracing::{info, warn};

use modde_core::installer::{self as installer, InstallStatus};
use modde_core::manifest::collection::CollectionManifest;
use modde_core::paths;
use modde_core::profile::{
    EnabledMod, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::{NexusFileId, NexusModId};
use modde_sources::nexus::api::NexusApi;
use modde_sources::nexus::auth::load_api_key;
use modde_sources::nexus::cdn::generate_download_link;

use crate::InstallSource;
use crate::commands::wabbajack::{acquire_missing, acquire_status_label};

/// Build a shared HTTP client with sensible timeouts for mod downloads.
mod archives;
mod collection;
mod launcher;
mod single_mod;
mod wabbajack;

#[cfg(test)]
mod tests;

pub use archives::find_fomod_config;
use archives::{
    build_http_client, download_file, extract_archive, fetch_collection, parse_nexus_url,
};
use collection::handle_nexus_collection;
pub use launcher::{configure_wine_overrides, deploy_mo2_to_game, print_tool_environment_report};
use single_mod::handle_single_mod;
use wabbajack::handle_wabbajack;

async fn save_profile_and_settings(
    pm: &ProfileManager,
    profile: &Profile,
    game_dir: Option<&Path>,
) -> Result<i64> {
    let profile_id = pm.create_or_update(profile).await?;
    let mut settings = modde_core::settings::AppSettings::load();
    if let Some(gd) = game_dir {
        settings.set_game_path(&profile.game_id, gd.to_path_buf());
    }
    settings.selected_game = Some(profile.game_id.to_string());
    settings.save();
    Ok(profile_id)
}

/// One-line rendering of a lock reason for CLI warnings. Intentionally
/// shorter than the `format_lock_reason` in `commands/profile.rs` (which
/// is used by `lock-info` output). Kept local so install.rs doesn't
/// depend on profile.rs internals.
fn format_lock_reason_short(reason: &LockReason) -> &'static str {
    match reason {
        LockReason::Wabbajack { .. } => "Wabbajack",
        LockReason::NexusCollection { .. } => "Nexus Collection",
        LockReason::TomlImport { .. } => "TOML import",
        LockReason::Manual { .. } => "manual",
    }
}
pub async fn handle(source: InstallSource) -> Result<()> {
    match source {
        InstallSource::NexusCollection {
            slug,
            version,
            profile,
        } => {
            handle_nexus_collection(slug, version, profile).await?;
        }
        InstallSource::Wabbajack {
            path,
            profile,
            game_dir,
            force,
            no_deploy,
            continue_on_error,
            reset_staging,
            skip_validate,
            diagnostics_dir,
            diagnostics_interval,
            stall_warn_seconds,
            stall_abort_seconds,
            archive_retention,
            missing_archive_policy,
            acquire_missing: _acquire_missing,
            acquire_download_dir,
            acquire_timeout,
            acquire_include_nexus,
            acquire_browser_controller,
            no_acquire_missing,
        } => {
            if !no_acquire_missing {
                let results = acquire_missing(
                    path.clone(),
                    acquire_download_dir,
                    None,
                    None,
                    acquire_include_nexus,
                    acquire_browser_controller,
                    acquire_timeout,
                    false,
                )
                .await?;
                let failed: Vec<_> = results
                    .iter()
                    .filter(|result| {
                        !matches!(
                            result.status,
                            modde_sources::wabbajack::acquire::AcquireStatus::Imported
                                | modde_sources::wabbajack::acquire::AcquireStatus::AlreadyPresent
                                | modde_sources::wabbajack::acquire::AcquireStatus::DirectResolved
                        )
                    })
                    .collect();
                if !failed.is_empty()
                    && matches!(
                        missing_archive_policy,
                        crate::WabbajackMissingArchivePolicyArg::Fail
                    )
                {
                    let details = failed
                        .iter()
                        .map(|result| {
                            format!(
                                "{} {:016x} {}",
                                acquire_status_label(&result.status),
                                result.archive.hash,
                                result.archive.name
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    anyhow::bail!(
                        "frontloaded Wabbajack archive acquisition did not complete:\n{details}"
                    );
                }
            }
            handle_wabbajack(modde_sources::wabbajack::runner::WabbajackInstallOptions {
                db: None,
                path,
                profile_name: profile,
                game_dir,
                force,
                no_deploy,
                safety: modde_sources::wabbajack::runner::WabbajackInstallSafety {
                    continue_on_error,
                    reset_staging,
                    skip_validate,
                },
                diagnostics: diagnostics_dir.map(|dir| {
                    modde_sources::wabbajack::diagnostics::WabbajackDiagnosticsOptions {
                        dir,
                        interval: std::time::Duration::from_secs(diagnostics_interval.max(1)),
                        stall_warn: std::time::Duration::from_secs(stall_warn_seconds.max(1)),
                        stall_abort: std::time::Duration::from_secs(stall_abort_seconds.max(1)),
                    }
                }),
                archive_retention: archive_retention.into(),
                missing_archive_policy: missing_archive_policy.into(),
            })
            .await?;
        }
        InstallSource::Mod { url, profile, .. } => {
            handle_single_mod(url, profile).await?;
        }
    }
    Ok(())
}
