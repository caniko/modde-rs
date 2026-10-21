#![allow(clippy::wildcard_imports)]
//! Single Nexus mod install workflow.

use super::*;

pub(super) async fn handle_single_mod(url: String, profile_name: Option<String>) -> Result<()> {
    info!(%url, ?profile_name, "installing mod from Nexus");

    // Parse the Nexus URL
    let (game_domain, mod_id, file_id_opt) =
        parse_nexus_url(&url).context("failed to parse mod URL")?;

    let api_key = load_api_key().context("failed to load Nexus API key")?;
    let client = build_http_client()?;

    // If no file_id provided, fetch mod files and select the most-recent MAIN file.
    let file_id = if let Some(id) = file_id_opt {
        id
    } else {
        let api = NexusApi::new(client.clone(), api_key.clone());
        let files = api
            .get_mod_files(&game_domain, mod_id)
            .await
            .context("failed to fetch mod files")?;

        // Prefer the newest MAIN file; fall back to the first file if no
        // MAIN category exists.
        let file = files
            .files
            .iter()
            .filter(|f| f.category_name.as_deref() == Some("MAIN"))
            .max_by_key(|f| f.uploaded_timestamp)
            .or_else(|| {
                warn!(%mod_id, "no MAIN category file found, using first available file");
                files.files.first()
            })
            .ok_or_else(|| anyhow::anyhow!("no files found for mod {mod_id}"))?;

        file.file_id
    };

    // Fetch mod metadata from Nexus for the display name.
    let api = NexusApi::new(client.clone(), api_key.clone());
    let mod_info = api
        .get_mod(&game_domain, mod_id)
        .await
        .context("failed to fetch mod info from Nexus")?;

    println!(
        "Installing mod: {} ({game_domain}/mods/{mod_id}, file {file_id})",
        mod_info.name
    );

    let store = paths::store_dir();
    let mod_store_dir = store.join(format!("{game_domain}_{mod_id}_{file_id}"));

    // Extract into a temp staging dir rather than straight into the
    // store, so `installer::analyze` runs on the raw archive tree and
    // `installer::execute` can decide the final staging layout. The
    // archive is kept until execution is committed so we can compute
    // its source hash and also dump the dossier from it if needed.
    let archive_path = store.join(format!("{mod_id}_{file_id}.zip"));
    let staging_root =
        paths::staging_dir().join(format!("install_{game_domain}_{mod_id}_{file_id}"));

    let mut install_outcome = InstallOutcome::AlreadyStaged;
    if mod_store_dir.exists() {
        println!("  Already in store, skipping download");
    } else {
        // Fresh install: download + extract + analyze + execute.
        let download_url = generate_download_link(&client, &api_key, &game_domain, mod_id, file_id)
            .await
            .context("failed to get download link")?;
        download_file(&client, &download_url, &archive_path)
            .await
            .context("failed to download mod")?;

        // Extract into staging (not the final store dir).
        if staging_root.exists() {
            let _ = std::fs::remove_dir_all(&staging_root);
        }
        std::fs::create_dir_all(&staging_root)?;
        installer::extract_archive(&archive_path, &staging_root)
            .context("failed to extract mod archive")?;

        let source_hash = installer::xxh64_file_hex(&archive_path)
            .context("failed to hash downloaded archive")?;
        // Archive is no longer needed once we have the hash.
        let _ = std::fs::remove_file(&archive_path);

        // Resolve the game plugin to build a probe. Games we don't
        // recognize yet still install through the generic pipeline,
        // just without game-specific hints.
        // Probe lookup tries the modde `game_id` first (in case the
        // caller passed one through), then falls back to the Nexus
        // domain — Nexus URLs carry the domain (e.g. "stellarblade"),
        // which doesn't match `game_id` (e.g. "stellar-blade").
        let probe = modde_games::resolve_game_plugin(&game_domain)
            .or_else(|| modde_games::resolve_game_plugin_by_nexus_domain(&game_domain))
            .map_or_else(installer::InstallProbe::noop, modde_games::game_probe);

        let mut plan = installer::analyze(&staging_root, &probe, source_hash)
            .context("installer analyze failed")?;
        info!(method = plan.method.label(), "install plan decided");

        install_outcome = match &plan.method {
            InstallMethod::Unknown { .. } => {
                let dossier = write_unknown_dossier(
                    &staging_root,
                    &game_domain,
                    mod_id,
                    file_id,
                    &mod_info,
                    &plan.method,
                    &plan.source_archive_hash,
                )?;
                InstallOutcome::Unknown {
                    dossier_path: dossier,
                }
            }
            _ if !plan.method.is_ready() => {
                // FOMOD / BAIN with no config yet — copy the raw
                // extracted tree into the store so the UI wizard can
                // walk it later without re-downloading. The archive
                // itself is already gone (removed above once hashed).
                std::fs::create_dir_all(&mod_store_dir)?;
                copy_dir_tree(&staging_root, &mod_store_dir)
                    .context("failed to copy staging → store for pending install")?;
                InstallOutcome::PendingUserInput {
                    method: plan.method.label().to_string(),
                }
            }
            _ => {
                std::fs::create_dir_all(&mod_store_dir)?;
                match installer::execute(&mut plan, &staging_root, &mod_store_dir) {
                    Ok(files) => {
                        println!(
                            "  Staged {} files into {}",
                            files.len(),
                            mod_store_dir.display()
                        );
                        InstallOutcome::Installed { plan }
                    }
                    Err(InstallerError::UnknownMethod { reason: _ }) => {
                        let dossier = write_unknown_dossier(
                            &staging_root,
                            &game_domain,
                            mod_id,
                            file_id,
                            &mod_info,
                            &plan.method,
                            &plan.source_archive_hash,
                        )?;
                        InstallOutcome::Unknown {
                            dossier_path: dossier,
                        }
                    }
                    Err(InstallerError::RequiresUserInput { method }) => {
                        InstallOutcome::PendingUserInput {
                            method: method.to_string(),
                        }
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        };

        // Clean up the staging dir regardless of outcome — anything
        // important has already been copied into the store or the
        // dossier directory.
        let _ = std::fs::remove_dir_all(&staging_root);
    }

    // Load or create profile.
    //
    // Lock policy for `modde install mod` (single-Nexus-mod flow):
    //   - **New profile** (not yet in DB) → `load_order_lock: None`.
    //     Fresh profiles start Manual; if the user wants to later mark
    //     them authoritative they run `modde profile lock`.
    //   - **Existing profile** → preserve whatever lock is already there
    //     (Wabbajack / Collection / TomlImport / Manual). If a lock is
    //     present, emit a warning because adding a mod drifts the load
    //     order away from the authoritative source — the lock exists
    //     precisely to prevent that kind of drift.
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile_name = profile_name.unwrap_or_else(|| game_domain.clone());
    let mod_id_str = format!("{game_domain}_{mod_id}_{file_id}");

    let mut profile = match pm.load(&profile_name, None).await {
        Ok(p) => p,
        Err(_) => Profile {
            id: None,
            name: profile_name.clone(),
            game_id: modde_core::GameId::from(game_domain.clone()),
            source: ProfileSource::Manual,
            mods: Vec::new(),
            overrides: ProfileManager::default_overrides(&profile_name),
            load_order_rules: smallvec::SmallVec::new(),
            load_order_lock: None,
        },
    };

    if let Some(lock) = profile.load_order_lock.as_ref() {
        eprintln!(
            "  warning: profile '{profile_name}' is locked ({}). Adding a mod will \
             drift from the authoritative source. Run `modde profile unlock \
             {profile_name}` first if this is intentional.",
            format_lock_reason_short(&lock.reason),
        );
    }

    // Add mod to profile if not already present
    let status = install_outcome.status();
    if !profile.mods.iter().any(|m| m.mod_id == mod_id_str) {
        profile.mods.push(EnabledMod {
            mod_id: mod_id_str.clone(),
            display_name: Some(mod_info.name.clone()),
            enabled: true,
            version: Some(mod_info.version.clone()),
            nexus_mod_id: Some(mod_id),
            nexus_file_id: Some(file_id),
            nexus_game_domain: Some(game_domain.clone()),
            install_status: Some(status),
            fomod_config: None,
            ..Default::default()
        });
    }

    save_profile_and_settings(&pm, &profile, None).await?;

    // If analyze+execute succeeded, wire the plan into the DB so
    // uninstall can remove the exact file list later.
    if let InstallOutcome::Installed { plan } = &install_outcome {
        let db = ModdeDb::open()
            .await
            .context("failed to open mod db for record_install")?;
        let profile_id = pm
            .load(&profile_name, None)
            .await
            .context("failed to reload profile to get id")?
            .id
            .ok_or_else(|| anyhow::anyhow!("saved profile has no database id"))?;
        db.record_install(
            profile_id,
            &modde_core::ModId::from(mod_id_str.as_str()),
            plan,
            InstallStatus::Installed,
        )
        .await
        .context("failed to persist install plan")?;
    }

    match install_outcome {
        InstallOutcome::Installed { .. } | InstallOutcome::AlreadyStaged => {
            println!("Mod '{mod_id_str}' added to profile '{profile_name}'");
        }
        InstallOutcome::PendingUserInput { method } => {
            println!(
                "Mod '{mod_id_str}' staged. Install method '{method}' needs user input — \
                 open the UI to complete the wizard."
            );
        }
        InstallOutcome::Unknown { dossier_path } => {
            println!("Mod '{mod_id_str}' has an unknown install layout. Dossier written to:");
            println!("  {}", dossier_path.display());
            println!(
                "Run `/modde-installer {mod_id_str}` inside Claude Code to extend modde \
                 with a handler for this layout."
            );
        }
    }

    Ok(())
}

/// High-level result of the Phase 4 pipeline, consumed by the profile-
/// write step below.
enum InstallOutcome {
    Installed {
        plan: modde_core::installer::InstallPlan,
    },
    PendingUserInput {
        method: String,
    },
    Unknown {
        dossier_path: PathBuf,
    },
    /// The mod was already in the store before we ran — no fresh plan
    /// was produced, so we only refresh the profile row.
    AlreadyStaged,
}

impl InstallOutcome {
    fn status(&self) -> InstallStatus {
        match self {
            InstallOutcome::Installed { .. } | InstallOutcome::AlreadyStaged => {
                InstallStatus::Installed
            }
            InstallOutcome::PendingUserInput { .. } => InstallStatus::PendingUserInput,
            InstallOutcome::Unknown { .. } => InstallStatus::Unknown,
        }
    }
}

/// Write a dossier for an unknown-layout mod and return the dossier
/// directory path so the CLI can tell the user where to look.
fn write_unknown_dossier(
    extracted_dir: &Path,
    game_domain: &str,
    mod_id: NexusModId,
    file_id: NexusFileId,
    mod_info: &modde_sources::nexus::api::NexusMod,
    method: &InstallMethod,
    source_hash: &str,
) -> Result<PathBuf> {
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
        source_archive_hash: source_hash.to_string(),
    };
    let trace = vec![installer::ProbeTrace {
        probe: "generic+game".to_string(),
        matched: false,
        note: format!("verdict: {}", method.label()),
    }];
    installer::dump_dossier(extracted_dir, &ctx, method, trace)
        .context("failed to write unknown-installer dossier")
}

/// Recursively copy `src` into `dst`, preserving directory structure.
/// Used for the `PendingUserInput` path where we want to keep the raw
/// archive around so the UI wizard can walk it, without touching the
/// files we would otherwise move via `installer::execute`.
fn copy_dir_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    if !src.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_tree(&src_path, &dst_path)?;
        } else if src_path.is_file() {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
