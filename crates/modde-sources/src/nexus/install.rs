//! Reusable single-mod install pipeline.
//!
//! Shared between `modde`'s `install mod` command and the
//! `modde-ui` **Browse Nexus → Install** button. The caller is
//! responsible for:
//!
//! * Loading / creating the target profile and wiring the returned
//!   [`InstallOutcome`] back into it.
//! * Supplying an [`InstallProbe`] — this crate intentionally does not
//!   depend on `modde-games`, so the probe has to come from the edge
//!   (CLI / UI) where `modde_games::resolve_game_plugin` is available.
//!
//! This module does the download + extraction + analysis + execution,
//! and writes the skill dossier when detection fails.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use reqwest::Client;

use modde_core::installer::{
    self, DossierContext, InstallMethod, InstallPlan, InstallProbe, InstallStatus, ProbeTrace,
};
use modde_core::paths;
use modde_core::{NexusFileId, NexusModId};

use super::api::NexusMod;
use super::cdn::generate_download_link;

/// Result of the install pipeline shared by the CLI, `modde update` and the UI.
///
/// The caller decides how to persist each variant:
/// * `Installed` → `ModdeDb::record_install(..., InstallStatus::Installed)`
/// * `PendingUserInput` → row with `install_status = 'pending_user_input'`,
///    route the UI to a wizard
/// * `Unknown` → row with `install_status = 'unknown'`, surface the dossier
/// * `AlreadyStaged` → store dir already exists, skip download
pub enum InstallOutcome {
    Installed(InstallPlan),
    PendingUserInput {
        method: &'static str,
    },
    Unknown {
        dossier_path: PathBuf,
        method: InstallMethod,
    },
    AlreadyStaged,
}

impl InstallOutcome {
    /// The profile-row install status this outcome records.
    #[must_use]
    pub fn status(&self) -> InstallStatus {
        match self {
            Self::Installed(_) | Self::AlreadyStaged => InstallStatus::Installed,
            Self::PendingUserInput { .. } => InstallStatus::PendingUserInput,
            Self::Unknown { .. } => InstallStatus::Unknown,
        }
    }
}

/// Run the single-mod install pipeline end-to-end.
///
/// `client` and `api_key` are used for the CDN download only —
/// callers that want richer metadata for the dossier can pass in a
/// `mod_info` fetched separately. `probe` should come from
/// `modde_games::game_probe(plugin)` for the game this mod belongs to,
/// or [`InstallProbe::noop`] if the game isn't registered.
pub async fn install_single_mod(
    client: &Client,
    api_key: &str,
    game_domain: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
    mod_info: &NexusMod,
    probe: &InstallProbe,
) -> Result<InstallOutcome> {
    // Update and UI callers pass domains from stored profile data.
    modde_core::nexus_id::validate_game_domain(game_domain)?;
    let store = paths::store_dir();
    let mod_store_dir = store.join(format!("{game_domain}_{mod_id}_{file_id}"));
    if mod_store_dir.exists() {
        return Ok(InstallOutcome::AlreadyStaged);
    }

    // Temp staging dir — analyze runs on the raw tree before we move
    // files into the canonical store layout. See
    // `modde_cli::commands::install::handle_single_mod` for the mirror
    // of this flow (kept separate so the CLI can print progress while
    // the UI can drive it from a `Task::perform`).
    let staging_root =
        paths::staging_dir().join(format!("install_{game_domain}_{mod_id}_{file_id}"));

    std::fs::create_dir_all(store.as_path())?;
    std::fs::create_dir_all(paths::staging_dir().as_path())?;

    // 1. Download via the CDN endpoint. This is gated on Nexus Premium;
    //    the caller should have verified that before getting here.
    let download_url = generate_download_link(client, api_key, game_domain, mod_id, file_id)
        .await
        .context("failed to get Nexus download link")?;
    let archive_path = store.join(format!("{mod_id}_{file_id}.zip"));
    download_with_reqwest(client, &download_url, &archive_path)
        .await
        .context("failed to download mod archive")?;

    // 2. Extract into the staging dir (NOT the store).
    if staging_root.exists() {
        let _ = std::fs::remove_dir_all(&staging_root);
    }
    std::fs::create_dir_all(&staging_root)?;
    installer::extract_archive(&archive_path, &staging_root)
        .context("failed to extract mod archive")?;

    // 3. Hash the archive for the plan, then remove it.
    let source_hash =
        installer::xxh64_file_hex(&archive_path).context("failed to hash downloaded archive")?;
    let _ = std::fs::remove_file(&archive_path);

    // 4. Analyze.
    let mut plan = installer::analyze(&staging_root, probe, source_hash)
        .context("installer analyze failed")?;

    let outcome = match &plan.method {
        InstallMethod::Unknown { .. } => {
            let dossier =
                write_dossier(&staging_root, game_domain, mod_id, file_id, mod_info, &plan)?;
            InstallOutcome::Unknown {
                dossier_path: dossier,
                method: plan.method.clone(),
            }
        }
        _ if !plan.method.is_ready() => {
            // FOMOD / BAIN with no config yet — copy the extracted tree
            // into the store so a wizard can run later without
            // re-downloading. The CLI and UI paths agree here: the store
            // is the wizard's working copy.
            std::fs::create_dir_all(&mod_store_dir)?;
            copy_dir_tree(&staging_root, &mod_store_dir)
                .context("failed to copy staging to store for pending install")?;
            InstallOutcome::PendingUserInput {
                method: plan.method.label(),
            }
        }
        _ => {
            // `is_ready()` mirrors `execute`'s preconditions, so unknown and
            // unconfigured methods never get here; any error is a real failure.
            std::fs::create_dir_all(&mod_store_dir)?;
            installer::execute(&mut plan, &staging_root, &mod_store_dir)?;
            InstallOutcome::Installed(plan)
        }
    };

    // Clean up staging now that we've either moved files into the
    // store or written a dossier.
    let _ = std::fs::remove_dir_all(&staging_root);
    Ok(outcome)
}

async fn download_with_reqwest(client: &Client, url: &str, dest: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt as _;

    let resp = crate::error::status_error(
        client
            .get(url)
            .send()
            .await
            .context("download GET failed")?,
    )
    .context("download HTTP error")?;
    let bytes = resp.bytes().await.context("failed to read download body")?;
    // The caller has already created the store directory `dest` lives in.
    let mut file = tokio::fs::File::create(dest).await?;
    file.write_all(&bytes).await?;
    file.flush().await?;
    Ok(())
}

fn write_dossier(
    extracted_dir: &Path,
    game_domain: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
    mod_info: &NexusMod,
    plan: &InstallPlan,
) -> Result<PathBuf> {
    let method = &plan.method;
    let ctx = DossierContext {
        game_id: game_domain.to_string(),
        game_domain: Some(game_domain.to_string()),
        nexus_mod_id: Some(mod_id),
        nexus_file_id: Some(file_id),
        mod_name: mod_info.name.clone(),
        mod_author: Some(mod_info.author.clone()),
        mod_version: Some(mod_info.version.clone()),
        mod_summary: mod_info.summary.clone(),
        nexus_url: Some(format!(
            "https://www.nexusmods.com/{game_domain}/mods/{mod_id}"
        )),
        source_archive_hash: plan.source_archive_hash.clone(),
    };
    let trace = vec![ProbeTrace {
        probe: "generic+game".to_string(),
        matched: false,
        note: format!("verdict: {}", method.label()),
    }];
    installer::dump_dossier(extracted_dir, &ctx, method, trace)
        .context("failed to write unknown-installer dossier")
}

/// Copy an extracted tree, which `extract_archive` fills with only regular
/// files and directories.
fn copy_dir_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_tree(&entry.path(), &dst_path)?;
        } else {
            std::fs::copy(entry.path(), &dst_path)?;
        }
    }
    Ok(())
}
