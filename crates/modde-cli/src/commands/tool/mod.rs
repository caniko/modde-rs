use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use modde_core::db::ModdeDb;
use modde_core::resolver::GameId;
use modde_games::tools::{GameTool, ToolConfig, ToolGameContext, ToolSettingKind, ToolSettingSpec};

mod exec;
mod releases;

pub use exec::{
    handle_add_executable, handle_list_executables, handle_remove_executable, handle_run,
    handle_run_executable,
};
pub use releases::{handle_install_release, handle_install_release_from_path, handle_releases};

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
pub struct ToolSetupOptions {
    pub tool_id: String,
    pub game: String,
    pub profile: Option<String>,
    pub source: String,
    pub release_tag: Option<String>,
    pub release_asset: Option<String>,
    pub local_source_dir: Option<PathBuf>,
    pub hardware_tuning: String,
    pub upgrade: bool,
    pub apply: bool,
    pub dry_run: bool,
    #[allow(dead_code)]
    pub yes: bool,
}

struct LoadedTool<'a> {
    tool: &'a dyn GameTool,
    game_plugin: &'a dyn modde_games::GamePlugin,
    install_dir: PathBuf,
    context: ToolGameContext,
    config: ToolConfig,
}

pub async fn handle_list(game_id: &str) -> Result<()> {
    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install directory for {}",
            game_plugin.display_name()
        )
    })?;

    println!("Game: {} ({})", game_plugin.display_name(), game_id);
    println!("Install: {}", install_dir.display());
    println!("\nDetected tools:");

    // Scan for common tool executables
    let common_tools = [
        ("xEdit", &["SSEEdit.exe", "FO4Edit.exe", "xEdit.exe"][..]),
        ("FNIS", &["GenerateFNISforUsers.exe"]),
        ("Nemesis", &["Nemesis Unlimited Behavior Engine.exe"]),
        ("BodySlide", &["BodySlide.exe", "BodySlide x64.exe"]),
        ("Creation Kit", &["CreationKit.exe"]),
        ("LOOT", &["LOOT.exe"]),
        ("zEdit", &["zEdit.exe"]),
    ];

    let mut found = false;
    for (name, executables) in &common_tools {
        for exe in *executables {
            let path = install_dir.join(exe);
            if path.exists() {
                println!("  {name}: {}", path.display());
                found = true;
            }
        }
    }

    if !found {
        println!("  (none detected)");
    }

    println!("\nRun tools with: modde tool run <executable> [-- args...]");

    Ok(())
}

// ── Gaming tool/overlay management ──────────────────────────────────────

/// Show status of all gaming tools for a game.
pub async fn handle_status(game_id: &str) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let stored = db.load_tool_configs(&GameId::from(game_id)).await?;
    let game_plugin = modde_games::resolve_game_plugin(game_id);
    let install_dir = game_plugin.and_then(modde_games::GamePlugin::detect_install);

    println!("Game: {game_id}\n");
    println!(
        "{:<14} {:<14} {:<10} Available",
        "Tool", "Category", "Status"
    );
    println!("{}", "-".repeat(60));

    for tool in modde_games::tools::all_tools() {
        let avail = tool.detect_available();
        let avail_str = match &avail {
            modde_games::tools::ToolAvailability::Available { version } => match version {
                Some(v) => format!("yes ({v})"),
                None => "yes".into(),
            },
            modde_games::tools::ToolAvailability::NotInstalled { .. } => "not installed".into(),
        };

        let enabled = stored
            .iter()
            .find(|r| r.tool_id == tool.tool_id())
            .is_some_and(|r| r.enabled);

        let status = if enabled { "enabled" } else { "disabled" };

        // Check if files are applied
        let applied_count = db
            .load_applied_files(&GameId::from(game_id), tool.tool_id())
            .await
            .map_or(0, |f| f.len());

        let status_str = if applied_count > 0 {
            format!("{status} ({applied_count} files)")
        } else {
            status.to_string()
        };

        println!(
            "{:<14} {:<14} {:<10} {}",
            tool.display_name(),
            tool.category(),
            status_str,
            avail_str,
        );

        if tool.tool_id() == "optiscaler" {
            let context = install_dir.as_ref().map(|install_dir| {
                modde_games::tools::ToolGameContext::from_parts(
                    game_id,
                    game_plugin
                        .map(modde_games::GamePlugin::display_name)
                        .unwrap_or(game_id),
                    Some(install_dir.clone()),
                    None,
                )
            });
            let config = stored
                .iter()
                .find(|r| r.tool_id == "optiscaler")
                .map_or_else(
                    || tool.default_config_for(context.as_ref()),
                    |row| modde_games::tools::ToolConfig {
                        tool_id: row.tool_id.clone(),
                        enabled: row.enabled,
                        settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
                    },
                );
            let managed = modde_games::tools::optiscaler::managed_paths_from_config(&config);
            if let Some(install_dir) = &install_dir
                && let Ok(state) = modde_games::tools::optiscaler::scan_optiscaler_install(
                    game_id,
                    install_dir,
                    &managed,
                )
            {
                println!("  OptiScaler install: {}", state.summary());
                println!("  Executable dir: {}", state.executable_dir.display());
                if let Some(path) = &state.config_path {
                    println!(
                        "  Config: {} ({} parsed setting(s))",
                        path.display(),
                        state.ini_settings.len()
                    );
                }
                if !state.wine_dll_overrides.is_empty() {
                    println!("  Wine overrides: {}", state.wine_dll_overrides.join(", "));
                }
                if let Some(path) = &state.latest_backup {
                    println!("  Latest backup: {}", path.display());
                }
            }
        }
    }

    Ok(())
}

async fn load_tool_for_game<'a>(
    db: &ModdeDb,
    tool_id: &str,
    game_id: &'a str,
) -> Result<LoadedTool<'a>> {
    let tool = modde_games::tools::resolve_tool(tool_id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown tool: '{tool_id}'\nAvailable: {}",
            modde_games::tools::all_tools()
                .iter()
                .map(|tool| tool.tool_id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;
    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;
    let context = ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(Some(&context)),
    };
    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_game_defaults(&mut config, Some(&context));
    }

    Ok(LoadedTool {
        tool,
        game_plugin,
        install_dir,
        context,
        config,
    })
}

async fn save_tool_config(
    db: &ModdeDb,
    game_id: &str,
    tool_id: &str,
    config: &ToolConfig,
) -> Result<()> {
    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(
        &GameId::from(game_id),
        tool_id,
        config.enabled,
        &settings_json,
    )
    .await?;
    Ok(())
}

pub async fn handle_show(tool_id: &str, game_id: &str, json: bool) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let loaded = load_tool_for_game(&db, tool_id, game_id).await?;
    let mut effective = loaded.config.clone();
    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_hardware_defaults(&mut effective);
    }
    let env = loaded
        .tool
        .env_vars_for(Some(&loaded.context), &loaded.config);
    let overrides = loaded
        .tool
        .wine_dll_overrides_for(Some(&loaded.context), &loaded.config);
    let preview = loaded
        .tool
        .preview_apply_for(&loaded.install_dir, Some(&loaded.context), &loaded.config)
        .ok();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "game": game_id,
                "gameName": loaded.game_plugin.display_name(),
                "tool": tool_id,
                "enabled": loaded.config.enabled,
                "installDir": loaded.install_dir,
                "savedSettings": loaded.config.settings,
                "effectiveSettings": effective.settings,
                "env": env.iter().map(|(k, v)| serde_json::json!({"key": k, "value": v})).collect::<Vec<_>>(),
                "wineDllOverrides": overrides.iter().collect::<Vec<_>>(),
                "preview": preview.as_ref().map(|preview| serde_json::json!({
                    "plannedFiles": preview.planned_files,
                    "changedFiles": preview.changed_files,
                    "unchangedFiles": preview.unchanged_files,
                    "missingInputs": preview.missing_inputs,
                })),
            }))?
        );
        return Ok(());
    }

    println!(
        "{} for {}",
        loaded.tool.display_name(),
        loaded.game_plugin.display_name()
    );
    println!("  Enabled: {}", loaded.config.enabled);
    println!("  Install: {}", loaded.install_dir.display());
    print_optiscaler_effective_summary(tool_id, &loaded.config);
    if !env.is_empty() {
        println!("  Environment:");
        for (key, value) in env {
            println!("    {key}={value}");
        }
    }
    if !overrides.is_empty() {
        println!("  Wine DLL overrides: {}", overrides.join(", "));
    }
    if let Some(preview) = preview {
        println!(
            "  Apply preview: {} changed, {} unchanged, {} missing input(s)",
            preview.changed_files.len(),
            preview.unchanged_files.len(),
            preview.missing_inputs.len()
        );
    }
    Ok(())
}

pub async fn handle_diagnose(tool_id: &str, game_id: &str, json: bool) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let loaded = load_tool_for_game(&db, tool_id, game_id).await?;
    let preview = loaded.tool.preview_apply_for(
        &loaded.install_dir,
        Some(&loaded.context),
        &loaded.config,
    )?;
    let source_dir = optiscaler_source_dir(&loaded.config);
    let gpu_arch = format!("{:?}", modde_games::gpu::detect_gpu_arch());
    let scan = if tool_id == "optiscaler" {
        let managed = modde_games::tools::optiscaler::managed_paths_from_config(&loaded.config);
        modde_games::tools::optiscaler::scan_optiscaler_install(
            game_id,
            &loaded.install_dir,
            &managed,
        )
        .ok()
    } else {
        None
    };
    let has_fsr4_int8 = source_dir.as_ref().is_some_and(|dir| {
        dir.join("FSR4_INT8")
            .join("amd_fidelityfx_upscaler_dx12.dll")
            .is_file()
    });
    let has_fsr4_latest = source_dir.as_ref().is_some_and(|dir| {
        dir.join("FSR4_LATEST")
            .join("amd_fidelityfx_upscaler_dx12.dll")
            .is_file()
    });

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "game": game_id,
                "tool": tool_id,
                "gpuArch": gpu_arch,
                "sourceDir": source_dir,
                "hasFsr4Int8": has_fsr4_int8,
                "hasFsr4Latest": has_fsr4_latest,
                "missingInputs": preview.missing_inputs,
                "changedFiles": preview.changed_files,
                "unchangedFiles": preview.unchanged_files,
                "installStatus": scan.as_ref().map(|state| state.status.to_string()),
                "proxyDlls": scan.as_ref().map(|state| state.proxy_dlls.clone()).unwrap_or_default(),
            }))?
        );
        return Ok(());
    }

    println!("{} diagnostics for {game_id}", loaded.tool.display_name());
    println!("  GPU architecture: {gpu_arch}");
    print_optiscaler_effective_summary(tool_id, &loaded.config);
    match &source_dir {
        Some(dir) => println!("  Source dir: {}", dir.display()),
        None => println!("  Source dir: unavailable"),
    }
    println!("  FSR4 INT8 payload: {}", yes_no(has_fsr4_int8));
    println!("  FSR4 Latest/FP8 payload: {}", yes_no(has_fsr4_latest));
    if let Some(scan) = scan {
        println!("  Install status: {}", scan.status);
        println!(
            "  Proxy DLLs: {}",
            if scan.proxy_dlls.is_empty() {
                "none".into()
            } else {
                scan.proxy_dlls.join(", ")
            }
        );
    }
    if preview.missing_inputs.is_empty() {
        println!("  Missing inputs: none");
    } else {
        println!("  Missing inputs:");
        for input in &preview.missing_inputs {
            println!("    {input}");
        }
    }
    println!(
        "  Apply preview: {} changed, {} unchanged",
        preview.changed_files.len(),
        preview.unchanged_files.len()
    );
    Ok(())
}

pub async fn handle_settings(tool_id: &str, game_id: &str, json: bool) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let loaded = load_tool_for_game(&db, tool_id, game_id).await?;
    let mut effective = loaded.config.clone();
    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_hardware_defaults(&mut effective);
    }
    let specs = loaded
        .tool
        .settings_schema_for(Some(&loaded.context), &loaded.config);

    if json {
        let settings = specs
            .iter()
            .map(|spec| {
                serde_json::json!({
                    "key": spec.key,
                    "label": spec.label,
                    "description": spec.description,
                    "section": spec.section,
                    "advanced": spec.advanced,
                    "kind": setting_kind_json(&spec.kind),
                    "savedValue": loaded.config.settings.get(spec.key.as_ref()),
                    "effectiveValue": effective.settings.get(spec.key.as_ref()),
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "game": game_id,
                "tool": tool_id,
                "settings": settings,
            }))?
        );
        return Ok(());
    }

    println!(
        "{} settings for {}",
        loaded.tool.display_name(),
        loaded.game_plugin.display_name()
    );

    // Group settings by section
    let mut grouped: BTreeMap<&str, Vec<&ToolSettingSpec>> = BTreeMap::new();
    for spec in &specs {
        grouped.entry(spec.section).or_default().push(spec);
    }
    for (section, section_specs) in &grouped {
        let advanced_count = section_specs.iter().filter(|s| s.advanced).count();
        let basic_count = section_specs.len() - advanced_count;
        println!(
            "\n  [{}{}{}]",
            section,
            if basic_count > 0 {
                format!(" ({basic_count})")
            } else {
                String::new()
            },
            if advanced_count > 0 {
                format!(", {advanced_count} advanced")
            } else {
                String::new()
            },
        );
        for spec in section_specs {
            let saved = loaded
                .config
                .settings
                .get(spec.key.as_ref())
                .map_or("unset".to_string(), json_value_display);
            let effective_value = effective
                .settings
                .get(spec.key.as_ref())
                .map_or("unset".to_string(), json_value_display);
            let advanced = if spec.advanced { " (advanced)" } else { "" };
            println!(
                "    {} ({}){}",
                spec.key,
                setting_kind_label(&spec.kind),
                advanced
            );
            println!("      {}", spec.label);
            println!("      saved: {saved}");
            if effective_value != saved {
                println!("      effective: {effective_value}");
            }
            if let ToolSettingKind::Select { options } = &spec.kind {
                let values = options
                    .iter()
                    .map(|option| option.value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("      allowed: {values}");
            }
        }
    }
    Ok(())
}

pub async fn handle_profiles(tool_id: &str, game_id: &str, json: bool) -> Result<()> {
    if tool_id != "optiscaler" {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "game": game_id,
                    "tool": tool_id,
                    "profiles": [],
                }))?
            );
        } else {
            println!("No profiles defined for {tool_id}");
        }
        return Ok(());
    }
    let profiles = modde_games::optiscaler::resolve_optiscaler_profiles(game_id);
    let default_id =
        modde_games::optiscaler::default_optiscaler_profile(game_id).map(|profile| profile.id);

    if json {
        let profiles = profiles
            .iter()
            .map(|profile| {
                serde_json::json!({
                    "id": profile.id,
                    "name": profile.name,
                    "default": default_id == Some(profile.id),
                    "sourceUrl": profile.source_url,
                    "testedOptiscalerVersion": profile.tested_optiscaler_version,
                    "proxyDll": profile.proxy_dll,
                    "copyCompanionFiles": profile.copy_companion_files,
                    "enableOptipatcher": profile.enable_optipatcher,
                    "fsr4Variant": profile.fsr4_variant,
                    "emulateFp8": profile.emulate_fp8,
                    "spoofDlss": profile.spoof_dlss,
                    "notes": profile.notes,
                })
            })
            .collect::<Vec<_>>();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "game": game_id,
                "tool": tool_id,
                "profiles": profiles,
            }))?
        );
        return Ok(());
    }

    println!("OptiScaler profiles for {game_id}");
    if profiles.is_empty() {
        println!("  none");
        return Ok(());
    }
    for profile in profiles {
        let default = if default_id == Some(profile.id) {
            " default"
        } else {
            ""
        };
        println!("  {}{}", profile.id, default);
        println!("    {}", profile.name);
        println!("    proxy: {}", profile.proxy_dll);
        if let Some(variant) = profile.fsr4_variant {
            println!("    profile FSR4 hint: {variant}");
        }
        if !profile.source_url.is_empty() {
            println!("    source: {}", profile.source_url);
        }
        if !profile.notes.is_empty() {
            println!("    notes: {}", profile.notes);
        }
    }
    Ok(())
}

pub async fn handle_sources(tool_id: &str, game_id: &str, json: bool) -> Result<()> {
    if tool_id != "optiscaler" {
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({
                    "game": game_id,
                    "tool": tool_id,
                    "sources": [],
                }))?
            );
        } else {
            println!("No release sources defined for {tool_id}");
        }
        return Ok(());
    }
    let db = ModdeDb::open().await.context("failed to open database")?;
    let loaded = load_tool_for_game(&db, tool_id, game_id).await?;
    let current = optiscaler_source_dir(&loaded.config);
    let sources = optiscaler_source_candidates(current.as_deref());

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "game": game_id,
                "tool": tool_id,
                "currentSourceDir": current,
                "sources": sources,
            }))?
        );
        return Ok(());
    }

    println!("OptiScaler sources for {game_id}");
    for source in sources {
        let current = if source
            .get("current")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            " current"
        } else {
            ""
        };
        println!(
            "  {}{}",
            source
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown"),
            current
        );
        println!(
            "    path: {}",
            source
                .get("path")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
        );
        println!(
            "    FSR4_INT8: {}, FSR4_LATEST: {}, OptiScaler.dll: {}",
            yes_no(
                source
                    .get("hasFsr4Int8")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            ),
            yes_no(
                source
                    .get("hasFsr4Latest")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            ),
            yes_no(
                source
                    .get("hasOptiscalerDll")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
            ),
        );
    }
    Ok(())
}

pub async fn handle_doctor(
    tool_id: Option<&str>,
    game_id: &str,
    json: bool,
    fix: bool,
) -> Result<()> {
    let db = ModdeDb::open().await.context("failed to open database")?;
    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;
    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;
    let context = ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let stored = db.load_tool_configs(&GameId::from(game_id)).await?;
    let tools = modde_games::tools::all_tools()
        .iter()
        .copied()
        .filter(|tool| tool_id.is_none_or(|wanted| wanted == tool.tool_id()))
        .collect::<Vec<_>>();
    if tools.is_empty() {
        anyhow::bail!("unknown tool: '{}'", tool_id.unwrap_or(""));
    }

    let reports = tools
        .into_iter()
        .map(|tool| {
            let mut config = stored
                .iter()
                .find(|row| row.tool_id == tool.tool_id())
                .map_or_else(
                    || tool.default_config_for(Some(&context)),
                    |row| ToolConfig {
                        tool_id: row.tool_id.clone(),
                        enabled: row.enabled,
                        settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
                    },
                );
            if tool.tool_id() == "optiscaler" {
                modde_games::tools::optiscaler::apply_game_defaults(&mut config, Some(&context));
            }
            tool_doctor_report(tool, game_id, &install_dir, &context, &config)
        })
        .collect::<Result<Vec<_>>>()?;

    if json {
        let total_score: u64 = reports
            .iter()
            .map(|r| {
                let status = r
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("ok");
                match status {
                    "ok" => 100,
                    "warning" => 75,
                    "error" => {
                        let error_count = r
                            .get("findings")
                            .and_then(serde_json::Value::as_array)
                            .map(|f| {
                                f.iter()
                                    .filter(|finding| {
                                        finding.get("severity").and_then(serde_json::Value::as_str)
                                            == Some("error")
                                    })
                                    .count() as u64
                            })
                            .unwrap_or(0);
                        50u64.saturating_sub(error_count.saturating_sub(1) * 25)
                    }
                    _ => 50,
                }
            })
            .sum::<u64>();
        let overall = if reports.is_empty() {
            100
        } else {
            total_score / reports.len() as u64
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "game": game_id,
                "installDir": install_dir,
                "health": overall,
                "reports": reports,
            }))?
        );
        return Ok(());
    }

    println!("Tool doctor for {} ({game_id})", game_plugin.display_name());
    for report in &reports {
        println!(
            "  {}: {}",
            report
                .get("tool")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown"),
            report
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
        );
        for finding in report
            .get("findings")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
        {
            let severity = finding
                .get("severity")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("info");
            let message = finding
                .get("message")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            println!("    {severity}: {message}");
        }
        let commands = report
            .get("recommendedCommands")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        if !commands.is_empty() {
            println!("    next:");
            for command in commands {
                println!("      {command}");
            }
        }
    }

    if fix && !reports.is_empty() {
        let all_cmds: Vec<&str> = reports
            .iter()
            .filter_map(|r| {
                r.get("recommendedCommands")
                    .and_then(serde_json::Value::as_array)
            })
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .collect();
        if let Some(first) = all_cmds.first() {
            println!("\n  Running fix: {first}");
            let mut parts = first.split_whitespace();
            let Some(exe) = parts.next() else {
                anyhow::bail!("fix command is empty");
            };
            let args: Vec<&str> = parts.collect();
            let status = std::process::Command::new(exe)
                .args(&args)
                .status()
                .context("failed to run fix command")?;
            if status.success() {
                println!("  Fix applied successfully.");
            } else {
                anyhow::bail!(
                    "fix command exited with code {}",
                    status.code().unwrap_or(-1)
                );
            }
        } else {
            println!("\n  No fixes needed.");
        }
    }

    Ok(())
}

fn print_optiscaler_effective_summary(tool_id: &str, config: &ToolConfig) {
    if tool_id != "optiscaler" {
        return;
    }
    let mut effective = config.clone();
    modde_games::tools::optiscaler::apply_hardware_defaults(&mut effective);
    println!(
        "  OptiScaler profile: {}",
        effective.get_str("optiscaler_profile").unwrap_or("custom")
    );
    println!(
        "  Hardware tuning: {}",
        effective.get_str("hardware_tuning").unwrap_or("auto")
    );
    println!(
        "  Effective FSR4 variant: {}",
        effective.get_str("fsr4_variant").unwrap_or("unset")
    );
    println!(
        "  Effective FP8 emulation: {}",
        effective.get_bool("emulate_fp8")
    );
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn json_value_display(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(value) => value.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => other.to_string(),
    }
}

fn setting_kind_label(kind: &ToolSettingKind) -> String {
    match kind {
        ToolSettingKind::Bool => "bool".to_string(),
        ToolSettingKind::TriStateBool => "true|false|auto".to_string(),
        ToolSettingKind::Text => "text".to_string(),
        ToolSettingKind::Path => "path".to_string(),
        ToolSettingKind::Select { .. } => "select".to_string(),
        ToolSettingKind::Number { min, max, step } => {
            format!("number {min}..{max} step {step}")
        }
        ToolSettingKind::ReadOnly => "read-only".to_string(),
    }
}

fn setting_kind_json(kind: &ToolSettingKind) -> serde_json::Value {
    match kind {
        ToolSettingKind::Bool => serde_json::json!({"type": "bool"}),
        ToolSettingKind::TriStateBool => serde_json::json!({"type": "tri_state_bool"}),
        ToolSettingKind::Text => serde_json::json!({"type": "text"}),
        ToolSettingKind::Path => serde_json::json!({"type": "path"}),
        ToolSettingKind::ReadOnly => serde_json::json!({"type": "read_only"}),
        ToolSettingKind::Number { min, max, step } => serde_json::json!({
            "type": "number",
            "min": min,
            "max": max,
            "step": step,
        }),
        ToolSettingKind::Select { options } => serde_json::json!({
            "type": "select",
            "options": options.iter().map(|option| serde_json::json!({
                "value": option.value,
                "label": option.label,
            })).collect::<Vec<_>>(),
        }),
    }
}

fn optiscaler_source_candidates(current: Option<&Path>) -> Vec<serde_json::Value> {
    let root = modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler");
    let mut sources = Vec::new();
    if let Some(current) = current {
        let name = current
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("current");
        sources.push(optiscaler_source_json(name, current, true));
    }
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_dir() || current.is_some_and(|current| current == path) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        sources.push(optiscaler_source_json(name, &path, false));
    }
    sources.sort_by(|a, b| {
        let a_current = a
            .get("current")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let b_current = b
            .get("current")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        b_current.cmp(&a_current).then_with(|| {
            a.get("name")
                .and_then(serde_json::Value::as_str)
                .cmp(&b.get("name").and_then(serde_json::Value::as_str))
        })
    });
    sources
}

fn optiscaler_source_json(name: &str, path: &Path, current: bool) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "path": path,
        "current": current,
        "hasOptiscalerDll": path.join("OptiScaler.dll").is_file(),
        "hasOptiscalerIni": path.join("OptiScaler.ini").is_file(),
        "hasOptipatcher": path.join("plugins").join("OptiPatcher.asi").is_file()
            || path.join("OptiPatcher.asi").is_file(),
        "hasFsr4Int8": path.join("FSR4_INT8").join("amd_fidelityfx_upscaler_dx12.dll").is_file(),
        "hasFsr4Latest": path.join("FSR4_LATEST").join("amd_fidelityfx_upscaler_dx12.dll").is_file(),
    })
}

fn tool_doctor_report(
    tool: &dyn GameTool,
    game_id: &str,
    install_dir: &Path,
    context: &ToolGameContext,
    config: &ToolConfig,
) -> Result<serde_json::Value> {
    let availability = match tool.detect_available() {
        modde_games::tools::ToolAvailability::Available { version } => serde_json::json!({
            "available": true,
            "version": version,
        }),
        modde_games::tools::ToolAvailability::NotInstalled { install_hint } => serde_json::json!({
            "available": false,
            "installHint": install_hint,
        }),
    };
    let preview = tool.preview_apply_for(install_dir, Some(context), config)?;
    let mut findings = Vec::new();
    let mut recommended = Vec::new();

    if !availability
        .get("available")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
    {
        findings.push(serde_json::json!({
            "severity": "error",
            "message": availability.get("installHint").and_then(serde_json::Value::as_str).unwrap_or("tool is not available"),
        }));
    }
    if !preview.missing_inputs.is_empty() {
        findings.push(serde_json::json!({
            "severity": "error",
            "message": format!("{} missing required input(s)", preview.missing_inputs.len()),
        }));
        if tool.tool_id() == "optiscaler" {
            let upgrade = if find_cached_suitable_goverlay_release(None).is_some() {
                ""
            } else {
                " --upgrade"
            };
            recommended.push(format!(
                "modde tool setup optiscaler --game {game_id} --source auto{upgrade} --apply"
            ));
        }
    }
    if !preview.changed_files.is_empty() {
        findings.push(serde_json::json!({
            "severity": "warning",
            "message": format!("{} file(s) would be written or updated", preview.changed_files.len()),
        }));
        recommended.push(format!(
            "modde tool apply {} --game {game_id}",
            tool.tool_id()
        ));
    }

    // Per-tool specific findings
    match tool.tool_id() {
        "reshade" => {
            let has_source = config.get_str("source_dir").is_some_and(|d| !d.is_empty());
            if config.enabled && !has_source {
                findings.push(serde_json::json!({
                    "severity": "error",
                    "message": "ReShade source_dir is not set; configure a path to your ReShade DLLs",
                }));
                recommended.push(format!(
                    "modde tool configure reshade --game {game_id} -- source_dir=/path/to/reshade"
                ));
            }
        }
        "proton" => {
            let has_runner = config
                .get_str("version_mode")
                .is_some_and(|m| m != "launcher_default")
                || config
                    .get_str("selected_version")
                    .is_some_and(|v| !v.is_empty());
            if config.enabled && !has_runner {
                findings.push(serde_json::json!({
                    "severity": "info",
                    "message": "Proton runner uses launcher default; no specific version pinned",
                }));
            }
        }
        "mangohud" | "vkbasalt" | "gamemode" => {
            let installed = availability
                .get("available")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            if config.enabled && !installed {
                findings.push(serde_json::json!({
                    "severity": "error",
                    "message": format!("{} is enabled but not installed", tool.display_name()),
                }));
            }
        }
        _ => {}
    }

    let mut scan_json = serde_json::Value::Null;
    if tool.tool_id() == "optiscaler" {
        let managed = modde_games::tools::optiscaler::managed_paths_from_config(config);
        let scan = modde_games::tools::optiscaler::scan_optiscaler_install(
            game_id,
            install_dir,
            &managed,
        )?;
        if matches!(
            scan.status,
            modde_games::tools::optiscaler::OptiScalerInstallStatus::Unmanaged
                | modde_games::tools::optiscaler::OptiScalerInstallStatus::PartiallyManaged
                | modde_games::tools::optiscaler::OptiScalerInstallStatus::Conflicted
        ) {
            findings.push(serde_json::json!({
                "severity": if matches!(scan.status, modde_games::tools::optiscaler::OptiScalerInstallStatus::Conflicted) { "error" } else { "warning" },
                "message": format!("OptiScaler install is {}", scan.status),
            }));
            recommended.push(format!("modde tool diagnose optiscaler --game {game_id}"));
        }
        scan_json = serde_json::json!({
            "status": scan.status.to_string(),
            "proxyDlls": scan.proxy_dlls,
            "wineDllOverrides": scan.wine_dll_overrides,
            "recognizedFiles": scan.recognized_files.iter().map(|file| serde_json::json!({
                "path": file.rel_path,
                "managed": file.managed,
                "hash": file.hash,
            })).collect::<Vec<_>>(),
        });
    }

    if findings.is_empty() {
        findings.push(serde_json::json!({
            "severity": "ok",
            "message": "no issues detected",
        }));
    }
    recommended.sort();
    recommended.dedup();
    let status = if findings
        .iter()
        .any(|finding| finding.get("severity").and_then(serde_json::Value::as_str) == Some("error"))
    {
        "error"
    } else if findings.iter().any(|finding| {
        finding.get("severity").and_then(serde_json::Value::as_str) == Some("warning")
    }) {
        "warning"
    } else {
        "ok"
    };

    Ok(serde_json::json!({
        "tool": tool.tool_id(),
        "displayName": tool.display_name(),
        "enabled": config.enabled,
        "status": status,
        "availability": availability,
        "findings": findings,
        "recommendedCommands": recommended,
        "preview": {
            "plannedFiles": preview.planned_files,
            "changedFiles": preview.changed_files,
            "unchangedFiles": preview.unchanged_files,
            "missingInputs": preview.missing_inputs,
        },
        "scan": scan_json,
    }))
}

fn unknown_setting_error(
    key: &str,
    specs: &[modde_games::tools::ToolSettingSpec],
) -> anyhow::Error {
    let known = specs
        .iter()
        .map(|spec| spec.key.as_ref())
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::anyhow!("unknown setting '{key}'. Known settings: {known}")
}

fn parse_setting_value(
    spec: &modde_games::tools::ToolSettingSpec,
    value: &str,
) -> Result<serde_json::Value> {
    match &spec.kind {
        ToolSettingKind::Bool => match value {
            "true" | "1" | "yes" | "on" => Ok(serde_json::json!(true)),
            "false" | "0" | "no" | "off" => Ok(serde_json::json!(false)),
            _ => anyhow::bail!("setting '{}' expects a boolean", spec.key),
        },
        ToolSettingKind::TriStateBool => match value {
            "auto" => Ok(serde_json::Value::Null),
            "true" | "1" | "yes" | "on" => Ok(serde_json::json!(true)),
            "false" | "0" | "no" | "off" => Ok(serde_json::json!(false)),
            _ => anyhow::bail!("setting '{}' expects true, false, or auto", spec.key),
        },
        ToolSettingKind::Number { min, max, .. } => {
            let parsed = value
                .parse::<f64>()
                .with_context(|| format!("setting '{}' expects a number", spec.key))?;
            if parsed < *min || parsed > *max {
                anyhow::bail!("setting '{}' must be between {min} and {max}", spec.key);
            }
            Ok(serde_json::json!(parsed))
        }
        ToolSettingKind::Select { options } => {
            if options.iter().any(|option| option.value == value) {
                Ok(serde_json::json!(value))
            } else {
                let options = options
                    .iter()
                    .map(|option| option.value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                anyhow::bail!("setting '{}' must be one of: {options}", spec.key);
            }
        }
        ToolSettingKind::ReadOnly => anyhow::bail!("setting '{}' is read-only", spec.key),
        ToolSettingKind::Text | ToolSettingKind::Path => Ok(serde_json::json!(value)),
    }
}

fn set_config_value(config: &mut ToolConfig, key: &str, value: serde_json::Value) {
    if let Some(path) = key.strip_prefix("ini_overrides.") {
        set_nested_ini_override(config, path, value);
    } else {
        config.set(key, value);
    }
}

fn set_nested_ini_override(config: &mut ToolConfig, path: &str, value: serde_json::Value) {
    let mut parts = path.splitn(2, '.');
    let Some(section) = parts.next().filter(|part| !part.is_empty()) else {
        return;
    };
    let Some(name) = parts.next().filter(|part| !part.is_empty()) else {
        return;
    };
    if !config.settings.is_object() {
        config.settings = serde_json::json!({});
    }
    let root = config
        .settings
        .as_object_mut()
        .expect("object just ensured");
    let ini = root
        .entry("ini_overrides")
        .or_insert_with(|| serde_json::json!({}));
    if !ini.is_object() {
        *ini = serde_json::json!({});
    }
    let ini = ini.as_object_mut().expect("object just ensured");
    let section = ini
        .entry(section.to_string())
        .or_insert_with(|| serde_json::json!({}));
    if !section.is_object() {
        *section = serde_json::json!({});
    }
    section
        .as_object_mut()
        .expect("object just ensured")
        .insert(name.to_string(), value);
}

/// Enable a tool for a game.
pub async fn handle_enable(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown tool: '{tool_id}'\nAvailable: {}",
            modde_games::tools::all_tools()
                .iter()
                .map(|t| t.tool_id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    // Load existing or use defaults
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    let mut config = if let Some(row) = db.load_tool_config(&GameId::from(game_id), tool_id).await?
    {
        modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: true,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        }
    } else {
        let mut cfg = tool.default_config_for(context.as_ref());
        cfg.enabled = true;
        cfg
    };

    config.enabled = true;

    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(&GameId::from(game_id), tool_id, true, &settings_json)
        .await?;

    println!("Enabled {} for {game_id}", tool.display_name());

    // Generate config file if applicable
    config.set("_game_id", serde_json::json!(game_id));
    if let Some(generated) = tool.generate_config(&config) {
        if let Some(parent) = generated.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&generated.path, &generated.content)?;
        println!("  Config: {}", generated.path.display());
    }

    Ok(())
}

/// Disable a tool for a game.
pub async fn handle_disable(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    // Load existing config to preserve settings
    let settings_json = db
        .load_tool_config(&GameId::from(game_id), tool_id)
        .await?
        .map_or_else(|| "{}".into(), |r| r.settings_json);

    db.save_tool_config(&GameId::from(game_id), tool_id, false, &settings_json)
        .await?;

    println!("Disabled {} for {game_id}", tool.display_name());

    Ok(())
}

/// Configure a tool's settings.
pub async fn handle_configure(
    tool_id: &str,
    game_id: &str,
    settings: &[String],
    reset_keys: &[String],
) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    // Load existing or defaults
    let context = modde_games::resolve_game_plugin(game_id).map(|plugin| {
        modde_games::tools::ToolGameContext::from_parts(game_id, plugin.display_name(), None, None)
    });
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(context.as_ref()),
    };

    for setting in settings {
        let (key, value) = setting.split_once('=').ok_or_else(|| {
            anyhow::anyhow!("invalid setting format: '{setting}' (expected key=value)")
        })?;

        let specs = tool.settings_schema_for(context.as_ref(), &config);
        let spec = specs
            .iter()
            .find(|spec| spec.key == key)
            .ok_or_else(|| unknown_setting_error(key, &specs))?;
        let json_value = parse_setting_value(spec, value)?;
        set_config_value(&mut config, key, json_value);
        if tool_id == "optiscaler" && key == "optiscaler_profile" {
            modde_games::tools::optiscaler::apply_profile_by_id(&mut config, game_id, value);
        }
        println!("  {key} = {value}");
    }

    for reset_key in reset_keys {
        let defaults = tool.default_config_for(context.as_ref());
        let default_value = defaults.settings.get(reset_key).cloned();
        let specs = tool.settings_schema_for(context.as_ref(), &config);
        let spec = specs
            .iter()
            .find(|spec| spec.key.as_ref() == reset_key.as_str())
            .ok_or_else(|| unknown_setting_error(reset_key, &specs))?;
        if spec.kind == ToolSettingKind::ReadOnly {
            anyhow::bail!("cannot reset read-only setting '{reset_key}'");
        }
        if let Some(value) = default_value {
            config.set(reset_key, value.clone());
            println!("  {reset_key} = (default)");
        } else {
            config
                .settings
                .as_object_mut()
                .map(|obj| obj.remove(reset_key));
            println!("  {reset_key} = (removed)");
        }
    }

    let settings_json = serde_json::to_string(&config.settings)?;
    db.save_tool_config(
        &GameId::from(game_id),
        tool_id,
        config.enabled,
        &settings_json,
    )
    .await?;

    // Regenerate config file
    config.set("_game_id", serde_json::json!(game_id));
    if let Some(generated) = tool.generate_config(&config) {
        if let Some(parent) = generated.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&generated.path, &generated.content)?;
        println!("  Config written: {}", generated.path.display());
    }

    println!("Updated {} config for {game_id}", tool.display_name());

    Ok(())
}

pub async fn handle_setup(options: ToolSetupOptions) -> Result<()> {
    if options.tool_id != "optiscaler" {
        anyhow::bail!("guided setup currently supports only optiscaler");
    }
    if !matches!(options.hardware_tuning.as_str(), "auto" | "manual") {
        anyhow::bail!("hardware tuning must be 'auto' or 'manual'");
    }

    let db = ModdeDb::open().await.context("failed to open database")?;
    let mut loaded = load_tool_for_game(&db, &options.tool_id, &options.game).await?;
    loaded.config.enabled = true;

    let profile = options.profile.as_deref().unwrap_or("community-dxgi");
    if !modde_games::tools::optiscaler::apply_profile_by_id(
        &mut loaded.config,
        &options.game,
        profile,
    ) {
        anyhow::bail!(
            "unknown OptiScaler profile '{profile}' for {}",
            options.game
        );
    }
    loaded.config.set(
        "hardware_tuning",
        serde_json::json!(options.hardware_tuning),
    );

    configure_optiscaler_source(&mut loaded.config, &options).await?;

    let preview = loaded.tool.preview_apply_for(
        &loaded.install_dir,
        Some(&loaded.context),
        &loaded.config,
    )?;
    if !preview.missing_inputs.is_empty() {
        println!("OptiScaler setup is not ready to apply; missing inputs:");
        for input in &preview.missing_inputs {
            println!("  {input}");
        }
        anyhow::bail!(
            "setup has missing inputs; rerun with --upgrade or choose a cached/local source with required payloads"
        );
    }

    if options.dry_run {
        println!("[dry-run] OptiScaler setup for {}", options.game);
        print_optiscaler_effective_summary(&options.tool_id, &loaded.config);
        println!(
            "  Source: {} / {} / {}",
            loaded.config.get_str("source_mode").unwrap_or(""),
            loaded.config.get_str("release_tag").unwrap_or(""),
            loaded.config.get_str("release_asset").unwrap_or("")
        );
        println!(
            "  Preview: {} changed, {} unchanged",
            preview.changed_files.len(),
            preview.unchanged_files.len()
        );
        return Ok(());
    }

    save_tool_config(&db, &options.game, &options.tool_id, &loaded.config).await?;
    println!("Configured OptiScaler for {}", options.game);
    print_optiscaler_effective_summary(&options.tool_id, &loaded.config);
    println!(
        "  Source: {} / {} / {}",
        loaded.config.get_str("source_mode").unwrap_or(""),
        loaded.config.get_str("release_tag").unwrap_or(""),
        loaded.config.get_str("release_asset").unwrap_or("")
    );
    println!(
        "  Preview: {} changed, {} unchanged",
        preview.changed_files.len(),
        preview.unchanged_files.len()
    );

    if options.apply {
        let applied =
            loaded
                .tool
                .apply_for(&loaded.install_dir, Some(&loaded.context), &loaded.config)?;
        let rel_paths: Vec<String> = applied
            .files
            .iter()
            .map(|path| path.to_string_lossy().to_string())
            .collect();
        db.save_applied_files(
            &GameId::from(options.game.as_str()),
            &options.tool_id,
            &rel_paths,
        )
        .await?;
        let mut updated_config = loaded.config.clone();
        updated_config.set(
            "managed_manifest",
            modde_games::tools::optiscaler::managed_manifest_json(&loaded.install_dir, &applied),
        );
        save_tool_config(&db, &options.game, &options.tool_id, &updated_config).await?;
        println!(
            "Applied OptiScaler ({} files) to {}",
            applied.files.len(),
            loaded.install_dir.display()
        );
        for file in &applied.files {
            println!("  {}", file.display());
        }
    } else {
        println!(
            "Not applying files. Re-run with --apply or use `modde tool apply optiscaler --game {}`.",
            options.game
        );
    }

    Ok(())
}

async fn configure_optiscaler_source(
    config: &mut ToolConfig,
    options: &ToolSetupOptions,
) -> Result<()> {
    if let Some(path) = &options.local_source_dir {
        config.set("source_mode", serde_json::json!("local_dir"));
        config.set(
            "local_source_dir",
            serde_json::json!(path.display().to_string()),
        );
        return Ok(());
    }

    if let (Some(tag), Some(asset)) = (&options.release_tag, &options.release_asset) {
        apply_release_selection_to_config(config, tag, asset);
        return Ok(());
    }

    match options.source.as_str() {
        "local" => anyhow::bail!("--source local requires --local-source-dir"),
        "fgmod" => {
            config.set("source_mode", serde_json::json!("goverlay_fgmod"));
            return Ok(());
        }
        "official" if options.upgrade => {
            install_latest_matching_release(config, &options.game, "official:").await?;
            return Ok(());
        }
        "goverlay-edge" if options.upgrade => {
            install_latest_matching_release(config, &options.game, "goverlay-edge:").await?;
            return Ok(());
        }
        "goverlay-stable" if options.upgrade => {
            install_latest_matching_release(config, &options.game, "goverlay-stable:").await?;
            return Ok(());
        }
        "auto" if options.upgrade => {
            install_latest_matching_release(config, &options.game, "goverlay-edge:").await?;
            return Ok(());
        }
        "official" | "goverlay-edge" | "goverlay-stable" | "auto" => {}
        other => anyhow::bail!(
            "unknown OptiScaler setup source '{other}' (expected auto, goverlay-edge, goverlay-stable, official, fgmod, local)"
        ),
    }

    if optiscaler_source_has_required_payloads(config) {
        return Ok(());
    }

    let wanted_prefix = match options.source.as_str() {
        "goverlay-stable" => Some("goverlay-stable_"),
        "goverlay-edge" | "auto" => Some("goverlay-edge_"),
        _ => None,
    };
    if let Some((tag, asset)) = find_cached_suitable_goverlay_release(wanted_prefix) {
        apply_release_selection_to_config(config, &tag, &asset);
        return Ok(());
    }

    if options.source == "official" {
        anyhow::bail!(
            "current official source does not expose FSR4_INT8/FSR4_LATEST payloads; choose --source goverlay-edge or pass --upgrade"
        );
    }
    anyhow::bail!(
        "no cached OptiScaler source with FSR4_INT8/FSR4_LATEST payloads found; rerun with --upgrade to install one"
    );
}

async fn install_latest_matching_release(
    config: &mut ToolConfig,
    game_id: &str,
    tag_prefix: &str,
) -> Result<()> {
    let tool = modde_games::tools::resolve_tool("optiscaler")
        .ok_or_else(|| anyhow::anyhow!("unknown tool: optiscaler"))?;
    let releases = tool.list_releases().await?;
    let (tag, asset) = releases
        .iter()
        .filter(|release| release.tag.starts_with(tag_prefix))
        .find_map(|release| {
            tool.installable_release_assets(release)
                .first()
                .map(|asset| (release.tag.clone(), asset.clone()))
        })
        .ok_or_else(|| {
            anyhow::anyhow!("no installable OptiScaler release found for prefix {tag_prefix}")
        })?;

    let installed = tool
        .install_release(game_id, config.clone(), &tag, &asset)
        .await?;
    *config = installed;
    Ok(())
}

fn apply_release_selection_to_config(config: &mut ToolConfig, tag: &str, asset: &str) {
    if tag.starts_with("goverlay-edge:") {
        config.set("source_mode", serde_json::json!("goverlay_builds"));
        config.set("goverlay_channel", serde_json::json!("edge"));
    } else if tag.starts_with("goverlay-stable:") {
        config.set("source_mode", serde_json::json!("goverlay_builds"));
        config.set("goverlay_channel", serde_json::json!("stable"));
    } else if tag.starts_with("goverlay-master:") {
        config.set("source_mode", serde_json::json!("goverlay_builds"));
        config.set("goverlay_channel", serde_json::json!("master"));
    } else {
        config.set("source_mode", serde_json::json!("github_release"));
    }
    config.set("release_tag", serde_json::json!(tag));
    config.set("release_asset", serde_json::json!(asset));
}

fn optiscaler_source_has_required_payloads(config: &ToolConfig) -> bool {
    optiscaler_source_dir(config).is_some_and(|dir| {
        dir.join("FSR4_INT8")
            .join("amd_fidelityfx_upscaler_dx12.dll")
            .is_file()
            && dir
                .join("FSR4_LATEST")
                .join("amd_fidelityfx_upscaler_dx12.dll")
                .is_file()
    })
}

fn optiscaler_source_dir(config: &ToolConfig) -> Option<PathBuf> {
    match config.get_str("source_mode").unwrap_or("goverlay_fgmod") {
        "local_dir" => config
            .get_str("local_source_dir")
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from),
        "github_release" | "goverlay_builds" => {
            let tag = config.get_str("release_tag")?;
            cached_release_source_dir(tag)
        }
        _ => dirs_home().map(|home| home.join(".local/share/goverlay/fgmod")),
    }
}

fn cached_release_source_dir(tag: &str) -> Option<PathBuf> {
    let mut candidates = vec![modde_games::tools::optiscaler::cached_release_dir(tag)];
    if let Some(official) = tag.strip_prefix("official:") {
        candidates.push(modde_games::tools::optiscaler::cached_release_dir(official));
    }
    candidates
        .into_iter()
        .find(|dir| dir.join("OptiScaler.dll").is_file())
}

fn find_cached_suitable_goverlay_release(prefix: Option<&str>) -> Option<(String, String)> {
    let root = modde_core::paths::modde_data_dir()
        .join("tools")
        .join("optiscaler");
    let entries = std::fs::read_dir(root).ok()?;
    let mut candidates = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.to_string();
            if let Some(prefix) = prefix
                && !name.starts_with(prefix)
            {
                return None;
            }
            let has_payloads = path
                .join("FSR4_INT8")
                .join("amd_fidelityfx_upscaler_dx12.dll")
                .is_file()
                && path
                    .join("FSR4_LATEST")
                    .join("amd_fidelityfx_upscaler_dx12.dll")
                    .is_file();
            if !has_payloads || !path.join("OptiScaler.dll").is_file() {
                return None;
            }
            cached_goverlay_name_to_release(&name)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().next()
}

fn cached_goverlay_name_to_release(name: &str) -> Option<(String, String)> {
    if let Some(rest) = name.strip_prefix("goverlay-edge_") {
        return Some((
            format!("goverlay-edge:{rest}"),
            "optiscaler-edge.7z".to_string(),
        ));
    }
    if let Some(rest) = name.strip_prefix("goverlay-stable_") {
        return Some((
            format!("goverlay-stable:{rest}"),
            "optiScaler-stable.7z".to_string(),
        ));
    }
    if let Some(rest) = name.strip_prefix("goverlay-master_") {
        return Some((
            format!("goverlay-master:{rest}"),
            format!(
                "OptiScaler_master_{}.7z",
                rest.strip_prefix("master-").unwrap_or(rest)
            ),
        ));
    }
    None
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

/// Apply tool patches to the game directory.
pub async fn handle_apply(tool_id: &str, game_id: &str, dry_run: bool) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    let context = modde_games::tools::ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(Some(&context)),
    };
    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_game_defaults(&mut config, Some(&context));
    }

    if dry_run {
        let preview = tool.preview_apply_for(&install_dir, Some(&context), &config)?;
        println!(
            "[dry-run] {} for {} at {}",
            tool.display_name(),
            game_id,
            install_dir.display()
        );
        if preview.changed_files.is_empty() {
            println!(
                "  All {} planned file(s) already current",
                preview.planned_files.len()
            );
        } else {
            println!(
                "  Would write/update {} file(s):",
                preview.changed_files.len()
            );
            for f in &preview.changed_files {
                println!("    {}", f.display());
            }
        }
        if !preview.missing_inputs.is_empty() {
            println!("  Missing inputs:");
            for input in &preview.missing_inputs {
                println!("    {input}");
            }
        }
        return Ok(());
    }

    let applied = tool.apply_for(&install_dir, Some(&context), &config)?;

    if applied.files.is_empty() {
        println!("No files to apply for {}", tool.display_name());
        return Ok(());
    }

    // Record applied files in the database
    let rel_paths: Vec<String> = applied
        .files
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    db.save_applied_files(&GameId::from(game_id), tool_id, &rel_paths)
        .await?;
    if tool_id == "optiscaler" {
        let mut updated_config = config.clone();
        updated_config.set(
            "managed_manifest",
            modde_games::tools::optiscaler::managed_manifest_json(&install_dir, &applied),
        );
        let settings_json = serde_json::to_string(&updated_config.settings)?;
        db.save_tool_config(&GameId::from(game_id), tool_id, true, &settings_json)
            .await?;
    }

    println!(
        "Applied {} ({} files) to {}",
        tool.display_name(),
        applied.files.len(),
        install_dir.display(),
    );
    for f in &applied.files {
        println!("  {}", f.display());
    }

    Ok(())
}

/// Preview tool patches without writing to the game directory.
pub async fn handle_preview(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;
    let context = modde_games::tools::ToolGameContext::from_parts(
        game_id,
        game_plugin.display_name(),
        Some(install_dir.clone()),
        None,
    );
    let mut config = match db.load_tool_config(&GameId::from(game_id), tool_id).await? {
        Some(row) => modde_games::tools::ToolConfig {
            tool_id: row.tool_id,
            enabled: row.enabled,
            settings: serde_json::from_str(&row.settings_json).unwrap_or_default(),
        },
        None => tool.default_config_for(Some(&context)),
    };

    if tool_id == "optiscaler" {
        modde_games::tools::optiscaler::apply_game_defaults(&mut config, Some(&context));
    }

    let preview = tool.preview_apply_for(&install_dir, Some(&context), &config)?;

    println!(
        "Preview {} for {} at {}",
        tool.display_name(),
        game_id,
        install_dir.display()
    );
    println!("  Planned files: {}", preview.planned_files.len());
    println!("  Changed files: {}", preview.changed_files.len());
    println!("  Unchanged files: {}", preview.unchanged_files.len());

    if !preview.missing_inputs.is_empty() {
        println!("\nMissing inputs:");
        for input in &preview.missing_inputs {
            println!("  {input}");
        }
    }
    if !preview.changed_files.is_empty() {
        println!("\nWould write/update:");
        for path in &preview.changed_files {
            println!("  {}", path.display());
        }
    }
    if !preview.unchanged_files.is_empty() {
        println!("\nAlready current:");
        for path in &preview.unchanged_files {
            println!("  {}", path.display());
        }
    }

    if tool_id == "optiscaler" {
        let mut effective_config = config.clone();
        modde_games::tools::optiscaler::apply_hardware_defaults(&mut effective_config);
        println!("\nEffective OptiScaler settings:");
        for key in [
            "optiscaler_profile",
            "hardware_tuning",
            "fsr4_variant",
            "emulate_fp8",
        ] {
            if let Some(value) = effective_config.settings.get(key) {
                println!("  {key} = {value}");
            }
        }
    }

    let env = tool.env_vars(&config);
    if !env.is_empty() {
        println!("\nEnvironment preview:");
        for (key, value) in env {
            println!("  {key}={value}");
        }
    }

    Ok(())
}

/// Revert tool patches from the game directory.
pub async fn handle_revert(tool_id: &str, game_id: &str) -> Result<()> {
    let tool = modde_games::tools::resolve_tool(tool_id)
        .ok_or_else(|| anyhow::anyhow!("unknown tool: '{tool_id}'"))?;

    let game_plugin = modde_games::resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game: '{game_id}'"))?;

    let install_dir = game_plugin.detect_install().ok_or_else(|| {
        anyhow::anyhow!(
            "could not detect install dir for {}",
            game_plugin.display_name()
        )
    })?;

    let db = ModdeDb::open().await.context("failed to open database")?;

    let files = db
        .load_applied_files(&GameId::from(game_id), tool_id)
        .await?;
    if files.is_empty() {
        println!("No applied files to revert for {}", tool.display_name());
        return Ok(());
    }

    let applied = modde_games::tools::AppliedFiles {
        files: files.iter().map(PathBuf::from).collect(),
    };

    tool.revert(&install_dir, &applied)?;
    db.clear_applied_files(&GameId::from(game_id), tool_id)
        .await?;

    println!(
        "Reverted {} ({} files) from {}",
        tool.display_name(),
        files.len(),
        install_dir.display(),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(spec: &ToolSettingSpec, value: &str) -> std::result::Result<serde_json::Value, String> {
        parse_setting_value(spec, value).map_err(|e| format!("{e:#}"))
    }

    #[test]
    fn setting_values_are_parsed_by_kind() {
        let flag = ToolSettingSpec::bool("flag", "Flag", "");
        assert_eq!(parse(&flag, "on"), Ok(json!(true)));
        assert_eq!(parse(&flag, "0"), Ok(json!(false)));
        assert!(parse(&flag, "maybe").unwrap_err().contains("'flag'"));

        let tri = ToolSettingSpec::tri_state_bool("tri", "Tri", "");
        assert_eq!(parse(&tri, "auto"), Ok(serde_json::Value::Null));
        assert_eq!(parse(&tri, "yes"), Ok(json!(true)));
        assert!(parse(&tri, "maybe").is_err());

        let number = ToolSettingSpec::number("n", "N", "", 0.0, 1.0, 0.1);
        assert_eq!(parse(&number, "1"), Ok(json!(1.0)));
        assert!(parse(&number, "1.01").unwrap_err().contains("between 0 and 1"));
        assert!(parse(&number, "-0.1").is_err());
        assert!(parse(&number, "abc").unwrap_err().contains("expects a number"));

        let select = ToolSettingSpec::select("mode", "Mode", "", &["a", "b"]);
        assert_eq!(parse(&select, "b"), Ok(json!("b")));
        assert!(parse(&select, "c").unwrap_err().contains("one of: a, b"));

        let read_only = ToolSettingSpec::read_only("ro", "RO", "");
        assert!(parse(&read_only, "x").unwrap_err().contains("read-only"));
        assert_eq!(parse(&ToolSettingSpec::text("t", "T", ""), "x"), Ok(json!("x")));
    }

    #[test]
    fn ini_override_keys_nest_by_section_and_name() {
        let mut config = ToolConfig::new("optiscaler");
        config.settings = json!("not an object");
        set_config_value(&mut config, "ini_overrides.Upscalers.Sharpness", json!("0.5"));
        set_config_value(&mut config, "ini_overrides.Sec.a.b", json!(1));
        set_config_value(&mut config, "ini_overrides.Sec", json!(2));
        set_config_value(&mut config, "ini_overrides..k", json!(3));
        set_config_value(&mut config, "plain", json!(true));
        assert_eq!(
            config.settings,
            json!({
                "ini_overrides": { "Upscalers": { "Sharpness": "0.5" }, "Sec": { "a.b": 1 } },
                "plain": true
            })
        );

        config.settings = json!({ "ini_overrides": "broken" });
        set_config_value(&mut config, "ini_overrides.A.b", json!(1));
        assert_eq!(config.settings, json!({ "ini_overrides": { "A": { "b": 1 } } }));
    }

    #[test]
    fn cached_goverlay_dirs_map_back_to_release_selection() {
        let release = |name| cached_goverlay_name_to_release(name);
        assert_eq!(
            release("goverlay-edge_edge-0.9.12"),
            Some(("goverlay-edge:edge-0.9.12".into(), "optiscaler-edge.7z".into()))
        );
        assert_eq!(
            release("goverlay-stable_0.9.1-0"),
            Some(("goverlay-stable:0.9.1-0".into(), "optiScaler-stable.7z".into()))
        );
        assert_eq!(
            release("goverlay-master_master-3ce6192"),
            Some((
                "goverlay-master:master-3ce6192".into(),
                "OptiScaler_master_3ce6192.7z".into()
            ))
        );
        assert_eq!(release("official_v0.9.1"), None);
    }

    #[test]
    fn release_selection_sets_source_and_channel() {
        let selected = |tag| {
            let mut config = ToolConfig::new("optiscaler");
            apply_release_selection_to_config(&mut config, tag, "a.7z");
            (
                config.get_str("source_mode").map(str::to_string),
                config.get_str("goverlay_channel").map(str::to_string),
                config.get_str("release_tag").map(str::to_string),
            )
        };
        let some = |s: &str| Some(s.to_string());
        assert_eq!(
            selected("goverlay-master:m"),
            (some("goverlay_builds"), some("master"), some("goverlay-master:m"))
        );
        assert_eq!(
            selected("goverlay-edge"),
            (some("github_release"), None, some("goverlay-edge"))
        );
    }
}
