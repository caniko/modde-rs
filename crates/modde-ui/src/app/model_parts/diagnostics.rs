#![allow(clippy::wildcard_imports)]
//! diagnostics model helpers.

use super::*;

/// Compute the save fingerprint for a profile (lifted verbatim from the old
/// `reload_profile`). `None` when the game doesn't support save profiles.
pub(super) fn compute_save_fingerprint(
    profile: &modde_core::Profile,
) -> Option<modde_core::save::SaveFingerprint> {
    let game_id = profile.game_id.as_str();
    let staging_dir = ProfileManager::staging_dir(&profile.name);
    modde_games::resolve_game_plugin(game_id)
        .filter(|plugin| plugin.supports_save_profiles())
        .map(|plugin| {
            modde_core::save::SaveFingerprint::compute(&profile.mods, |mod_id| {
                let mod_path = staging_dir.join(mod_id);
                plugin.classify_mod(&mod_path).affects_saves()
            })
        })
}

/// Compute data-tab conflict rows + missing-store count for a profile. Shared
/// by [`load_profile_context`] and [`load_data_tab_conflicts`].
async fn load_hidden_files(
    db: &modde_core::db::ModdeDb,
    profile: &modde_core::Profile,
) -> std::collections::HashSet<(String, String)> {
    match profile.id {
        Some(profile_id) => db
            .list_hidden_files(profile_id)
            .await
            .ok()
            .map(|rows| {
                rows.into_iter()
                    .map(|row| (row.mod_id, row.rel_path))
                    .collect()
            })
            .unwrap_or_default(),
        None => std::collections::HashSet::new(),
    }
}

pub(super) async fn compute_data_tab_conflicts(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<(Vec<(String, Vec<String>)>, usize), String> {
    let hidden = load_hidden_files(&db, &profile).await;
    tokio::task::spawn_blocking(move || compute_data_tab_conflicts_blocking(profile, hidden))
        .await
        .map_err(|err| err.to_string())?
}

fn compute_data_tab_conflicts_blocking(
    profile: modde_core::Profile,
    hidden: std::collections::HashSet<(String, String)>,
) -> Result<(Vec<(String, Vec<String>)>, usize), String> {
    let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());
    match modde_core::diagnostics::analyze_profile_state(
        &profile,
        &modde_core::paths::store_dir(),
        &hidden,
        classifier.as_deref(),
    ) {
        Ok(analysis) => Ok((
            build_conflict_rows(&analysis, &hidden),
            analysis.missing_store_mods.len(),
        )),
        Err(err) => Err(format!("Failed to load data tab: {err}")),
    }
}

/// Off-thread lightweight data-tab conflict refresh (fired when the Data tab is
/// opened).
pub(crate) async fn load_data_tab_conflicts(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<DataTabConflicts, String> {
    compute_data_tab_conflicts(db, profile)
        .await
        .map(|(conflicts, missing_store_mod_count)| DataTabConflicts {
            conflicts,
            missing_store_mod_count,
        })
}

pub(crate) async fn load_diagnostics(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
) -> Result<DiagnosticsComputed, String> {
    tokio::task::spawn_blocking(move || load_diagnostics_blocking(db, profile, None))
        .await
        .map_err(|err| err.to_string())?
}

pub(crate) async fn load_diagnostics_with_crash(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
    crash_log_path: PathBuf,
) -> Result<DiagnosticsComputed, String> {
    let profile_id = profile
        .id
        .ok_or_else(|| format!("Profile '{}' is not stored in the database", profile.name))?;
    let raw_log = std::fs::read_to_string(&crash_log_path).map_err(|err| {
        format!(
            "Failed to read crash log {}: {err}",
            crash_log_path.display()
        )
    })?;
    let installed_files = db
        .installed_files_for_profile(profile_id)
        .await
        .map_err(|err| err.to_string())?;
    let tool_files = db
        .load_all_applied_files(&profile.game_id)
        .await
        .map_err(|err| err.to_string())?;
    let raw_log_for_record = raw_log.clone();
    let db_for_blocking = db.clone();
    let computed = tokio::task::spawn_blocking(move || {
        load_diagnostics_blocking(
            db_for_blocking,
            profile,
            Some(CrashLogInput {
                path: crash_log_path,
                raw_log,
                installed_files,
                tool_files,
            }),
        )
    })
    .await
    .map_err(|err| err.to_string())??;
    if let Some(report) = &computed.report.crash_report {
        db.record_crash_log(Some(profile_id), report, &raw_log_for_record)
            .await
            .map_err(|err| err.to_string())?;
    }
    Ok(computed)
}

struct CrashLogInput {
    path: PathBuf,
    raw_log: String,
    installed_files: Vec<(String, StagedFile)>,
    tool_files: Vec<String>,
}

fn load_diagnostics_blocking(
    db: modde_core::db::ModdeDb,
    profile: modde_core::Profile,
    crash_log: Option<CrashLogInput>,
) -> Result<DiagnosticsComputed, String> {
    let pm = ProfileManager::with_db(db);
    let hidden = load_hidden_files_blocking(&pm, &profile);
    let active_plugins = load_active_plugins_blocking(&pm, &profile);
    let integrity = Modde::verify_staging_integrity(&ProfileManager::staging_dir(&profile.name));
    let engine = match profile.game_id.as_str() {
        #[cfg(feature = "bethesda")]
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" => {
            modde_games::bethesda::diagnostics::bethesda_diagnostics()
        }
        _ => modde_core::diagnostics::base_diagnostics(),
    };
    let classifier = modde_games::resolve_collision_classifier(profile.game_id.as_str());
    let (diagnostics, analysis) = modde_core::diagnostics::run_profile_diagnostics(
        profile.game_id.as_str(),
        &profile,
        &active_plugins,
        &modde_core::paths::store_dir(),
        &ProfileManager::staging_dir(&profile.name),
        &hidden,
        classifier.as_deref(),
        &engine,
    )
    .map_err(|err| err.to_string())?;
    let crash_report = if let Some(input) = crash_log {
        Some(analyze_crash_log_blocking(
            &profile,
            &active_plugins,
            input,
        )?)
    } else {
        None
    };
    let entries = diagnostics.iter().map(format_diagnostic_entry).collect();
    Ok(DiagnosticsComputed {
        report: crate::views::diagnostics::DiagnosticsReport {
            profile_name: profile.name.clone(),
            game_id: profile.game_id.to_string(),
            entries,
            integrity,
            crash_report,
        },
        data_tab_conflicts: build_conflict_rows(&analysis, &hidden),
        missing_store_mod_count: analysis.missing_store_mods.len(),
    })
}

fn analyze_crash_log_blocking(
    profile: &modde_core::Profile,
    active_plugins: &[String],
    input: CrashLogInput,
) -> Result<modde_core::crash::CrashCorrelationReport, String> {
    let report = modde_core::crash::correlate_crash_log(
        &input.path,
        &input.raw_log,
        modde_core::crash::CrashLogFormat::Auto,
        modde_core::crash::CrashCorrelationInput {
            game_id: profile.game_id.to_string(),
            profile: profile.clone(),
            active_plugins: active_plugins
                .iter()
                .enumerate()
                .map(|(idx, plugin_name)| modde_core::PluginEntry {
                    plugin_name: plugin_name.clone(),
                    sort_index: idx as i64,
                    enabled: true,
                })
                .collect(),
            installed_files: input.installed_files,
            tool_files: input.tool_files,
        },
    );
    Ok(report)
}
