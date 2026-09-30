//! Candidate profile launch, cleanup, and crash-log handling.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use modde_core::bisect::apply_result;
use modde_core::profile::{ActivateResult, Profile, ProfileManager};
use modde_core::{BisectResult, BisectSaveSafety, BisectSession, BisectStatus, GameId};

use crate::commands::{
    compute_fingerprint, load_plugin_order, supports_save_profiles,
};

#[derive(serde::Serialize, serde::Deserialize)]
pub(super) struct InstallationPin {
    pub installation: String,
    pub save_scope: GameId,
}

pub(super) fn pin_path(session: &str) -> PathBuf {
    modde_core::paths::modde_data_dir().join("bisect").join(format!("{session}.installation.json"))
}

pub(super) async fn installation(pm: &ProfileManager, session: &BisectSession) -> Result<modde_games::library::context::InstallationContext> {
    let pin: InstallationPin = serde_json::from_slice(&std::fs::read(pin_path(&session.session_id))
        .context("bisect has no installation pin; start a new installation-scoped bisect")?)?;
    let games = modde_games::library::catalogue(&modde_core::settings::AppSettings::load())?.games;
    let game = games.iter().find(|game| game.id == pin.installation).context("bisect installation is unavailable")?;
    let context = modde_games::library::context::for_installation(game, &games, pm.db()).await?;
    anyhow::ensure!(context.saves.game_id == session.game_id && context.saves.scope == pin.save_scope,
        "bisect installation/save destination changed; restore the pinned configuration");
    Ok(context)
}

pub(super) async fn create_candidate_profile(
    pm: &ProfileManager,
    source: &Profile,
    candidate_name: &str,
    disabled_mod_ids: &[String],
) -> Result<Profile> {
    let disabled: BTreeSet<&str> = disabled_mod_ids.iter().map(String::as_str).collect();
    let mut candidate = source.clone();
    candidate.id = None;
    candidate.name = candidate_name.to_string();
    candidate.overrides = ProfileManager::default_overrides(candidate_name);
    for m in &mut candidate.mods {
        if m.enabled && disabled.contains(m.mod_id.as_str()) {
            m.enabled = false;
        }
    }
    let source_id = source
        .id
        .ok_or_else(|| anyhow::anyhow!("source profile '{}' has no database ID", source.name))?;
    let candidate_id = pm.create(&candidate).await?;
    pm.db()
        .copy_profile_auxiliary_state(source_id, candidate_id)
        .await?;
    pm.load(candidate_name, Some(&source.game_id))
        .await
        .map_err(Into::into)
}

pub(super) fn enforce_save_safety(
    session: &BisectSession,
    source: &Profile,
    candidate: &Profile,
) -> Result<()> {
    if session.save_safety == BisectSaveSafety::Force {
        return Ok(());
    }
    if !matches!(supports_save_profiles(session.game_id.as_str()), Ok(true)) {
        return Ok(());
    }
    let source_fp = modde_games::save_fingerprint(source);
    let candidate_fp = modde_games::save_fingerprint(candidate);
    if source_fp.hash == candidate_fp.hash {
        return Ok(());
    }
    let source_set: BTreeSet<_> = source_fp.mod_ids.iter().cloned().collect();
    let candidate_set: BTreeSet<_> = candidate_fp.mod_ids.iter().cloned().collect();
    let removed = source_set
        .difference(&candidate_set)
        .cloned()
        .collect::<Vec<_>>();
    let added = candidate_set
        .difference(&source_set)
        .cloned()
        .collect::<Vec<_>>();
    anyhow::bail!(
        "refusing unsafe bisect step because candidate '{}' changes the save-affecting mod set. removed=[{}] added=[{}]. Re-run start with --force-save-risk to allow this.",
        candidate.name,
        removed.join(", "),
        added.join(", ")
    );
}

pub(super) async fn launch_candidate(
    pm: &ProfileManager,
    profile_name: &str,
    session: &BisectSession,
    no_deploy: bool,
) -> Result<Option<std::process::ExitStatus>> {
    let context = installation(pm, session).await?;
    let current = pm.db().load_bisect_session(&session.session_id).await?;
    anyhow::ensure!(current.current_candidate_profile.as_deref() == Some(profile_name), "bisect candidate changed before launch");
    match crate::commands::library::play(&context.game.id, crate::commands::library::PlayOptions {
        profile: Some(profile_name.into()), no_deploy, require_observed: true,
        expected_scope: Some(context.saves.scope),
        bisect: Some(super::Completion { session: session.session_id.clone(), step: current.current_step_id.context("bisect step missing")?, started_unix_ms: None }),
        ..Default::default()
    }).await? {
        modde_games::library::launch::LaunchOutcome::Exited(status) => Ok(Some(status)),
        modde_games::library::launch::LaunchOutcome::Requested => Ok(None),
    }
}

pub(super) async fn complete_and_advance(
    pm: &ProfileManager,
    session_id: String,
    step_id: i64,
    result: BisectResult,
    observed_signal: Option<String>,
    notes: Option<String>,
) -> Result<()> {
    let session = pm.db().load_bisect_session(&session_id).await?;
    let step = pm.db().load_bisect_step(step_id).await?;
    pm.db()
        .complete_bisect_step(
            step_id,
            result,
            observed_signal.as_deref(),
            notes.as_deref(),
        )
        .await?;

    let suspects = apply_result(&step, result);
    let mut known_good = session.known_good_mod_ids.clone();
    let mut known_bad = session.known_bad_mod_ids.clone();
    match result {
        BisectResult::Good => known_bad.extend(step.disabled_mod_ids.clone()),
        BisectResult::Bad => known_good.extend(step.disabled_mod_ids.clone()),
    }
    known_good.sort();
    known_good.dedup();
    known_bad.sort();
    known_bad.dedup();

    let status = if suspects.is_empty() {
        BisectStatus::Inconclusive
    } else if suspects.len() == 1 {
        BisectStatus::Complete
    } else {
        BisectStatus::Active
    };
    pm.db()
        .update_bisect_session_state(
            &session_id,
            status,
            &suspects,
            &known_good,
            &known_bad,
            None,
            None,
        )
        .await?;
    let updated = pm.db().load_bisect_session(&session_id).await?;
    if matches!(status, BisectStatus::Complete | BisectStatus::Inconclusive) {
        restore_source_profile(pm, &updated).await?;
        cleanup_candidates(pm, &updated).await?;
    }
    print_completion_if_any(pm, &updated).await?;
    if status == BisectStatus::Active {
        println!("Next candidate: modde bisect run {session_id}");
    }
    Ok(())
}

pub(super) async fn finish_without_candidate(
    pm: &ProfileManager,
    session: &BisectSession,
) -> Result<()> {
    let status = if session.suspect_mod_ids.is_empty() {
        BisectStatus::Inconclusive
    } else {
        BisectStatus::Complete
    };
    pm.db()
        .update_bisect_session_state(
            &session.session_id,
            status,
            &session.suspect_mod_ids,
            &session.known_good_mod_ids,
            &session.known_bad_mod_ids,
            None,
            None,
        )
        .await?;
    let updated = pm.db().load_bisect_session(&session.session_id).await?;
    restore_source_profile(pm, &updated).await?;
    cleanup_candidates(pm, &updated).await?;
    print_completion_if_any(pm, &updated).await
}

pub(super) async fn print_completion_if_any(
    pm: &ProfileManager,
    session: &BisectSession,
) -> Result<()> {
    match session.status {
        BisectStatus::Complete if session.suspect_mod_ids.len() == 1 => {
            let profile = pm
                .load(&session.source_profile_name, Some(&session.game_id))
                .await?;
            let id = &session.suspect_mod_ids[0];
            let entry = profile.mods.iter().find(|m| &m.mod_id == id);
            let display = entry
                .and_then(|m| m.display_name.as_deref())
                .unwrap_or(id.as_str());
            let version = entry
                .and_then(|m| m.version.as_deref())
                .unwrap_or("unknown");
            println!("Culprit: {display} ({id}, version: {version})");
        }
        BisectStatus::Complete => {
            println!(
                "Minimal suspect set: {}",
                session.suspect_mod_ids.join(", ")
            );
        }
        BisectStatus::Inconclusive => {
            println!("Bisect inconclusive: no reproducing suspect set remains.");
        }
        _ => {}
    }
    Ok(())
}

pub(super) async fn cleanup_candidates(pm: &ProfileManager, session: &BisectSession) -> Result<()> {
    if session.keep_profiles {
        return Ok(());
    }
    for profile in pm
        .db()
        .bisect_candidate_profiles(&session.session_id)
        .await?
    {
        if profile.starts_with("__bisect_") {
            match pm.delete(&profile, Some(&session.game_id)).await {
                Ok(()) | Err(modde_core::error::CoreError::ProfileNotFound(_)) => {}
                Err(error) => return Err(error).with_context(|| format!("removing bisect candidate {profile}")),
            }
        }
    }
    Ok(())
}

pub(super) async fn restore_source_profile(
    pm: &ProfileManager,
    session: &BisectSession,
) -> Result<()> {
    let context = installation(pm, session).await?;
    let active = pm.active(&context.saves.scope).await?;
    if active.as_ref().is_some_and(|active| active.profile.name == session.source_profile_name) { return Ok(()); }
    let fp = if let Some(active) = active { compute_fingerprint(pm, &active.profile.name, session.game_id.as_str()).await } else { None };
    match context.activate_profile(pm, &session.source_profile_name, fp.as_ref()).await? {
        ActivateResult::Activated => {}
        ActivateResult::AdoptionRequired { .. } => anyhow::bail!("adopt this installation's saves before restoring the bisect source"),
    }
    Ok(())
}

pub(super) async fn analyze_crash_log(
    pm: &ProfileManager,
    profile: &Profile,
    log_path: &Path,
) -> Result<()> {
    let raw = std::fs::read_to_string(log_path)
        .with_context(|| format!("failed to read crash log {}", log_path.display()))?;
    let profile_id = profile.id.ok_or_else(|| {
        anyhow::anyhow!("candidate profile '{}' has no database ID", profile.name)
    })?;
    let active_plugins = load_plugin_order(pm, profile).await?;
    let enabled_mods = profile
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.mod_id.as_str())
        .collect::<BTreeSet<_>>();
    let installed_files = pm
        .db()
        .installed_files_for_profile(profile_id)
        .await?
        .into_iter()
        .filter(|(mod_id, _)| enabled_mods.contains(mod_id.as_str()))
        .collect();
    let tool_files = pm.db().load_all_applied_files(&profile.game_id).await?;
    let report = modde_core::crash::correlate_crash_log(
        log_path,
        &raw,
        modde_core::crash::CrashLogFormat::Auto,
        modde_core::crash::CrashCorrelationInput {
            game_id: profile.game_id.to_string(),
            profile: profile.clone(),
            active_plugins,
            installed_files,
            tool_files,
        },
    );
    pm.db()
        .record_crash_log(Some(profile_id), &report, &raw)
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

pub(super) fn newest_file_after(dir: &Path, after: SystemTime) -> Result<Option<PathBuf>> {
    let mut newest = None;
    collect_new_files(dir, after, &mut newest)?;
    Ok(newest.map(|(_, path)| path))
}

pub(super) fn collect_new_files(
    dir: &Path,
    after: SystemTime,
    newest: &mut Option<(SystemTime, PathBuf)>,
) -> Result<()> {
    for entry in
        std::fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.is_dir() {
            collect_new_files(&path, after, newest)?;
        } else if meta.is_file() {
            let modified = meta.modified().unwrap_or(UNIX_EPOCH);
            if modified > after && newest.as_ref().is_none_or(|(old, _)| modified > *old) {
                *newest = Some((modified, path));
            }
        }
    }
    Ok(())
}
