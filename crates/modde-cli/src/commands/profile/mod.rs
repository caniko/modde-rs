use anyhow::{Context, Result};
use smallvec::SmallVec;
use tracing::info;

use modde_core::error::CoreError;
use modde_core::profile::{
    ActivateResult, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::resolver::GameId;

use super::{compute_fingerprint, installation_context, supports_save_profiles};
use crate::cli::args::ProfileAction;

/// Human-readable byte size (KB/MB/GB) for `lock-info` output.
mod dedup;

use dedup::dedup;

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

/// Render a `LockReason` as a short, human-readable phrase for CLI output.
fn format_lock_reason(reason: &LockReason) -> String {
    match reason {
        LockReason::Wabbajack { manifest_hash } => format!("Wabbajack (hash {manifest_hash})"),
        LockReason::NexusCollection { slug, version } => {
            format!("Nexus Collection '{slug}' v{version}")
        }
        LockReason::TomlImport { source_path } => format!("TOML import from {source_path}"),
        LockReason::Manual { note: Some(n) } => format!("manual ({n})"),
        LockReason::Manual { note: None } => "manual".to_string(),
    }
}

/// Look up a mod in a profile by `mod_id` and return its index, or construct
/// a typed [`CoreError::ModNotFound`] whose `candidates` field lists the
/// first 5 mod ids in the profile as a hint.
fn find_mod_or_bail(profile: &Profile, mod_id: &str) -> Result<usize> {
    if let Some(idx) = profile.mods.iter().position(|m| m.mod_id == mod_id) {
        return Ok(idx);
    }
    let candidates: SmallVec<[String; 5]> = profile
        .mods
        .iter()
        .take(5)
        .map(|m| m.mod_id.clone())
        .collect();
    Err(CoreError::ModNotFound {
        profile: profile.name.clone(),
        mod_id: mod_id.to_string(),
        candidates,
    }
    .into())
}

pub async fn handle(action: ProfileAction) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;

    match action {
        ProfileAction::List { game } => {
            let profiles = match game {
                Some(ref g) => pm.list_for_game(&GameId::from(g.as_str())).await?,
                None => pm.list().await?,
            };
            if profiles.is_empty() {
                println!("No profiles found.");
            } else {
                for p in profiles {
                    println!(
                        "  {} (game: {}, {} mods, source: {})",
                        p.name, p.game_id, p.mod_count, p.source_type
                    );
                }
            }
        }
        ProfileAction::Switch { name, game } => {
            let context = installation_context(&game, &pm).await?;
            context.require_save_management()?;
            let save_dir = &context.saves.directory;
            let current = pm.active(&context.saves.scope).await?;
            let fp = if let Some(current) = current {
                compute_fingerprint(&pm, &current.profile.name, &game).await
            } else {
                None
            };
            match context.activate_profile(&pm, &name, fp.as_ref()).await? {
                ActivateResult::Activated => {
                    info!(profile = %name, "switched to profile");
                    if save_dir.is_some() {
                        println!("Switched to profile: {name} (saves swapped)");
                    } else if matches!(supports_save_profiles(&game), Ok(false)) {
                        println!("Switched to profile: {name} (save profiles unsupported)");
                    } else {
                        println!("Switched to profile: {name} (no save directory detected)");
                    }
                }
                ActivateResult::AdoptionRequired { save_count } => {
                    println!(
                        "Found {save_count} existing save(s) in the game directory.\n\
                         Run `modde save adopt --game {game} --profile {name}` to adopt them first,\n\
                         or use a different profile."
                    );
                }
            }
        }
        ProfileAction::Create { name, game } => {
            let profile = Profile {
                id: None,
                name: name.clone(),
                game_id: modde_core::GameId::from(game.clone()),
                source: ProfileSource::Manual,
                mods: vec![],
                overrides: ProfileManager::default_overrides(&name),
                load_order_rules: smallvec::SmallVec::new(),
                load_order_lock: None,
            };
            pm.create(&profile).await?;
            println!("Created profile: {name} (game: {game})");
        }
        ProfileAction::Delete { name, game } => {
            pm.delete(&name, game.as_deref().map(GameId::from).as_ref())
                .await?;
            println!("Deleted profile: {name}");
        }
        ProfileAction::Try { name, game } => {
            let context = installation_context(&game, &pm).await?;
            context.require_save_management()?;
            let current = pm.active(&context.saves.scope).await?;
            let fp = if let Some(current) = current {
                compute_fingerprint(&pm, &current.profile.name, &game).await
            } else {
                None
            };
            context.try_profile(&pm, &name, fp.as_ref()).await?;
            let depth = pm
                .active(&context.saves.scope)
                .await?
                .map_or(0, |a| a.experiment_depth);
            println!("Experimenting with profile: {name} (stack depth: {depth})");
            println!(
                "Use `modde profile rollback --game {game}` to undo, or `modde profile commit --game {game}` to accept."
            );
        }
        ProfileAction::Rollback { game } => {
            let context = installation_context(&game, &pm).await?;
            context.require_save_management()?;

            // Compute fingerprint for the current (about-to-be-rolled-back) profile
            let fp = pm.active(&context.saves.scope).await?.and_then(|info| {
                if !supports_save_profiles(&game).ok()? {
                    return None;
                }
                Some(modde_games::save_fingerprint(&info.profile))
            });

            let restored = context.rollback_profile(&pm, fp.as_ref()).await?;
            println!("Rolled back to profile: {restored}");
        }
        ProfileAction::Commit { game } => {
            let context = installation_context(&game, &pm).await?;
            pm.commit(&context.saves.scope).await?;
            println!("Experiment accepted. Rollback stack cleared for game: {game}");
        }
        ProfileAction::Active { game } => {
            let context = installation_context(&game, &pm).await?;
            match pm.active(&context.saves.scope).await? {
                Some(info) => {
                    println!(
                        "Active profile: {} (game: {})",
                        info.profile.name, info.profile.game_id
                    );
                    println!("  Mods: {}", info.profile.mods.len());
                    if info.experiment_depth > 0 {
                        println!(
                            "  Experiment depth: {} (use `rollback` to undo or `commit` to accept)",
                            info.experiment_depth
                        );
                    }

                    // Show fingerprint info
                    if let Some(fp) =
                        compute_fingerprint(&pm, &info.profile.name, info.profile.game_id.as_str())
                            .await
                    {
                        if fp.is_empty() {
                            println!("  Save fingerprint: none (no save-breaking mods)");
                        } else {
                            println!(
                                "  Save fingerprint: {} ({} save-breaking mod(s))",
                                fp.short_hash(),
                                fp.mod_ids.len()
                            );
                        }
                    }
                }
                None => {
                    println!("No active profile for game: {game}");
                }
            }
        }
        ProfileAction::Fork {
            source,
            name,
            game,
            unlock,
        } => {
            let context = installation_context(&game, &pm).await?;
            let id = pm
                .fork_scoped(
                    &source,
                    &name,
                    &context.saves,
                    modde_core::profile::ForkOptions { unlock },
                )
                .await?;
            if unlock {
                println!(
                    "Forked profile '{source}' -> '{name}' (id: {id}, mods + saves cloned, locks stripped)"
                );
            } else {
                println!("Forked profile '{source}' -> '{name}' (id: {id}, mods + saves cloned)");
            }
        }
        ProfileAction::Lock { name, game, note } => {
            let mut profile = pm
                .load(&name, game.as_deref().map(GameId::from).as_ref())
                .await?;
            if let Some(existing) = profile.load_order_lock.as_ref() {
                anyhow::bail!(
                    "profile '{name}' is already locked by {} — unlock first to re-lock",
                    format_lock_reason(&existing.reason)
                );
            }
            profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Manual {
                note: note.clone(),
            }));
            pm.update(&profile).await?;
            match note {
                Some(n) => println!("Locked profile '{name}' (manual: {n})"),
                None => println!("Locked profile '{name}' (manual)"),
            }
        }
        ProfileAction::Unlock { name, game } => {
            let mut profile = pm
                .load(&name, game.as_deref().map(GameId::from).as_ref())
                .await?;
            match profile.load_order_lock.take() {
                None => println!("Profile '{name}' was not locked."),
                Some(prior) => {
                    pm.update(&profile).await?;
                    println!(
                        "Unlocked profile '{name}' (was {}, locked at {})",
                        format_lock_reason(&prior.reason),
                        prior.locked_at
                    );
                }
            }
        }
        ProfileAction::LockInfo { name, game } => {
            let profile = pm
                .load(&name, game.as_deref().map(GameId::from).as_ref())
                .await?;
            match profile.load_order_lock.as_ref() {
                None => println!("Profile '{name}' is not locked."),
                Some(lock) => {
                    println!("Profile '{name}' lock:");
                    println!("  Reason:    {}", format_lock_reason(&lock.reason));
                    println!("  Locked at: {}", lock.locked_at);
                    // For Wabbajack locks, also show the cached source file
                    // status so the user can re-verify / re-import from it.
                    if let LockReason::Wabbajack { manifest_hash } = &lock.reason {
                        let cache_path = modde_core::paths::wabbajack_cache_path(manifest_hash);
                        match std::fs::metadata(&cache_path) {
                            Ok(meta) => println!(
                                "  Source:    {} ({})",
                                cache_path.display(),
                                format_bytes(meta.len())
                            ),
                            Err(_) => println!(
                                "  Source:    {} (missing — re-run \
                                 `modde scan --manifest <file> --import-to {name}`)",
                                cache_path.display()
                            ),
                        }
                    }
                }
            }
            let pinned: Vec<&str> = profile
                .mods
                .iter()
                .filter(|m| m.lock.is_some())
                .map(|m| m.mod_id.as_str())
                .collect();
            if !pinned.is_empty() {
                println!("  Per-mod pins ({}):", pinned.len());
                for id in pinned.iter().take(20) {
                    println!("    - {id}");
                }
                if pinned.len() > 20 {
                    println!("    ... and {} more", pinned.len() - 20);
                }
            }
        }
        ProfileAction::LockMod {
            name,
            mod_id,
            game,
            note,
        } => {
            // A per-mod pin is independent of the profile-level lock. If the
            // profile is already Wabbajack-locked, lock-mod still succeeds —
            // the pin takes effect after a later `unlock` or `fork --unlock`.
            let mut profile = pm
                .load(&name, game.as_deref().map(GameId::from).as_ref())
                .await?;
            let idx = find_mod_or_bail(&profile, &mod_id)?;
            let entry = profile
                .mods
                .get_mut(idx)
                .with_context(|| format!("mod '{mod_id}' disappeared while updating profile"))?;
            if let Some(existing) = entry.lock.as_ref() {
                anyhow::bail!(
                    "mod '{mod_id}' is already pinned ({}) — unlock-mod first to re-pin",
                    format_lock_reason(existing)
                );
            }
            entry.lock = Some(LockReason::Manual { note: note.clone() });
            pm.update(&profile).await?;
            match note {
                Some(n) => println!("Pinned '{mod_id}' in profile '{name}' (manual: {n})"),
                None => println!("Pinned '{mod_id}' in profile '{name}'"),
            }
        }
        ProfileAction::UnlockMod { name, mod_id, game } => {
            let mut profile = pm
                .load(&name, game.as_deref().map(GameId::from).as_ref())
                .await?;
            let idx = find_mod_or_bail(&profile, &mod_id)?;
            let entry = profile
                .mods
                .get_mut(idx)
                .with_context(|| format!("mod '{mod_id}' disappeared while updating profile"))?;
            match entry.lock.take() {
                None => println!("'{mod_id}' was not pinned"),
                Some(prior) => {
                    pm.update(&profile).await?;
                    println!("Unpinned '{mod_id}' (was {})", format_lock_reason(&prior));
                }
            }
        }
        ProfileAction::Dedup {
            name,
            game,
            manifest,
            apply,
        } => {
            dedup(&pm, &name, game.as_deref(), manifest.as_deref(), apply).await?;
        }
    }

    Ok(())
}
