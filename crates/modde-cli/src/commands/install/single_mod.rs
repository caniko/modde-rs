#![allow(clippy::wildcard_imports)]
//! Single Nexus mod install workflow.

use super::*;
use modde_sources::nexus::install::{InstallOutcome, install_single_mod};

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

    // Games we don't recognize yet still install through the generic
    // pipeline, just without game-specific hints. Probe lookup tries the
    // modde `game_id` first, then the Nexus domain — Nexus URLs carry the
    // domain (e.g. "stellarblade"), which doesn't match `game_id`
    // (e.g. "stellar-blade").
    let probe = modde_games::resolve_game_plugin(&game_domain)
        .or_else(|| modde_games::resolve_game_plugin_by_nexus_domain(&game_domain))
        .map_or_else(installer::InstallProbe::noop, modde_games::game_probe);
    let install_outcome = install_single_mod(
        &client,
        &api_key,
        &game_domain,
        mod_id,
        file_id,
        &mod_info,
        &probe,
    )
    .await?;
    match &install_outcome {
        InstallOutcome::AlreadyStaged => println!("  Already in store, skipping download"),
        InstallOutcome::Installed(plan) => {
            info!(method = plan.method.label(), "install plan decided");
            println!(
                "  Staged {} files into {}",
                plan.staged_files.len(),
                paths::store_dir()
                    .join(format!("{game_domain}_{mod_id}_{file_id}"))
                    .display()
            );
        }
        InstallOutcome::PendingUserInput { .. } | InstallOutcome::Unknown { .. } => {}
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
            // `update check` / `update apply` only track mods with a timestamp.
            installed_timestamp: Some(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
            ),
            install_status: Some(status),
            fomod_config: None,
            ..Default::default()
        });
    }

    let profile_id = save_profile_and_settings(&pm, &profile, None).await?;

    // If analyze+execute succeeded, wire the plan into the DB so
    // uninstall can remove the exact file list later.
    if let InstallOutcome::Installed(plan) = &install_outcome {
        pm.db()
            .record_install(
                profile_id,
                &modde_core::ModId::from(mod_id_str.as_str()),
                plan,
                InstallStatus::Installed,
            )
            .await
            .context("failed to persist install plan")?;
    }

    match install_outcome {
        InstallOutcome::Installed(_) | InstallOutcome::AlreadyStaged => {
            println!("Mod '{mod_id_str}' added to profile '{profile_name}'");
        }
        InstallOutcome::PendingUserInput { method } => {
            println!(
                "Mod '{mod_id_str}' staged. Install method '{method}' needs user input — \
                 open the UI to complete the wizard."
            );
        }
        InstallOutcome::Unknown { dossier_path, .. } => {
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
