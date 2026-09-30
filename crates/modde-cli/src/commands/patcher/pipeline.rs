//! Patcher pipeline execution and stage runtime.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use modde_core::profile::{Profile, ProfileManager};
use modde_core::{ModdeDb, PatcherStageRow, PatcherStageSettings, paths};

use super::fs_ops::{
    compute_next_manifest, copy_dir_recursive, install_managed_output, patcher_cache_key,
    patcher_time_id, remove_dir_if_exists, remove_rel_paths, require_profile_id, snapshot_dir,
    stage_generated_dir, write_load_order,
};
use super::process::{run_command_stage, run_synthesis_stage};

pub(super) async fn run_enabled_pipeline(
    pm: &ProfileManager,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    prefix: Option<&Path>,
) -> Result<usize> {
    let profile_id = match profile.id {
        Some(id) => id,
        None => return Ok(0),
    };
    let game_mod_dir = game_plugin
        .mod_root_at(install_dir, prefix)
        .context("failed to resolve game mod root for patcher pipeline")?;
    let stages = pm.db().list_patcher_stages(profile_id).await?;
    let enabled: Vec<PatcherStageRow> = stages.into_iter().filter(|stage| stage.enabled).collect();
    let original_manifests = load_stage_manifests(pm.db(), profile_id, &enabled).await?;
    let backup = backup_generated_outputs(profile, &enabled)?;

    reset_managed_outputs(
        pm.db(),
        profile,
        &enabled,
        game_plugin,
        install_dir,
        prefix,
        &game_mod_dir,
    )
    .await?;

    let mut manifests = original_manifests.clone();
    for stage in &enabled {
        if let Err(error) = run_stage(
            pm.db(),
            profile,
            game_plugin,
            install_dir,
            prefix,
            &game_mod_dir,
            stage,
            &mut manifests,
        )
        .await
        {
            restore_patcher_pipeline(
                pm.db(),
                profile,
                &enabled,
                game_plugin,
                install_dir,
                prefix,
                &game_mod_dir,
                &original_manifests,
                &backup,
            )
            .await?;
            return Err(error);
        }
    }
    remove_dir_if_exists(&backup.root)?;

    Ok(enabled.len())
}

#[derive(Debug)]
pub(super) struct PatcherPipelineBackup {
    root: PathBuf,
    stage_dirs: HashMap<String, PathBuf>,
}

pub(super) fn backup_generated_outputs(
    profile: &Profile,
    stages: &[PatcherStageRow],
) -> Result<PatcherPipelineBackup> {
    let root = paths::modde_cache_dir()
        .join("patchers")
        .join("backups")
        .join(format!("{}-{}", profile.name, patcher_time_id()));
    if root.exists() {
        fs::remove_dir_all(&root)?;
    }
    fs::create_dir_all(&root)?;
    let mut stage_dirs = HashMap::new();
    for stage in stages {
        let generated = stage_generated_dir(profile, &stage.name);
        if generated.exists() {
            let backup_dir = root.join(&stage.name);
            copy_dir_recursive(&generated, &backup_dir)?;
            stage_dirs.insert(stage.name.clone(), backup_dir);
        }
    }
    Ok(PatcherPipelineBackup { root, stage_dirs })
}

pub(super) async fn restore_patcher_pipeline(
    db: &ModdeDb,
    profile: &Profile,
    stages: &[PatcherStageRow],
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    prefix: Option<&Path>,
    game_mod_dir: &Path,
    manifests: &HashMap<String, Vec<String>>,
    backup: &PatcherPipelineBackup,
) -> Result<()> {
    for stage in stages {
        let generated = stage_generated_dir(profile, &stage.name);
        if generated.exists() {
            fs::remove_dir_all(&generated)?;
        }
        if let Some(backup_dir) = backup.stage_dirs.get(&stage.name) {
            copy_dir_recursive(backup_dir, &generated)?;
        }
        db.replace_patcher_stage_outputs(
            require_profile_id(profile)?,
            &stage.name,
            manifests
                .get(&stage.name)
                .map(Vec::as_slice)
                .unwrap_or_default(),
        )
        .await?;
    }
    reset_managed_outputs(
        db,
        profile,
        stages,
        game_plugin,
        install_dir,
        prefix,
        game_mod_dir,
    )
    .await?;
    remove_dir_if_exists(&backup.root)
}

pub(super) async fn load_stage_manifests(
    db: &ModdeDb,
    profile_id: i64,
    stages: &[PatcherStageRow],
) -> Result<HashMap<String, Vec<String>>> {
    let mut manifests = HashMap::new();
    for stage in stages {
        let rows = db
            .list_patcher_stage_outputs(profile_id, &stage.name)
            .await?;
        manifests.insert(
            stage.name.clone(),
            rows.into_iter().map(|row| row.rel_path).collect(),
        );
    }
    Ok(manifests)
}

pub(super) async fn reset_managed_outputs(
    db: &ModdeDb,
    profile: &Profile,
    stages: &[PatcherStageRow],
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    prefix: Option<&Path>,
    game_mod_dir: &Path,
) -> Result<()> {
    if let Some(profile_id) = profile.id {
        let existing = db.list_all_patcher_stage_outputs(profile_id).await?;
        let mut rel_paths = existing
            .into_iter()
            .map(|row| row.rel_path)
            .collect::<HashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        rel_paths.sort();
        remove_rel_paths(game_mod_dir, &rel_paths)?;
    }

    for stage in stages {
        let generated = stage_generated_dir(profile, &stage.name);
        if generated.exists() {
            game_plugin
                .deploy_to_install_at(&generated, install_dir, prefix)
                .with_context(|| {
                    format!(
                        "failed to project managed patcher output for stage '{}'",
                        stage.name
                    )
                })?;
        }
    }
    Ok(())
}

pub(super) async fn run_stage(
    db: &ModdeDb,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    prefix: Option<&Path>,
    game_mod_dir: &Path,
    stage: &PatcherStageRow,
    manifests: &mut HashMap<String, Vec<String>>,
) -> Result<()> {
    validate_stage_definition(stage)?;
    validate_stage_runtime(stage, profile, game_plugin)?;

    let before = snapshot_dir(game_mod_dir).await?;
    let execution = execute_stage(profile, install_dir, game_mod_dir, stage).await?;
    if execution.skipped {
        println!("Skipped patcher stage '{}' (cache hit)", stage.name);
        return Ok(());
    }
    let after = snapshot_dir(game_mod_dir).await?;

    let own_before = manifests
        .get(&stage.name)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .collect::<HashSet<_>>();
    let other_owned = manifests
        .iter()
        .filter(|(stage_name, _)| *stage_name != &stage.name)
        .flat_map(|(_, rel_paths)| rel_paths.iter().cloned())
        .collect::<HashSet<_>>();

    let next_manifest = compute_next_manifest(&before, &after, &own_before, &other_owned, stage)?;
    install_managed_output(
        db,
        profile,
        game_plugin,
        install_dir,
        prefix,
        game_mod_dir,
        stage,
        &own_before,
        &next_manifest,
        &execution.cache_key,
    )
    .await?;
    manifests.insert(stage.name.clone(), next_manifest);
    Ok(())
}

pub(super) fn validate_synthesis_game(profile: &Profile) -> Result<()> {
    let game_plugin = modde_games::resolve_game_plugin(profile.game_id.as_str())
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{}'", profile.game_id))?;
    if !game_plugin.has_plugin_system() {
        anyhow::bail!(
            "synthesis-cli patchers are only supported on Bethesda plugin-order games in v1; '{}' does not expose plugins.txt-style load order",
            profile.game_id
        );
    }
    Ok(())
}

pub(super) fn validate_stage_definition(stage: &PatcherStageRow) -> Result<()> {
    match &stage.settings {
        PatcherStageSettings::SynthesisCli(settings) => {
            validate_file(
                &settings.executable,
                "Synthesis CLI executable",
                "Synthesis release zip",
                "download and unpack Synthesis.zip outside the game/Data and modde store directories",
                format!("{} --help", settings.executable.display()),
            )?;
            validate_file(
                &settings.pipeline_settings,
                "Synthesis pipeline settings",
                "Synthesis UI or Synthesis CLI profile export",
                "create or export PipelineSettings.json with Synthesis",
                format!("test -r {}", settings.pipeline_settings.display()),
            )?;
            if settings.synthesis_profile.trim().is_empty() {
                anyhow::bail!(
                    "patcher stage '{}' is missing synthesis_profile",
                    stage.name
                );
            }
        }
        PatcherStageSettings::Command(settings) => {
            validate_file(
                &settings.executable,
                "patcher command executable",
                "the user-authored patcher package or script",
                "install/build the command and update this stage with modde patcher add-command",
                format!("{} --help", settings.executable.display()),
            )?;
        }
        PatcherStageSettings::RustNative => {
            anyhow::bail!(
                "patcher stage '{}' uses rust-native, but rust-native patchers are reserved for a future release",
                stage.name
            );
        }
    }
    Ok(())
}

pub(super) fn validate_stage_runtime(
    stage: &PatcherStageRow,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
) -> Result<()> {
    if matches!(stage.settings, PatcherStageSettings::SynthesisCli(_))
        && !game_plugin.has_plugin_system()
    {
        anyhow::bail!(
            "patcher stage '{}' uses synthesis-cli, but '{}' does not expose a Bethesda-style plugin load order",
            stage.name,
            profile.game_id
        );
    }
    Ok(())
}

pub(super) fn validate_file(
    path: &Path,
    label: &str,
    producer: &str,
    fix: &str,
    validation: String,
) -> Result<()> {
    if path.is_file() {
        return Ok(());
    }
    anyhow::bail!(
        "missing required {label}: {}. Why required: enabled patcher stages are deployment artifacts and must be reproducible. Upstream producer: {producer}. Regenerate/fix by: {fix}. Validate with: {validation}",
        path.display()
    )
}

pub(super) async fn execute_stage(
    profile: &Profile,
    install_dir: &Path,
    game_mod_dir: &Path,
    stage: &PatcherStageRow,
) -> Result<StageExecution> {
    let work_dir = paths::modde_cache_dir()
        .join("patchers")
        .join(profile.game_id.as_str())
        .join(&profile.name)
        .join(&stage.name);
    fs::create_dir_all(&work_dir)?;
    let load_order_path = work_dir.join("load-order.txt");
    write_load_order(profile, &load_order_path).await?;
    let load_order = fs::read_to_string(&load_order_path)?;
    let cache_key = patcher_cache_key(stage, &load_order)?;
    let generated = stage_generated_dir(profile, &stage.name);
    if stage.last_cache_key.as_deref() == Some(cache_key.as_str()) && generated.exists() {
        return Ok(StageExecution {
            cache_key,
            skipped: true,
        });
    }
    if work_dir.exists() {
        fs::remove_dir_all(&work_dir)?;
    }
    fs::create_dir_all(&work_dir)?;
    fs::write(&load_order_path, load_order)?;

    match &stage.settings {
        PatcherStageSettings::SynthesisCli(settings) => {
            run_synthesis_stage(
                settings,
                game_mod_dir,
                &load_order_path,
                install_dir,
                &stage.name,
                &work_dir,
                stage.timeout_seconds,
            )?;
        }
        PatcherStageSettings::Command(settings) => {
            run_command_stage(
                settings,
                game_mod_dir,
                &load_order_path,
                install_dir,
                profile,
                &stage.name,
                &work_dir,
                stage.timeout_seconds,
            )?;
        }
        PatcherStageSettings::RustNative => unreachable!("validated before execution"),
    }
    Ok(StageExecution {
        cache_key,
        skipped: false,
    })
}

#[derive(Debug, Clone)]
pub(super) struct StageExecution {
    cache_key: String,
    skipped: bool,
}
