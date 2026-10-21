//! Executable tool launch and overwrite capture commands.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use tracing::{info, warn};

use modde_core::db::{ExecutableConfigRow, ModdeDb};
use modde_core::fs::walk_files_relative;
use modde_core::paths;
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;

use crate::commands::load_profile_or_default;

struct ExternalToolRun {
    executable: PathBuf,
    args: Vec<String>,
    profile_name: Option<String>,
    game_id: Option<String>,
    working_dir: Option<PathBuf>,
    environment: HashMap<String, String>,
    wine_dll_overrides: Option<String>,
    output_mod: String,
}

/// Run an external tool and capture any new files it writes into the game directory
/// as an "Overwrite" mod.
pub async fn handle_run(
    executable: PathBuf,
    args: Vec<String>,
    profile_name: Option<String>,
    game_id: Option<String>,
) -> Result<()> {
    run_external_tool(ExternalToolRun {
        executable,
        args,
        profile_name,
        game_id,
        working_dir: None,
        environment: HashMap::new(),
        wine_dll_overrides: None,
        output_mod: "__overwrite__".to_string(),
    })
    .await
}

async fn run_external_tool(options: ExternalToolRun) -> Result<()> {
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database")?;
    let profile = load_profile_or_default(
        &pm,
        options.profile_name.as_deref(),
        options.game_id.as_deref(),
    )
    .await?;

    let game_plugin = modde_games::resolve_game_plugin(profile.game_id.as_str())
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{}'", profile.game_id))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    let mod_dir = game_plugin
        .mod_root(&install_dir)
        .context("failed to resolve game mod root")?;

    // Step 1: Snapshot current state of mod directory
    info!(mod_dir = %mod_dir.display(), "snapshotting mod directory before tool run");
    let before = snapshot_dir(&mod_dir)?;

    // Step 2: Run the tool
    let working_dir = options.working_dir.as_ref().unwrap_or(&install_dir);
    let mut command = Command::new(&options.executable);
    command.args(&options.args).current_dir(working_dir);
    for (key, value) in &options.environment {
        command.env(key, value);
    }
    if let Some(overrides) = &options.wine_dll_overrides {
        command.env("WINEDLLOVERRIDES", overrides);
    }

    println!(
        "Running: {} {}",
        options.executable.display(),
        options.args.join(" ")
    );
    println!("Working directory: {}", working_dir.display());
    if !options.environment.is_empty() || options.wine_dll_overrides.is_some() {
        println!("Environment overrides:");
        for key in options.environment.keys() {
            println!("  {key}=<configured>");
        }
        if let Some(overrides) = &options.wine_dll_overrides {
            println!("  WINEDLLOVERRIDES={overrides}");
        }
    }

    let status = command
        .status()
        .with_context(|| format!("failed to execute: {}", options.executable.display()))?;

    if !status.success() {
        warn!(
            exit_code = status.code(),
            "tool exited with non-zero status"
        );
    }

    // Step 3: Diff to find new files
    let after = snapshot_dir(&mod_dir)?;
    let new_files: Vec<String> = after.difference(&before).cloned().collect();

    if new_files.is_empty() {
        println!("Tool completed. No new files written to mod directory.");
        return Ok(());
    }

    // Step 4: Move new files to the configured output mod
    let overwrite_dir = paths::store_dir().join(&options.output_mod);
    std::fs::create_dir_all(&overwrite_dir)?;

    println!(
        "\nTool wrote {} new file(s). Moving to output mod '{}':",
        new_files.len(),
        options.output_mod
    );

    for rel_path in &new_files {
        let src = mod_dir.join(rel_path);
        let dst = overwrite_dir.join(rel_path);

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::rename(&src, &dst)
            .or_else(|_| {
                // Cross-device: copy + remove
                std::fs::copy(&src, &dst)?;
                std::fs::remove_file(&src)
            })
            .with_context(|| format!("failed to move {rel_path} to overwrite"))?;

        println!("  {rel_path}");
    }

    println!("\nOutput mod: {}", overwrite_dir.display());
    println!(
        "Add '{}' to your profile mod list to include these files in future deploys.",
        options.output_mod
    );

    Ok(())
}

/// Save a named executable launch target.
pub async fn handle_add_executable(
    name: &str,
    executable: PathBuf,
    game_id: &str,
    working_dir: Option<PathBuf>,
    output_mod: &str,
    wine_dll_overrides: Option<String>,
    environment: &[String],
    args: &[String],
) -> Result<()> {
    if modde_games::resolve_game_plugin(game_id).is_none() {
        anyhow::bail!(
            "unsupported game: '{game_id}'. Supported games: {}",
            modde_games::supported_game_ids().join(", ")
        );
    }
    if name.trim().is_empty() {
        anyhow::bail!("executable name cannot be empty");
    }
    if output_mod.trim().is_empty() {
        anyhow::bail!("output mod cannot be empty");
    }

    let env_map = crate::commands::parse_env(environment)?;
    let db = ModdeDb::open().await.context("failed to open database")?;
    let row = ExecutableConfigRow {
        game_id: game_id.to_string(),
        name: name.to_string(),
        executable_path: executable,
        arguments_json: serde_json::to_string(args)?,
        working_dir,
        environment_json: serde_json::to_string(&env_map)?,
        wine_dll_overrides,
        output_mod: output_mod.to_string(),
        enabled: true,
    };
    db.save_executable_config(&row).await?;

    println!("Saved executable '{}' for {game_id}", row.name);
    println!("  Path: {}", row.executable_path.display());
    println!("  Output mod: {}", row.output_mod);
    Ok(())
}

/// List saved executable launch targets for a game.
pub async fn handle_list_executables(game_id: &str) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let rows = db.load_executable_configs(&GameId::from(game_id)).await?;

    println!("Executables for {game_id}:");
    if rows.is_empty() {
        println!("  (none configured)");
        return Ok(());
    }

    for row in rows {
        let args: Vec<String> = serde_json::from_str(&row.arguments_json).unwrap_or_default();
        println!("  {}", row.name);
        println!("    path: {}", row.executable_path.display());
        if !args.is_empty() {
            println!("    args: {}", args.join(" "));
        }
        if let Some(working_dir) = row.working_dir {
            println!("    working dir: {}", working_dir.display());
        }
        if let Some(overrides) = row.wine_dll_overrides {
            println!("    WINEDLLOVERRIDES: {overrides}");
        }
        println!("    output mod: {}", row.output_mod);
    }

    Ok(())
}

/// Remove a saved executable launch target.
pub async fn handle_remove_executable(name: &str, game_id: &str) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    if db
        .delete_executable_config(&GameId::from(game_id), name)
        .await?
    {
        println!("Removed executable '{name}' for {game_id}");
    } else {
        anyhow::bail!("no executable named '{name}' is configured for {game_id}");
    }
    Ok(())
}

/// Run a saved executable launch target.
pub async fn handle_run_executable(
    name: &str,
    game_id: &str,
    profile_name: Option<String>,
    extra_args: Vec<String>,
) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let row = db
        .load_executable_config(&GameId::from(game_id), name)
        .await?
        .ok_or_else(|| {
            anyhow::anyhow!("no executable named '{name}' is configured for {game_id}")
        })?;
    if !row.enabled {
        anyhow::bail!("executable '{name}' is disabled for {game_id}");
    }

    let mut args: Vec<String> = serde_json::from_str(&row.arguments_json)
        .with_context(|| format!("stored arguments for '{name}' are invalid JSON"))?;
    args.extend(extra_args);
    let environment: HashMap<String, String> = serde_json::from_str(&row.environment_json)
        .with_context(|| format!("stored environment for '{name}' is invalid JSON"))?;

    run_external_tool(ExternalToolRun {
        executable: row.executable_path,
        args,
        profile_name,
        game_id: Some(game_id.to_string()),
        working_dir: row.working_dir,
        environment,
        wine_dll_overrides: row.wine_dll_overrides,
        output_mod: row.output_mod,
    })
    .await
}

/// List known tools for a game.
fn snapshot_dir(dir: &Path) -> Result<HashSet<String>> {
    if !dir.exists() {
        return Ok(HashSet::new());
    }

    let files = walk_files_relative(dir)
        .with_context(|| format!("failed to walk directory: {}", dir.display()))?;

    Ok(files.into_iter().map(|(rel, _)| rel).collect())
}
