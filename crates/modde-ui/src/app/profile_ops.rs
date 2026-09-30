use modde_core::profile::{ProfileManager, ReorderError, try_reorder};
use modde_core::resolver::GameId;

use super::{
    ExperimentWriteKind, ExperimentWriteOutcome, ProfileWriteOutcome, ReorderDirection,
    format_lock_reason,
};

pub(super) async fn create_profile(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<ProfileWriteOutcome, String> {
    ProfileManager::with_db(db)
        .create(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some("Profile created".to_string()),
        reload: true,
    })
}

pub(super) async fn delete_profile(
    db: modde_core::db::ModdeDb,
    name: String,
) -> Result<ProfileWriteOutcome, String> {
    ProfileManager::with_db(db)
        .delete(&name, None)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Profile '{name}' deleted")),
        reload: true,
    })
}

pub(super) async fn fork_profile(
    db: modde_core::db::ModdeDb,
    source: String,
    new_name: String,
    game_id: GameId,
    settings: modde_core::settings::AppSettings,
) -> Result<ProfileWriteOutcome, String> {
    let context = modde_games::library::context::for_game(&settings, game_id.as_str(), &db)
        .await
        .map_err(|err| err.to_string())?;
    ProfileManager::with_db(db)
        .fork_scoped(&source, &new_name, &context.saves, Default::default())
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Profile forked as '{new_name}'")),
        reload: true,
    })
}

pub(super) async fn reorder_mod(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
    direction: ReorderDirection,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;

    match try_reorder(&mut profile, &mod_id, direction) {
        Ok(()) => {
            pm.create_or_update(&profile)
                .await
                .map_err(|err| err.to_string())?;
            Ok(ProfileWriteOutcome {
                status_message: Some(format!(
                    "Moved '{mod_id}' {}",
                    match direction {
                        ReorderDirection::Up => "up",
                        ReorderDirection::Down => "down",
                    }
                )),
                reload: true,
            })
        }
        Err(ReorderError::ProfileLocked { reason }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!(
                "Load order is locked by {} — unlock the profile to reorder.",
                format_lock_reason(&reason)
            )),
            reload: false,
        }),
        Err(ReorderError::ModPinned {
            mod_id: mid,
            reason,
        }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!(
                "'{mid}' is pinned ({}) — unpin it to reorder.",
                format_lock_reason(&reason)
            )),
            reload: false,
        }),
        Err(ReorderError::AdjacentPinned { neighbor_id, .. }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!("Cannot move past a pinned mod ('{neighbor_id}').")),
            reload: false,
        }),
        Err(ReorderError::ModNotFound { mod_id: mid }) => Ok(ProfileWriteOutcome {
            status_message: Some(format!("Mod not found in profile: {mid}")),
            reload: false,
        }),
        Err(ReorderError::AtBoundary) => Ok(ProfileWriteOutcome {
            status_message: None,
            reload: false,
        }),
    }
}

pub(super) async fn add_mod_to_profile(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    profile.mods.push(modde_core::EnabledMod {
        mod_id: mod_id.clone(),
        enabled: true,
        ..Default::default()
    });
    pm.create_or_update(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Added mod: {mod_id}")),
        reload: true,
    })
}

pub(super) async fn remove_mod_from_profile(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    index: usize,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    if index >= profile.mods.len() {
        return Ok(ProfileWriteOutcome {
            status_message: None,
            reload: false,
        });
    }
    let removed_id = profile.mods[index].mod_id.clone();
    if let Some(analyzer) = modde_games::resolve_save_dependency_analyzer(profile.game_id.as_str())
    {
        let mut save_roots = Vec::new();
        let context = modde_games::library::context::for_game(
            &modde_core::settings::AppSettings::load(),
            profile.game_id.as_str(),
            pm.db(),
        )
        .await
        .map_err(|err| err.to_string())?;
        if let Some(save_dir) = context.saves.directory {
            save_roots.push(save_dir);
        }
        let vault_dir = modde_core::paths::save_vault_dir(&context.saves.scope);
        if vault_dir.exists() {
            save_roots.push(vault_dir);
        }
        let mod_dir = ProfileManager::staging_dir(&profile.name).join(&removed_id);
        let report = analyzer
            .analyze_mod_removal(&removed_id, &mod_dir, &save_roots)
            .map_err(|err| format!("Save removal gate failed: {err}"))?;
        if report.is_blocked() {
            let saves = report
                .blocking_findings
                .iter()
                .map(|finding| finding.save_path.display().to_string())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            return Err(format!(
                "Removal blocked: {saves} save(s) depend on {removed_id}. Fork the profile before removing this mod."
            ));
        }
    }
    let removed = profile.mods.remove(index);
    pm.create_or_update(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!("Removed mod: {}", removed.mod_id)),
        reload: true,
    })
}

pub(super) async fn toggle_mod_enabled(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
    enabled: bool,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    if let Some(mod_entry) = profile.mods.iter_mut().find(|entry| entry.mod_id == mod_id) {
        mod_entry.enabled = enabled;
    }
    pm.create_or_update(&profile)
        .await
        .map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(format!(
            "Mod {mod_id} {}",
            if enabled { "enabled" } else { "disabled" }
        )),
        reload: true,
    })
}

pub(super) async fn set_mod_lock(
    db: modde_core::db::ModdeDb,
    profile_name: String,
    mod_id: String,
    locked: bool,
) -> Result<ProfileWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let mut profile = pm
        .load(&profile_name, None)
        .await
        .map_err(|err| err.to_string())?;
    let Some(mod_entry) = profile.mods.iter_mut().find(|entry| entry.mod_id == mod_id) else {
        return Ok(ProfileWriteOutcome {
            status_message: None,
            reload: false,
        });
    };

    mod_entry.lock = locked.then_some(modde_core::LockReason::Manual { note: None });
    pm.update(&profile).await.map_err(|err| err.to_string())?;
    Ok(ProfileWriteOutcome {
        status_message: Some(if locked {
            format!("Pinned '{mod_id}'")
        } else {
            format!("Unpinned '{mod_id}'")
        }),
        reload: true,
    })
}

pub(super) async fn run_experiment_write(
    db: modde_core::db::ModdeDb,
    kind: ExperimentWriteKind,
    profile_name: Option<String>,
    game_id: GameId,
    settings: modde_core::settings::AppSettings,
    current_depth: usize,
) -> Result<ExperimentWriteOutcome, String> {
    let pm = ProfileManager::with_db(db);
    let context = modde_games::library::context::for_game(&settings, game_id.as_str(), pm.db())
        .await
        .map_err(|err| err.to_string())?;
    context
        .require_save_management()
        .map_err(|err| err.to_string())?;
    let fingerprint = pm
        .active(&context.saves.scope)
        .await
        .map_err(|err| err.to_string())?
        .map(|active| modde_games::save_fingerprint(&active.profile));
    match kind {
        ExperimentWriteKind::Try => {
            let profile_name =
                profile_name.ok_or_else(|| "No active profile selected".to_string())?;
            context
                .try_profile(&pm, &profile_name, fingerprint.as_ref())
                .await
                .map_err(|err| err.to_string())?;
            let next_depth = current_depth.saturating_add(1);
            Ok(ExperimentWriteOutcome {
                previous_profile: None,
                status_message: format!("Experiment started (depth {next_depth})"),
                reload: true,
            })
        }
        ExperimentWriteKind::Rollback => {
            let previous_profile = context
                .rollback_profile(&pm, fingerprint.as_ref())
                .await
                .map_err(|err| err.to_string())?;
            Ok(ExperimentWriteOutcome {
                status_message: format!("Rolled back to '{previous_profile}'"),
                previous_profile: Some(previous_profile),
                reload: true,
            })
        }
        ExperimentWriteKind::Commit => {
            pm.commit(&context.saves.scope)
                .await
                .map_err(|err| err.to_string())?;
            Ok(ExperimentWriteOutcome {
                previous_profile: None,
                status_message: "Experiment committed".to_string(),
                reload: true,
            })
        }
    }
}
