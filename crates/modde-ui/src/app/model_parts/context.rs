#![allow(clippy::wildcard_imports)]
//! context model helpers.

use super::diagnostics::{compute_data_tab_conflicts, compute_save_fingerprint};
use super::*;

/// Async loader for the profile + data-tab + tool context. Pure DB work is
/// awaited directly; synchronous data-tab analysis and tool-state generation
/// move into their own blocking bridges.
pub(crate) async fn load_profile_context(
    db: modde_core::db::ModdeDb,
    request: ProfileContextRequest,
) -> Result<ProfileContextSnapshot, String> {
    let pm = ProfileManager::with_db(db.clone());
    let selected_game_id = request.selected_game.as_deref().map(GameId::from);
    let installation = match selected_game_id.as_ref() {
        Some(game_id) => modde_games::library::context::for_game(&request.settings, game_id.as_str(), pm.db()).await.ok(),
        None => None,
    };

    // Profile list (scoped to the game when one is selected, else all).
    let profiles = match selected_game_id.as_ref() {
        Some(game_id) => pm.list_for_game(game_id).await.unwrap_or_default(),
        None => pm.list().await.unwrap_or_default(),
    };

    // Active profile: recompute from the DB for game switches; otherwise trust
    // the caller-provided name.
    let active_profile = if request.recompute_active {
        match installation.as_ref() {
            Some(context) => pm
                .active(&context.saves.scope)
                .await
                .ok()
                .flatten()
                .map(|info| info.profile.name),
            None => None,
        }
        .or_else(|| profiles.first().map(|profile| profile.name.clone()))
    } else {
        request.active_profile.clone()
    };

    // Load the active profile.
    let profile_outcome = match active_profile.as_deref() {
        Some(name) => match pm.load(name, selected_game_id.as_ref()).await {
            Ok(profile) => {
                let experiment_depth = match installation.as_ref() {
                    Some(context) => pm.active(&context.saves.scope).await.ok().flatten().map_or(0, |info| info.experiment_depth),
                    None => 0,
                };
                let current_fingerprint = compute_save_fingerprint(&profile);
                let mod_id_filter_keys = modde_core::filter::mod_id_filter_keys(&profile.mods);
                ProfileLoadOutcome::Loaded {
                    profile: Box::new(profile),
                    experiment_depth,
                    current_fingerprint,
                    mod_id_filter_keys,
                }
            }
            // Load failed — keep the previously-loaded profile (and its derived
            // fields) untouched, as the old synchronous helper did.
            Err(_) => ProfileLoadOutcome::KeepPrevious,
        },
        None => ProfileLoadOutcome::Cleared,
    };

    // Data-tab conflicts come from whichever profile will be displayed: the
    // freshly loaded one, the kept previous one, or none.
    let effective_profile = match &profile_outcome {
        ProfileLoadOutcome::Loaded { profile, .. } => Some(profile.as_ref()),
        ProfileLoadOutcome::KeepPrevious => request.fallback_profile.as_ref(),
        ProfileLoadOutcome::Cleared => None,
    };
    // On a data-tab analysis failure the conflicts are cleared (matching the
    // old behavior). The error is NOT surfaced via `status_message` here: every
    // composite-load caller set its own status after the old synchronous
    // `reload_profile`, so the data-tab error was never visible on this path.
    // The standalone `load_data_tab_conflicts` (Data-tab open) still surfaces it.
    let (data_tab_conflicts, missing_store_mod_count) = match effective_profile {
        Some(profile) => compute_data_tab_conflicts(db.clone(), profile.clone())
            .await
            .unwrap_or_default(),
        None => (Vec::new(), 0),
    };

    // Fold in the tool reload when a game is in scope.
    let tools = match request.tool_request {
        Some(tool_request) => Some(load_tools_state(db, tool_request).await?),
        None => None,
    };

    Ok(ProfileContextSnapshot {
        profiles,
        active_profile,
        profile_outcome,
        data_tab_conflicts,
        missing_store_mod_count,
        tools,
        rerun_diagnostics: request.rerun_diagnostics,
    })
}
