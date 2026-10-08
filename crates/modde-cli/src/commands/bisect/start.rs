//! Bisect session start and source-profile analysis.

use std::collections::HashMap;
#[cfg(feature = "bethesda")]
use std::path::Path;
use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::{BisectOracle, BisectSaveSafety, GameId, NewBisectSession};

use crate::commands::crash;

use super::BisectOracleArg;
use super::perf::time_id;

pub async fn handle_start(
    game: String,
    profile_name: String,
    oracle: BisectOracleArg,
    baseline_run: Option<String>,
    perf_p99_frame_time_percent: u16,
    perf_one_percent_low_fps_percent: u16,
    perf_alpha_micros: u32,
    perf_min_samples: usize,
    crash_dir: Option<PathBuf>,
    force_save_risk: bool,
    keep_profiles: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let game_id = GameId::from(game.as_str());
    let profile = pm.load(&profile_name, Some(&game_id)).await?;
    if matches!(profile.source, ProfileSource::Wabbajack { .. }) {
        anyhow::bail!(
            "modde bisect cannot safely operate on Wabbajack-source profile '{profile_name}' yet. \
             Why required: Wabbajack deploy currently materializes staging directly and ignores per-mod enabled flags. \
             Upstream producer: Wabbajack deployment profile model. \
             Regenerate/fix by: add enabled-aware Wabbajack candidate deployment or import the list into a normal resolver-backed profile. \
             Validate with: modde deploy --profile <candidate> --game {game}"
        );
    }
    let profile_id = profile
        .id
        .ok_or_else(|| anyhow::anyhow!("profile '{profile_name}' is not stored in the database"))?;
    let suspect_mod_ids = profile
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.mod_id.clone())
        .collect::<Vec<_>>();
    if suspect_mod_ids.len() < 2 {
        anyhow::bail!("bisect requires at least two enabled mods");
    }

    let context = modde_games::library::context::for_game(
        &modde_core::settings::AppSettings::load(),
        &game,
        pm.db(),
    )
    .await?;
    let oracle = match oracle {
        BisectOracleArg::Manual => BisectOracle::Manual,
        BisectOracleArg::Crash => {
            let crash_dir = match crash_dir {
                Some(path) => path,
                None => crash::first_existing_crash_log_dir(&game)?,
            };
            if !crash_dir.is_dir() {
                anyhow::bail!("crash directory does not exist: {}", crash_dir.display());
            }
            BisectOracle::Crash { crash_dir }
        }
        BisectOracleArg::Perf => {
            let baseline_run = baseline_run
                .ok_or_else(|| anyhow::anyhow!("--baseline-run is required for perf oracle"))?;
            let baseline = pm.db().load_performance_run(&baseline_run).await?;
            if baseline.status != "complete" {
                anyhow::bail!(
                    "baseline performance run '{baseline_run}' is not complete (status: {})",
                    baseline.status
                );
            }
            if baseline.game_id != game_id {
                anyhow::bail!(
                    "baseline performance run '{baseline_run}' belongs to '{}' not '{game}'",
                    baseline.game_id
                );
            }
            anyhow::ensure!(
                baseline.exit_status == Some(0),
                "baseline must have a successful observed game exit"
            );
            super::perf::require_baseline_profile(&baseline, &profile)?;
            crate::commands::perf::require_baseline_configuration(&baseline_run, &context)?;
            BisectOracle::Perf {
                baseline_run,
                p99_frame_time_percent: perf_p99_frame_time_percent,
                one_percent_low_fps_percent: perf_one_percent_low_fps_percent,
                alpha_micros: perf_alpha_micros,
                min_samples: perf_min_samples,
            }
        }
    };

    let session_id = format!("b{}", time_id());
    modde_core::library::atomic_json(
        &super::candidate::pin_path(&session_id),
        &super::candidate::InstallationPin {
            installation: context.game.id,
            save_scope: context.saves.scope,
        },
    )?;
    pm.db()
        .create_bisect_session(&NewBisectSession {
            session_id: session_id.clone(),
            game_id,
            source_profile_id: profile_id,
            source_profile_name: profile_name.clone(),
            oracle,
            suspect_mod_ids,
            save_safety: if force_save_risk {
                BisectSaveSafety::Force
            } else {
                BisectSaveSafety::Refuse
            },
            keep_profiles,
        })
        .await?;

    println!("Started bisect session: {session_id}");
    println!("Source profile: {profile_name} ({game})");
    println!("Run the first candidate with: modde bisect run {session_id}");
    Ok(())
}

pub(super) async fn bisect_dependency_map(
    _pm: &ProfileManager,
    source: &Profile,
) -> Result<HashMap<String, Vec<String>>> {
    let mut dependencies: HashMap<String, Vec<String>> = HashMap::new();
    for rule in &source.load_order_rules {
        match rule {
            modde_core::resolver::LoadOrderRule::LoadAfter { mod_id, after } => {
                dependencies
                    .entry(mod_id.to_string())
                    .or_default()
                    .push(after.to_string());
            }
            modde_core::resolver::LoadOrderRule::LoadBefore { mod_id, before } => {
                dependencies
                    .entry(before.to_string())
                    .or_default()
                    .push(mod_id.to_string());
            }
            modde_core::resolver::LoadOrderRule::Incompatible { .. } => {}
        }
    }

    if !matches!(
        source.game_id.as_str(),
        "skyrim-se" | "skyrim-ae" | "fallout4" | "fallout76" | "starfield"
    ) {
        return Ok(dependencies);
    }

    #[cfg(not(feature = "bethesda"))]
    anyhow::bail!("Bethesda bisect dependency analysis requires the bethesda feature");

    #[cfg(feature = "bethesda")]
    {
        let Some(profile_id) = source.id else {
            return Ok(dependencies);
        };
        let installed_files = _pm.db().installed_files_for_profile(profile_id).await?;
        let mut plugin_owner_by_lower: HashMap<String, String> = HashMap::new();
        let mut plugin_path_by_lower: HashMap<String, PathBuf> = HashMap::new();
        for (mod_id, file) in installed_files {
            let rel_path = Path::new(&file.rel_path);
            let Some(file_name) = rel_path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if is_bethesda_plugin_name(file_name) {
                let lower = file_name.to_ascii_lowercase();
                plugin_owner_by_lower.insert(lower.clone(), mod_id);
                plugin_path_by_lower.insert(lower, PathBuf::from(file.rel_path));
            }
        }
        for enabled_mod in source.mods.iter().filter(|m| m.enabled) {
            if let Some(plugin_name) = enabled_mod.mod_id.strip_prefix("plugin/")
                && is_bethesda_plugin_name(plugin_name)
            {
                plugin_owner_by_lower
                    .entry(plugin_name.to_ascii_lowercase())
                    .or_insert_with(|| enabled_mod.mod_id.clone());
                plugin_path_by_lower
                    .entry(plugin_name.to_ascii_lowercase())
                    .or_insert_with(|| PathBuf::from(plugin_name));
            }
        }

        let staging = ProfileManager::staging_dir(&source.name);
        for (plugin_lower, mod_id) in plugin_owner_by_lower.clone() {
            let Some(rel_path) = plugin_path_by_lower.get(&plugin_lower) else {
                continue;
            };
            let plugin_path = staging.join(rel_path);
            let header = modde_games::bethesda::plugin_header::parse_plugin_header(&plugin_path)
                .with_context(|| {
                    format!("failed to parse plugin header {}", plugin_path.display())
                });
            let Ok(header) = header else {
                continue;
            };
            for master in header.masters {
                if let Some(master_owner) = plugin_owner_by_lower.get(&master.to_ascii_lowercase())
                    && master_owner != &mod_id
                {
                    dependencies
                        .entry(mod_id.clone())
                        .or_default()
                        .push(master_owner.clone());
                }
            }
        }

        for deps in dependencies.values_mut() {
            deps.sort();
            deps.dedup();
        }
        Ok(dependencies)
    }
}

#[cfg(feature = "bethesda")]
pub(super) fn is_bethesda_plugin_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".esp") || lower.ends_with(".esm") || lower.ends_with(".esl")
}
