//! High-level entry points that drive a complete Wabbajack install: build the
//! HTTP client, wire up sources, run the installer, register a profile, and
//! configure Wine DLL overrides.

use std::io::Read as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{info, warn};

use modde_core::manifest::wabbajack::{
    WabbajackManifest, cache_wabbajack_file, compute_manifest_hash,
};
use modde_core::paths;
use modde_core::profile::{
    EnabledMod, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::resolver::GameId;

use crate::direct::DirectSource;
use crate::nexus::NexusSource;
use crate::wabbajack::cdn::WabbajackCdnSource;
use crate::wabbajack::diagnostics::{WabbajackDiagnostics, WabbajackDiagnosticsOptions};
use crate::wabbajack::impact::{MissingArchiveImpact, MissingArchivePolicy};
use crate::wabbajack::installer::{ArchiveRetentionPolicy, InstallProgress, WabbajackInstaller};
use crate::wabbajack::staging::{StagingStore, is_compressed_path, logical_path_from_compressed};

/// User-supplied options controlling a single Wabbajack install run.
#[derive(Debug, Clone)]
pub struct WabbajackInstallOptions {
    pub db: Option<modde_core::db::ModdeDb>,
    pub path: PathBuf,
    pub profile_name: Option<String>,
    pub game_dir: Option<PathBuf>,
    pub force: bool,
    /// When true, stage the modlist but skip the final copy/deploy into
    /// `game_dir`. The directory is still used to read `GameFileSource` archives
    /// and to register a profile.
    pub no_deploy: bool,
    pub safety: WabbajackInstallSafety,
    pub diagnostics: Option<WabbajackDiagnosticsOptions>,
    pub archive_retention: ArchiveRetentionPolicy,
    pub missing_archive_policy: MissingArchivePolicy,
}

/// Safety toggles that relax or skip parts of the install pipeline.
#[derive(Debug, Clone, Copy, Default)]
pub struct WabbajackInstallSafety {
    /// When true, log per-archive download / per-directive apply failures
    /// instead of aborting. Useful for very large modlists with a handful of
    /// archives that need manual intervention.
    pub continue_on_error: bool,
    /// Explicitly discard existing staging instead of adopting it.
    pub reset_staging: bool,
    /// Skip post-apply validation before deploy.
    pub skip_validate: bool,
}

/// Result summary returned after a successful install.
#[derive(Debug, Clone)]
pub struct WabbajackInstallSummary {
    pub profile_name: String,
    pub modlist_name: String,
    pub game_id: GameId,
    pub mod_count: usize,
    pub manifest_hash: String,
}

/// Build the shared HTTP client used for Wabbajack downloads, with sensible
/// connect and request timeouts.
///
/// # Errors
///
/// Returns an error if the underlying `reqwest` client cannot be built.
pub fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}

/// Parse the JSON manifest out of the `.wabbajack` archive at `path`.
///
/// # Errors
///
/// Returns an error if the file cannot be opened, read as a zip, or parsed.
pub fn parse_wabbajack_manifest(path: &Path) -> Result<WabbajackManifest> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("failed to open wabbajack file: {}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read wabbajack archive: {}", path.display()))?;

    let entry_name = if archive.by_name("modlist").is_ok() {
        "modlist"
    } else {
        "modlist.json"
    };
    let mut entry = archive
        .by_name(entry_name)
        .context("wabbajack archive missing modlist entry")?;
    let mut json = String::new();
    entry
        .read_to_string(&mut json)
        .context("failed to read modlist entry")?;
    serde_json::from_str(&json).context("failed to parse modlist JSON")
}

pub async fn install_wabbajack(
    options: WabbajackInstallOptions,
    progress_tx: Option<mpsc::UnboundedSender<InstallProgress>>,
) -> Result<WabbajackInstallSummary> {
    info!(path = %options.path.display(), ?options.profile_name, "installing Wabbajack modlist");

    let manifest = parse_wabbajack_manifest(&options.path)?;
    let modlist_name = manifest.name.clone();
    let manifest_hash = compute_manifest_hash(&manifest);
    let transaction = crate::resolution::preflight_wabbajack_transaction(&manifest, &manifest_hash)
        .context("Wabbajack dependency transaction is not solvable")?;
    info!(
        artifacts = transaction.artifacts.len(),
        "Wabbajack dependency transaction solved"
    );
    let game_id = GameId::from(
        modde_games::normalize_wabbajack_game(&manifest.game)
            .map_or_else(|| manifest.game.to_lowercase(), String::from),
    );
    let profile_name = options
        .profile_name
        .clone()
        .unwrap_or_else(|| modlist_name.clone());

    let store = paths::store_dir();
    let staging = paths::staging_dir().join(&profile_name);
    std::fs::create_dir_all(&store)?;
    let staging_store = StagingStore::new(&staging);
    let prepare_status = if options.safety.reset_staging {
        staging_store.reset_and_prepare().await
    } else {
        staging_store.prepare_resumable().await
    }
    .context("failed to prepare Wabbajack staging layout")?;
    info!(?prepare_status, staging = %staging.display(), "prepared Wabbajack staging");

    let client = build_http_client()?;
    let mut installer = WabbajackInstaller::new(
        manifest.clone(),
        options.path.clone(),
        store.clone(),
        staging.clone(),
    );
    if let Some(game_dir) = options.game_dir.clone() {
        installer.set_game_dir(game_dir);
    }
    installer.set_continue_on_error(options.safety.continue_on_error);
    installer.set_archive_retention(options.archive_retention);
    installer.set_missing_archive_policy(options.missing_archive_policy);
    if let Some(diagnostics_options) = options.diagnostics.clone() {
        let diagnostics = WabbajackDiagnostics::new(diagnostics_options).await?;
        installer.set_diagnostics(diagnostics);
    }

    match NexusSource::new(client.clone()) {
        Ok(nexus) => installer.add_source(crate::AnySource::Nexus(nexus)),
        Err(e) => warn!("failed to create Nexus source (no API key?): {e:#}"),
    }
    installer.add_source(crate::AnySource::WabbajackCdn(WabbajackCdnSource::new(
        client.clone(),
    )));
    installer.add_source(crate::AnySource::GitHub(crate::github::GitHubSource::new(
        client.clone(),
    )));
    installer.add_source(crate::AnySource::GoogleDrive(
        crate::gdrive::GoogleDriveSource::new(client.clone()),
    ));
    installer.add_source(crate::AnySource::Mega(crate::mega::MegaSource::new(
        client.clone(),
    )));
    installer.add_source(crate::AnySource::MediaFire(
        crate::mediafire::MediaFireSource::new(client.clone()),
    ));
    installer.add_source(crate::AnySource::Manual(crate::manual::ManualSource::new()));
    installer.add_source(crate::AnySource::Direct(DirectSource::new(client)));

    let skip_install =
        !options.force && crate::wabbajack::validator::preflight_staging(&manifest, &staging).await;

    let launcher_progress_tx = progress_tx.clone();

    if skip_install {
        if let Some(tx) = &progress_tx {
            tx.send(InstallProgress::Complete).ok();
        }
    } else {
        let (fallback_tx, mut fallback_rx) = mpsc::unbounded_channel();
        let tx = progress_tx.unwrap_or(fallback_tx);
        let drain = tokio::spawn(async move { while fallback_rx.recv().await.is_some() {} });
        installer
            .install(tx)
            .await
            .context("wabbajack install pipeline failed")?;
        drain.abort();
    }

    if !options.safety.skip_validate {
        let report = crate::wabbajack::validator::validate_install(&manifest, &staging)
            .await
            .context("Wabbajack staging validation failed")?;
        let skip_plan = MissingArchiveImpact::analyze(&manifest, &store)
            .skip_plan(&manifest, options.missing_archive_policy);
        let missing = report
            .missing
            .iter()
            .filter(|path| !skip_plan.should_omit_path(path))
            .collect::<Vec<_>>();
        let mismatches = report
            .mismatches
            .iter()
            .filter(|mismatch| !skip_plan.should_omit_path(&mismatch.path))
            .collect::<Vec<_>>();
        if !missing.is_empty() || !mismatches.is_empty() {
            anyhow::bail!(
                "Wabbajack staging validation found {} missing and {} mismatched file(s)",
                missing.len(),
                mismatches.len()
            );
        }
        info!(
            total_files = report.total_files,
            verified = report.verified,
            "Wabbajack staging validation passed"
        );
    }

    if let Some(ref game_dir) = options.game_dir
        && !options.no_deploy
    {
        deploy_mo2_to_game(&staging, game_dir, options.force)
            .await
            .context("failed to deploy mods to game directory")?;
        let launcher_report =
            configure_wine_overrides(&game_id, game_dir, &staging, options.db.as_ref()).await?;
        if !launcher_report.is_empty()
            && let Some(tx) = &launcher_progress_tx
        {
            tx.send(InstallProgress::LauncherConfigured {
                report: launcher_report,
            })
            .ok();
        }
    } else if options.no_deploy {
        info!("--no-deploy set: skipping copy of staging into game directory");
    }

    let mut enabled_mods = Vec::new();
    for archive in &manifest.archives {
        enabled_mods.push(EnabledMod {
            mod_id: modde_core::scanner::archive_mod_id(archive),
            enabled: true,
            version: None,
            fomod_config: None,
            ..Default::default()
        });
    }

    let pm = match options.db {
        Some(db) => ProfileManager::with_db(db),
        None => ProfileManager::open()
            .await
            .context("failed to open profile database")?,
    };
    let profile = Profile {
        id: None,
        name: profile_name.clone(),
        game_id: game_id.clone(),
        source: ProfileSource::Wabbajack {
            manifest_hash: manifest_hash.clone(),
        },
        mods: enabled_mods,
        overrides: ProfileManager::default_overrides(&profile_name),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: Some(LoadOrderLock::now(LockReason::Wabbajack {
            manifest_hash: manifest_hash.clone(),
        })),
    };

    save_profile_and_settings(&pm, &profile, options.game_dir.as_deref()).await?;

    if let Err(e) = cache_wabbajack_file(&options.path, &manifest_hash) {
        warn!("failed to cache wabbajack source file: {e:#}");
    }

    Ok(WabbajackInstallSummary {
        profile_name,
        modlist_name,
        game_id,
        mod_count: profile.mods.len(),
        manifest_hash,
    })
}

async fn save_profile_and_settings(
    pm: &ProfileManager,
    profile: &Profile,
    game_dir: Option<&Path>,
) -> Result<()> {
    pm.create_or_update(profile).await?;
    let mut settings = modde_core::settings::AppSettings::load();
    if let Some(gd) = game_dir {
        settings.set_game_path(&profile.game_id, gd.to_path_buf());
    }
    settings.selected_game = Some(profile.game_id.to_string());
    settings.save();
    Ok(())
}

/// Detect DLLs that need Wine `dll-override` entries for the installed modlist
/// and apply them, returning a report of what was configured.
///
/// # Errors
///
/// Returns an error if launcher configuration fails.
pub async fn configure_wine_overrides(
    game_id: &GameId,
    game_dir: &Path,
    staging: &Path,
    db: Option<&modde_core::db::ModdeDb>,
) -> Result<modde_games::launcher::LauncherConfigurationReport> {
    let mut report = modde_games::launcher::LauncherConfigurationReport::default();
    let Some(plugin) = modde_games::resolve_game_plugin(game_id.as_str()) else {
        info!(%game_id, "no game plugin found, skipping Wine DLL override detection");
        return Ok(report);
    };

    let mut overrides = plugin.wine_dll_overrides(game_dir);
    for dll in plugin.wine_dll_overrides_from_staging(staging) {
        if !overrides.contains(&dll) {
            overrides.push(dll);
        }
    }
    if overrides.is_empty() {
        return Ok(report);
    }

    let launcher = modde_games::launcher::detect_launcher(game_dir);
    #[cfg(all(target_os = "linux", feature = "linux-integrations"))]
    {
        report.wine_overrides = modde_games::launcher::apply_wine_overrides(&launcher, &overrides)?;
    }

    let tool_env_vars = if let Some(db) = db {
        modde_games::launcher::collect_tool_env_vars(game_id, db)
            .await
            .unwrap_or_default()
    } else {
        match modde_core::db::ModdeDb::open().await {
            Ok(db) => modde_games::launcher::collect_tool_env_vars(game_id, &db)
                .await
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    };

    if let Some(wrapper) =
        modde_games::launcher::generate_launch_wrapper(game_dir, staging, game_id, &tool_env_vars)?
    {
        report.wrapper_registration =
            modde_games::launcher::register_heroic_wrapper(&launcher, &wrapper.path)?;
        report.launch_wrapper = Some(wrapper);
    }

    Ok(report)
}

/// Enabled mods of the staging's MO2 profile, highest priority first.
///
/// MO2 writes `modlist.txt` with the highest-priority mod on top and marks
/// enabled mods with `+`. The profile is `selected_profile` from
/// `ModOrganizer.ini`, or the only profile when the ini is absent. `None`
/// (no ini and zero or several profiles) keeps the legacy deploy-everything
/// behavior.
fn mo2_enabled_mods(staging: &Path) -> Option<Vec<String>> {
    let selected = std::fs::read_to_string(staging.join("ModOrganizer.ini"))
        .ok()
        .and_then(|ini| {
            ini.lines().find_map(|line| {
                let value = line.trim().strip_prefix("selected_profile=")?;
                Some(
                    value
                        .strip_prefix("@ByteArray(")
                        .and_then(|v| v.strip_suffix(')'))
                        .unwrap_or(value)
                        .to_string(),
                )
            })
        });
    let profiles_dir = staging.join("profiles");
    let profile = if let Some(name) = selected {
        name
    } else {
        let mut profiles = std::fs::read_dir(&profiles_dir)
            .ok()?
            .filter_map(Result::ok)
            .filter(|entry| entry.path().join("modlist.txt").is_file());
        let only = profiles.next()?;
        if profiles.next().is_some() {
            return None;
        }
        only.file_name().to_string_lossy().into_owned()
    };
    let modlist = std::fs::read_to_string(profiles_dir.join(profile).join("modlist.txt")).ok()?;
    Some(
        modlist
            .lines()
            .filter_map(|line| line.trim_end_matches('\r').strip_prefix('+'))
            // A mod name is a single directory under mods/, never a path.
            .filter(|name| Path::new(name).file_name() == Some(std::ffi::OsStr::new(name)))
            .map(str::to_string)
            .collect(),
    )
}

pub async fn deploy_mo2_to_game(staging: &Path, game_dir: &Path, force: bool) -> Result<()> {
    #[cfg(not(unix))]
    let _ = force;

    let mods_dir = staging.join("mods");
    if !mods_dir.exists() {
        info!("no mods/ directory in staging, skipping deployment");
        return Ok(());
    }

    let mod_paths: Vec<PathBuf> = if let Some(order) = mo2_enabled_mods(staging) {
        order
            .iter()
            .map(|name| mods_dir.join(name))
            .filter(|path| path.is_dir())
            .collect()
    } else {
        let mut all = Vec::new();
        let mut entries = tokio::fs::read_dir(&mods_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                all.push(entry.path());
            }
        }
        all
    };
    info!(
        mods = mod_paths.len(),
        "deploying MO2 staging to game directory"
    );

    // Mods are visited highest priority first, so the first provider of a
    // path wins, as in MO2. Keys are lowercased because the game resolves
    // paths with Windows (case-insensitive) semantics.
    let mut claimed = std::collections::HashSet::new();
    let staging_store = StagingStore::new(staging);
    for mod_path in mod_paths {
        let mut stack = vec![mod_path.clone()];
        while let Some(dir) = stack.pop() {
            let mut dir_entries = tokio::fs::read_dir(&dir).await?;
            while let Some(entry) = dir_entries.next_entry().await? {
                let file_type = entry.file_type().await?;
                let entry_path = entry.path();
                if file_type.is_dir() {
                    stack.push(entry_path);
                    continue;
                }

                let logical_entry_path = if is_compressed_path(&entry_path) {
                    logical_path_from_compressed(&entry_path)
                } else {
                    entry_path.clone()
                };
                let rel_path = logical_entry_path
                    .strip_prefix(&mod_path)
                    .unwrap_or(&logical_entry_path);
                let filename = rel_path.file_name().unwrap_or_default().to_string_lossy();
                if filename == "meta.ini" || filename == "meta.json" {
                    continue;
                }
                if !claimed.insert(rel_path.to_string_lossy().to_lowercase()) {
                    continue;
                }

                let dest = game_dir.join(rel_path);
                let staging_relative = logical_entry_path
                    .strip_prefix(staging)
                    .unwrap_or(&logical_entry_path)
                    .to_string_lossy()
                    .to_string();
                if is_compressed_path(&entry_path) {
                    staging_store
                        .materialize_logical_file(&staging_relative, &dest)
                        .await?;
                    tracing::debug!(src = %entry_path.display(), dst = %dest.display(), "deployed compressed staging file");
                    continue;
                }

                #[cfg(unix)]
                if !force
                    && let (Ok(src_meta), Ok(dst_meta)) = (
                        tokio::fs::metadata(&entry_path).await,
                        tokio::fs::metadata(&dest).await,
                    )
                {
                    use std::os::unix::fs::MetadataExt;
                    if src_meta.ino() == dst_meta.ino() && src_meta.dev() == dst_meta.dev() {
                        continue;
                    }
                }

                let kind = modde_core::link::link_or_copy(&entry_path, &dest).await?;
                tracing::debug!(src = %entry_path.display(), dst = %dest.display(), ?kind, "deployed file");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
