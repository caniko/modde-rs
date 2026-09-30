#![allow(clippy::wildcard_imports)]
//! Async CLI command dispatcher.

use anyhow::Result;

use crate::commands;

use super::super::args::*;

pub(super) fn dispatch_async(cli: Cli) -> Result<()> {
    tokio::runtime::Runtime::new()?.block_on(async {
        match cli.command {
            Commands::Library { action } => commands::library::handle(action).await?,
            Commands::Play {
                profile,
                game,
                no_deploy,
                no_switch,
                no_capture,
            } => commands::play::handle(profile, game, no_deploy, no_switch, no_capture).await?,
            Commands::Perf { action } => match action {
                PerfAction::Sandbox { id, profile, pairs, duration, warmup_seconds } => {
                    commands::perf::sandbox_pairs(&id, &profile, pairs, duration, warmup_seconds).await?;
                }
                PerfAction::Run {
                    profile,
                    game,
                    duration,
                    label,
                    warmup_seconds,
                    no_deploy,
                } => {
                    commands::perf::handle_run(
                        profile,
                        game,
                        duration,
                        label,
                        warmup_seconds,
                        no_deploy,
                    )
                    .await?;
                }
                PerfAction::Ingest {
                    run_id,
                    csv,
                    warmup_seconds,
                } => {
                    commands::perf::handle_ingest(run_id, csv, warmup_seconds).await?;
                }
                PerfAction::List {
                    game,
                    profile,
                    limit,
                } => {
                    commands::perf::handle_list(game, profile, limit).await?;
                }
                PerfAction::Show { run_id } => {
                    commands::perf::handle_show(run_id).await?;
                }
                PerfAction::Compare {
                    baseline,
                    candidate,
                } => {
                    commands::perf::handle_compare(baseline, candidate).await?;
                }
            },
            Commands::Bisect { action } => match action {
                BisectAction::Start {
                    game,
                    profile,
                    oracle,
                    baseline_run,
                    perf_p99_frame_time_percent,
                    perf_one_percent_low_fps_percent,
                    perf_alpha_micros,
                    perf_min_samples,
                    crash_dir,
                    force_save_risk,
                    keep_profiles,
                } => {
                    commands::bisect::handle_start(
                        game,
                        profile,
                        oracle,
                        baseline_run,
                        perf_p99_frame_time_percent,
                        perf_one_percent_low_fps_percent,
                        perf_alpha_micros,
                        perf_min_samples,
                        crash_dir,
                        force_save_risk,
                        keep_profiles,
                    )
                    .await?;
                }
                BisectAction::Run { session_id } => {
                    commands::bisect::handle_run(session_id).await?;
                }
                BisectAction::Retry { session_id } => {
                    commands::bisect::handle_retry(session_id).await?;
                }
                BisectAction::Mark {
                    session_id,
                    result,
                    notes,
                } => {
                    commands::bisect::handle_mark(session_id, result, notes).await?;
                }
                BisectAction::Status { session_id } => {
                    commands::bisect::handle_status(session_id).await?;
                }
                BisectAction::History { session_id } => {
                    commands::bisect::handle_history(session_id).await?;
                }
                BisectAction::Abort { session_id } => {
                    commands::bisect::handle_abort(session_id).await?;
                }
            },
            Commands::Deploy { profile, game } => commands::deploy::handle(profile, game).await?,
            Commands::HotDeploy {
                profile,
                game,
                mod_id,
                enable,
                disable,
                dry_run,
                force,
            } => {
                commands::hot_deploy::handle(
                    profile, game, mod_id, enable, disable, dry_run, force,
                )
                .await?;
            }
            Commands::Collisions {
                profile,
                game,
                all,
                suggest_hides,
            } => commands::collisions::handle(profile, game, all, suggest_hides).await?,
            Commands::Rollback { profile, game } => {
                commands::rollback::handle(profile, game).await?;
            }
            Commands::Install { source } => commands::install::handle(source).await?,
            Commands::Mod { action } => match action {
                ModAction::Remove {
                    mod_id,
                    profile,
                    dry_run,
                    force_contaminate,
                } => {
                    commands::uninstall::handle(mod_id, profile, dry_run, force_contaminate)
                        .await?;
                }
                ModAction::Diagnose { mod_id } => {
                    commands::uninstall::handle_diagnose(mod_id).await?;
                }
            },
            Commands::Verify { profile, game } => commands::verify::handle(profile, game).await?,
            Commands::Nexus { action } => commands::nexus::handle(action).await?,
            Commands::Stock { action } => commands::stock::handle(action).await?,
            Commands::Save { action } => commands::save::handle(action).await?,
            Commands::Game {
                action:
                    GameAction::Add {
                        id,
                        display_name,
                        executable_dir,
                        steam_app_id,
                        install_dir_name,
                        mod_dir,
                        nexus_domain,
                        proxy_dlls,
                        force,
                    },
            } => {
                commands::game::handle_add(commands::game::AddGameArgs {
                    id,
                    display_name,
                    executable_dir,
                    steam_app_id,
                    install_dir_name,
                    mod_dir,
                    nexus_domain,
                    proxy_dlls,
                    force,
                })
                .await?;
            }
            Commands::Update { action } => match action {
                UpdateAction::Check {
                    profile,
                    game,
                    period,
                    mods,
                } => {
                    if mods || profile.is_some() || game.is_some() {
                        commands::update::handle_check(profile, game, period).await?;
                    } else {
                        commands::update::handle_product_check().await?;
                    }
                }
                UpdateAction::Apply {
                    profile,
                    game,
                    period,
                    dry_run,
                    confirm_locked,
                    accept_breaking,
                    yes,
                } => {
                    commands::update::handle_apply(commands::update::ApplyOptions {
                        profile_name: profile,
                        game_id: game,
                        period,
                        dry_run,
                        safety: commands::update::ApplySafety {
                            confirm_locked,
                            accept_breaking,
                            yes,
                        },
                    })
                    .await?;
                }
            },
            Commands::Tool { action } => match action {
                ToolAction::Run {
                    executable,
                    profile,
                    game,
                    args,
                } => commands::tool::handle_run(executable, args, profile, game).await?,
                ToolAction::AddExecutable {
                    name,
                    executable,
                    game,
                    working_dir,
                    output_mod,
                    wine_dll_overrides,
                    environment,
                    args,
                } => {
                    commands::tool::handle_add_executable(
                        &name,
                        executable,
                        &game,
                        working_dir,
                        &output_mod,
                        wine_dll_overrides,
                        &environment,
                        &args,
                    )
                    .await?;
                }
                ToolAction::ListExecutables { game } => {
                    commands::tool::handle_list_executables(&game).await?;
                }
                ToolAction::RemoveExecutable { name, game } => {
                    commands::tool::handle_remove_executable(&name, &game).await?;
                }
                ToolAction::RunExecutable {
                    name,
                    game,
                    profile,
                    args,
                } => {
                    commands::tool::handle_run_executable(&name, &game, profile, args).await?;
                }
                ToolAction::Releases { tool_id, game } => {
                    commands::tool::handle_releases(&tool_id, &game).await?;
                }
                ToolAction::InstallRelease {
                    tool_id,
                    game,
                    tag,
                    asset,
                } => {
                    commands::tool::handle_install_release(&tool_id, &game, &tag, &asset).await?;
                }
                ToolAction::InstallReleaseFromPath {
                    tool_id,
                    game,
                    tag,
                    asset,
                    path,
                } => {
                    commands::tool::handle_install_release_from_path(
                        &tool_id, &game, &tag, &asset, path,
                    )
                    .await?;
                }
                ToolAction::List { .. }
                | ToolAction::Status { .. }
                | ToolAction::Show { .. }
                | ToolAction::Diagnose { .. }
                | ToolAction::Doctor { .. }
                | ToolAction::Settings { .. }
                | ToolAction::Profiles { .. }
                | ToolAction::Sources { .. }
                | ToolAction::Enable { .. }
                | ToolAction::Disable { .. }
                | ToolAction::Configure { .. }
                | ToolAction::Setup { .. }
                | ToolAction::Apply { .. }
                | ToolAction::Preview { .. }
                | ToolAction::Revert { .. } => {
                    unreachable!(
                        "these ToolAction variants are dispatched in the pre-tokio sync block"
                    )
                }
            },
            Commands::Patcher { action } => match action {
                PatcherAction::List { profile, game } => {
                    commands::patcher::handle_list(profile, game).await?;
                }
                PatcherAction::AddSynthesis {
                    name,
                    profile,
                    game,
                    executable,
                    pipeline_settings,
                    synthesis_profile,
                    output_mod,
                    order,
                    timeout_seconds,
                } => {
                    commands::patcher::handle_add_synthesis(
                        &name,
                        profile,
                        game,
                        executable,
                        pipeline_settings,
                        synthesis_profile,
                        output_mod,
                        order,
                        timeout_seconds,
                    )
                    .await?;
                }
                PatcherAction::AddCommand {
                    name,
                    profile,
                    game,
                    executable,
                    working_dir,
                    args,
                    environment,
                    output_mod,
                    order,
                    timeout_seconds,
                } => {
                    commands::patcher::handle_add_command(
                        &name,
                        profile,
                        game,
                        executable,
                        working_dir,
                        args,
                        environment,
                        output_mod,
                        order,
                        timeout_seconds,
                    )
                    .await?;
                }
                PatcherAction::Remove {
                    name,
                    profile,
                    game,
                } => commands::patcher::handle_remove(&name, profile, game).await?,
                PatcherAction::Enable {
                    name,
                    profile,
                    game,
                } => commands::patcher::handle_set_enabled(&name, profile, game, true).await?,
                PatcherAction::Disable {
                    name,
                    profile,
                    game,
                } => commands::patcher::handle_set_enabled(&name, profile, game, false).await?,
                PatcherAction::Reorder {
                    profile,
                    game,
                    names,
                } => commands::patcher::handle_reorder(profile, game, names).await?,
                PatcherAction::Validate { profile, game } => {
                    commands::patcher::handle_validate(profile, game).await?;
                }
                PatcherAction::Run {
                    name,
                    profile,
                    game,
                } => {
                    if let Some(name) = name {
                        commands::patcher::handle_run_stage(&name, profile, game).await?;
                    } else {
                        commands::patcher::handle_run(profile, game).await?;
                    }
                }
            },
            Commands::Nxm { action } => match action {
                NxmAction::Handle { uri, profile } => commands::nxm::handle(uri, profile).await?,
                NxmAction::Install => {
                    unreachable!("NxmAction::Install is dispatched in the pre-tokio sync block")
                }
            },
            Commands::Exec { action } => match action {
                ExecAction::Run {
                    name,
                    game,
                    profile,
                    args,
                } => {
                    commands::tool::handle_run_executable(&name, &game, profile, args).await?;
                }
                // Sync arms handled in the pre-tokio block above.
                ExecAction::Add { .. } | ExecAction::List { .. } | ExecAction::Remove { .. } => {
                    unreachable!("Exec sync arms are dispatched in the pre-tokio block above")
                }
            },
            Commands::Wabbajack { action } => commands::wabbajack::handle(action).await?,
            Commands::Lock { action } => commands::lockfile::handle(action).await?,
            // Already handled above
            Commands::Profile { .. }
            | Commands::Config { .. }
            | Commands::Dev { .. }
            | Commands::Scan { .. }
            | Commands::Detect
            | Commands::Game { .. }
            | Commands::Import
            | Commands::Instance { .. }
            | Commands::Backup { .. }
            | Commands::Diagnostics { .. }
            | Commands::Crash { .. }
            | Commands::Doctor { .. }
            | Commands::Export { .. }
            | Commands::Fomod { .. }
            | Commands::Skill { .. } => {
                unreachable!("these commands are dispatched before the async runtime block")
            }
            #[cfg(feature = "gui")]
            Commands::Gui => unreachable!("GUI is dispatched before the async runtime block"),
            #[cfg(feature = "bethesda")]
            Commands::Loot { .. } => unreachable!("LOOT is dispatched before the async runtime block"),
        }
        Ok(())
    })
}
