//! Patcher command handlers.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use modde_core::profile::{Profile, ProfileManager};
use modde_core::{CommandSettings, PatcherStageRow, PatcherStageSettings, SynthesisCliSettings};

use crate::commands::load_profile_or_default;

use super::fs_ops::{parse_env, require_profile_id, stage_generated_dir};
use super::pipeline::{
    load_stage_manifests, run_enabled_pipeline, run_stage, validate_stage_definition,
    validate_stage_runtime, validate_synthesis_game,
};

pub async fn handle_list(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let profile_id = require_profile_id(&profile)?;
    let stages = pm.db().list_patcher_stages(profile_id).await?;

    if stages.is_empty() {
        println!("No patcher stages configured for {}.", profile.name);
        return Ok(());
    }

    println!("Patcher stages for {} ({}):", profile.name, profile.game_id);
    for stage in stages {
        println!(
            "  {} [{}] {} order={} -> {}",
            stage.name,
            stage.stage_kind.as_str(),
            if stage.enabled { "enabled" } else { "disabled" },
            stage.sort_index,
            stage.output_mod
        );
    }
    Ok(())
}

pub async fn handle_add_synthesis(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
    executable: PathBuf,
    pipeline_settings: PathBuf,
    synthesis_profile: String,
    output_mod: String,
    order: i64,
    timeout_seconds: u64,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    validate_synthesis_game(&profile)?;
    let stage = PatcherStageRow::new(
        require_profile_id(&profile)?,
        name,
        true,
        order,
        PatcherStageSettings::SynthesisCli(SynthesisCliSettings {
            executable,
            pipeline_settings,
            synthesis_profile,
        }),
        output_mod,
    )?
    .with_timeout_seconds(timeout_seconds);
    validate_stage_definition(&stage)?;
    pm.db().save_patcher_stage(&stage).await?;
    println!(
        "Saved Synthesis patcher stage '{}' for {} at order {}",
        stage.name, profile.name, stage.sort_index
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_add_command(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
    executable: PathBuf,
    working_dir: Option<PathBuf>,
    args: Vec<String>,
    environment: Vec<String>,
    output_mod: String,
    order: i64,
    timeout_seconds: u64,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let stage = PatcherStageRow::new(
        require_profile_id(&profile)?,
        name,
        true,
        order,
        PatcherStageSettings::Command(CommandSettings {
            executable,
            args,
            environment: parse_env(environment)?,
            working_dir,
        }),
        output_mod,
    )?
    .with_timeout_seconds(timeout_seconds);
    validate_stage_definition(&stage)?;
    pm.db().save_patcher_stage(&stage).await?;
    println!(
        "Saved command patcher stage '{}' for {} at order {}",
        stage.name, profile.name, stage.sort_index
    );
    Ok(())
}

pub async fn handle_remove(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let profile_id = require_profile_id(&profile)?;
    let removed = pm.db().delete_patcher_stage(profile_id, name).await?;
    if !removed {
        anyhow::bail!(
            "no patcher stage named '{name}' is configured for {}",
            profile.name
        );
    }

    pm.db()
        .replace_patcher_stage_outputs(profile_id, name, &[])
        .await?;
    let generated = stage_generated_dir(&profile, name);
    if generated.exists() {
        fs::remove_dir_all(&generated)?;
    }
    println!("Removed patcher stage '{name}' from {}", profile.name);
    Ok(())
}

pub async fn handle_set_enabled(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
    enabled: bool,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let changed = pm
        .db()
        .set_patcher_stage_enabled(require_profile_id(&profile)?, name, enabled)
        .await?;
    if !changed {
        anyhow::bail!(
            "no patcher stage named '{name}' is configured for {}",
            profile.name
        );
    }
    println!(
        "{} patcher stage '{name}' for {}",
        if enabled { "Enabled" } else { "Disabled" },
        profile.name
    );
    Ok(())
}

pub async fn handle_reorder(
    profile_name: Option<String>,
    game_id: Option<String>,
    names: Vec<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    pm.db()
        .reorder_patcher_stages(require_profile_id(&profile)?, &names)
        .await?;
    println!("Updated patcher stage order for {}", profile.name);
    Ok(())
}

pub async fn handle_validate(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;
    let stages = pm
        .db()
        .list_patcher_stages(require_profile_id(&profile)?)
        .await?;
    for stage in stages.iter().filter(|stage| stage.enabled) {
        validate_stage_definition(stage)?;
        validate_stage_runtime(stage, &profile, game_plugin)?;
    }
    println!(
        "Validated {} enabled patcher stage(s) for {}",
        stages.iter().filter(|stage| stage.enabled).count(),
        profile.name
    );
    Ok(())
}

pub async fn handle_run_stage(
    name: &str,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;
    let context = crate::commands::installation_context(profile.game_id.as_str(), &pm).await?;
    let install_dir = context.game.install_path.context("installation missing")?;
    let profile_id = require_profile_id(&profile)?;
    let stage = pm
        .db()
        .load_patcher_stage(profile_id, name)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!(
                "no patcher stage named '{name}' is configured for {}",
                profile.name
            )
        })?;
    let game_mod_dir = game_plugin
        .mod_root_at(&install_dir, context.prefix.as_deref())
        .context("failed to resolve game mod root for patcher pipeline")?;
    let stages = pm.db().list_patcher_stages(profile_id).await?;
    let mut manifests = load_stage_manifests(pm.db(), profile_id, &stages).await?;
    run_stage(
        pm.db(),
        &profile,
        game_plugin,
        &install_dir,
        context.prefix.as_deref(),
        &game_mod_dir,
        &stage,
        &mut manifests,
    )
    .await?;
    println!("Ran patcher stage '{name}' for {}", profile.name);
    Ok(())
}

pub async fn handle_run(profile_name: Option<String>, game_id: Option<String>) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(&pm, profile_name.as_deref(), game_id.as_deref()).await?;
    let game_plugin =
        modde_games::resolve_game_plugin(profile.game_id.as_str()).ok_or_else(|| {
            anyhow::anyhow!(
                "unsupported game: '{}'\nSupported games: {}",
                profile.game_id,
                modde_games::supported_game_ids().join(", ")
            )
        })?;
    let context = crate::commands::installation_context(profile.game_id.as_str(), &pm).await?;
    let install_dir = context.game.install_path.context("installation missing")?;

    let ran = run_enabled_pipeline(&pm, &profile, game_plugin, &install_dir, context.prefix.as_deref()).await?;
    println!("Ran {ran} patcher stage(s) for {}", profile.name);
    Ok(())
}

pub async fn run_enabled_for_deploy_at(
    pm: &ProfileManager,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    prefix: Option<&Path>,
) -> Result<usize> {
    run_enabled_pipeline(pm, profile, game_plugin, install_dir, prefix).await
}
