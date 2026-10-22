#![allow(clippy::wildcard_imports)]
//! Launcher integration and MO2 staging deployment helpers.

use super::*;

pub fn print_launcher_configuration_report(
    report: &modde_games::launcher::LauncherConfigurationReport,
) {
    if let Some(wine_overrides) = &report.wine_overrides {
        print_wine_override_report(wine_overrides);
    }
    if let Some(wrapper) = &report.launch_wrapper {
        print_launch_wrapper_report(wrapper);
    }
    if let Some(registration) = &report.wrapper_registration {
        print_wrapper_registration_report(registration);
    }
}

fn print_wine_override_report(report: &modde_games::launcher::WineOverrideReport) {
    match report {
        modde_games::launcher::WineOverrideReport::HeroicUpdated { value } => {
            println!("  Updated Heroic config with WINEDLLOVERRIDES: {value}");
        }
        modde_games::launcher::WineOverrideReport::SteamInstruction { override_value } => {
            println!(
                "\nSteam: Add this to your launch options:\n  \
                 WINEDLLOVERRIDES=\"{override_value}\" %command%"
            );
        }
        modde_games::launcher::WineOverrideReport::UnknownInstruction { override_value } => {
            println!(
                "\nSet this environment variable before launching:\n  \
                 WINEDLLOVERRIDES=\"{override_value}\""
            );
        }
    }
}

fn print_launch_wrapper_report(report: &modde_games::launcher::LaunchWrapperReport) {
    println!("  Launch wrapper: {}", report.path.display());
    if report.restore_count > 0 {
        println!("  Launch wrapper: restores {} DLL(s)", report.restore_count);
    }
    if report.tool_env_var_count > 0 {
        println!(
            "  Launch wrapper: exports {} tool env var(s)",
            report.tool_env_var_count
        );
    }
}

fn print_wrapper_registration_report(report: &modde_games::launcher::WrapperRegistrationReport) {
    match report {
        modde_games::launcher::WrapperRegistrationReport::HeroicRegistered => {
            println!("  Registered modde launch wrapper in Heroic (position: after fgmod)");
        }
        modde_games::launcher::WrapperRegistrationReport::ManualInstruction { wrapper_path } => {
            let wrapper_str = wrapper_path.display();
            println!("\nAdd this wrapper before your game launcher:\n  {wrapper_str} --");
        }
    }
}

pub fn print_tool_environment_report(report: &modde_games::launcher::ToolEnvironmentReport) {
    if report.env_var_count > 0 {
        println!(
            "  Applied {} tool env var(s) to Heroic config",
            report.env_var_count
        );
    }
    if report.wrapper_count > 0 {
        println!(
            "  Registered {} tool wrapper(s) in Heroic config",
            report.wrapper_count
        );
    }
}

/// Fetch a Nexus Collection manifest via the two-step API.
///
/// Step 1: `GET /v1/collections/{slug}.json` → discover `game_domain` + latest revision.
/// Step 2: `GET /v1/collections/{slug}/revisions/{rev}.json?game_domain_name={game_domain}`.
///
/// If `version` is already known (e.g. from CLI arg), the revision number is
pub async fn configure_wine_overrides(
    game_id: &str,
    game_dir: &Path,
    staging: &Path,
) -> Result<modde_games::launcher::LauncherConfigurationReport> {
    let mut report = modde_games::launcher::LauncherConfigurationReport::default();
    let Some(plugin) = modde_games::resolve_game_plugin(game_id) else {
        info!(%game_id, "no game plugin found, skipping Wine DLL override detection");
        return Ok(report);
    };

    // Merge overrides from deployed game dir and staging (staging catches DLLs
    // that fgmod may have deleted from the game dir)
    let mut overrides = plugin.wine_dll_overrides(game_dir);
    for dll in plugin.wine_dll_overrides_from_staging(staging) {
        if !overrides.contains(&dll) {
            overrides.push(dll);
        }
    }
    if overrides.is_empty() {
        info!("no proxy DLLs detected, no Wine overrides needed");
        return Ok(report);
    }

    println!(
        "  Detected proxy DLLs needing Wine overrides: {}",
        overrides
            .iter()
            .map(|d| format!("{d}.dll"))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let launcher = modde_games::launcher::detect_launcher(game_dir);
    info!(?launcher, "detected game launcher");

    // Set WINEDLLOVERRIDES in the launcher config (Linux only — Wine/Proton concept)
    #[cfg(all(target_os = "linux", feature = "linux-integrations"))]
    {
        report.wine_overrides = modde_games::launcher::apply_wine_overrides(&launcher, &overrides)?;
    }

    // Collect tool env vars for the launch wrapper
    let tool_env_vars = match modde_core::db::ModdeDb::open().await {
        Ok(db) => {
            modde_games::launcher::collect_tool_env_vars(&modde_core::GameId::from(game_id), &db)
                .await
                .unwrap_or_default()
        }
        Err(_) => Vec::new(),
    };

    // Generate a launch wrapper that restores mod DLLs deleted by fgmod + exports tool env vars
    if let Some(wrapper) = modde_games::launcher::generate_launch_wrapper(
        game_dir,
        staging,
        &modde_core::GameId::from(game_id),
        &tool_env_vars,
    )? {
        report.wrapper_registration =
            modde_games::launcher::register_heroic_wrapper(&launcher, &wrapper.path)?;
        report.launch_wrapper = Some(wrapper);
    }

    print_launcher_configuration_report(&report);

    Ok(report)
}
