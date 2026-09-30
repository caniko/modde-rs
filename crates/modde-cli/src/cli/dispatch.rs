#![allow(clippy::wildcard_imports)]
//! Top-level CLI command dispatcher.

use anyhow::Result;

use crate::commands;

use super::args::*;

mod async_dispatch;

pub(crate) fn run_command(cli: Cli) -> Result<()> {
    // Commands that touch the (async) database are dispatched through a tokio
    // runtime via `block_on`. Purely-synchronous commands (no DB, no network)
    // still run directly without a runtime. Only the single matched arm runs,
    // so at most one runtime is created per invocation.
    macro_rules! db_sync {
        ($fut:expr) => {
            return tokio::runtime::Runtime::new()?.block_on($fut)
        };
    }

    match cli.command {
        Commands::Dev {
            action: DevAction::Completions { shell },
        } => return commands::completions::handle_completions(&shell),
        Commands::Dev {
            action: DevAction::ExportToolSchema { out },
        } => return commands::nix_schema::handle_export(&out),
        #[cfg(feature = "screenshot")]
        Commands::Dev {
            action:
                DevAction::Screenshot {
                    screen,
                    out,
                    all,
                    width,
                    height,
                    scale,
                    theme,
                },
        } => {
            let opts = modde_ui::screenshot::ShotOptions {
                width,
                height,
                scale,
                theme,
            };
            if all {
                for shot in modde_ui::screenshot::all_screens() {
                    let path = out.join(format!("{}.png", shot.as_str()));
                    modde_ui::screenshot::capture_to_png(*shot, &opts, &path)?;
                    println!("wrote {}", path.display());
                }
            } else {
                modde_ui::screenshot::capture_to_png(screen.into(), &opts, &out)?;
                println!("wrote {}", out.display());
            }
            return Ok(());
        }
        #[cfg(feature = "screenshot")]
        Commands::Dev {
            action:
                DevAction::Visual {
                    output,
                    manifest,
                    report,
                    rubric_bin,
                    preset,
                    width,
                    height,
                    scale,
                    theme,
                },
        } => {
            return commands::visual::run(commands::visual::VisualArgs {
                output,
                manifest,
                report,
                rubric_bin,
                preset,
                width,
                height,
                scale,
                theme,
            });
        }
        Commands::Config { action } => {
            return match action {
                ConfigAction::Show => commands::config::handle_show(),
                ConfigAction::Test => db_sync!(commands::config::handle_test()),
                ConfigAction::SetDatabase {
                    backend,
                    url,
                    host,
                    port,
                    dbname,
                    user,
                    password_file,
                    clear,
                } => commands::config::handle_set_database(
                    &backend,
                    url,
                    host,
                    port,
                    dbname,
                    user,
                    password_file,
                    &clear,
                ),
                ConfigAction::ResetDatabase => commands::config::handle_reset_database(),
            };
        }
        Commands::Profile { action } => db_sync!(commands::profile::handle(action)),
        Commands::Scan {
            game,
            game_dir,
            manifest,
            import_to,
            threshold,
            dry_run,
            prune_duplicates,
        } => {
            db_sync!(commands::scan::handle(
                game,
                game_dir,
                manifest,
                import_to,
                threshold,
                dry_run,
                prune_duplicates,
            ));
        }
        Commands::Diagnostics { game, profile } => {
            eprintln!("warning: `modde diagnostics` is deprecated; use `modde doctor profile`");
            db_sync!(commands::doctor::handle_profile(game, profile, false));
        }
        Commands::Crash {
            action:
                CrashAction::Analyze {
                    log_path,
                    game,
                    profile,
                    format,
                    json,
                },
        } => {
            eprintln!("warning: `modde crash analyze` is deprecated; use `modde doctor crash`");
            db_sync!(commands::doctor::handle_crash(
                log_path, game, profile, format, json
            ));
        }
        Commands::Doctor { action } => match action {
            DoctorAction::Profile {
                game,
                profile,
                json,
            } => {
                db_sync!(commands::doctor::handle_profile(game, profile, json));
            }
            DoctorAction::Crash {
                log_path,
                game,
                profile,
                format,
                json,
            } => {
                db_sync!(commands::doctor::handle_crash(
                    log_path, game, profile, format, json
                ));
            }
            DoctorAction::Explain {
                log_path,
                game,
                profile,
                provider,
                json,
            } => {
                db_sync!(commands::doctor::handle_explain(
                    log_path, game, profile, provider, json
                ));
            }
        },
        Commands::Export {
            profile,
            game,
            columns,
            output,
        } => {
            db_sync!(commands::export::handle(profile, game, columns, output));
        }
        Commands::Backup { action } => {
            db_sync!(commands::backup::handle(action));
        }
        Commands::Detect => return commands::detect::handle(),
        Commands::Game {
            action: GameAction::List,
        } => return commands::game::handle_list(),
        Commands::Game {
            action: GameAction::Remove { id, yes },
        } => return commands::game::handle_remove(&id, yes),
        Commands::Game {
            action: GameAction::Detect { install_path },
        } => return commands::game::handle_detect(install_path),
        Commands::Game {
            action: GameAction::Show { id },
        } => return commands::game::handle_show(&id),
        Commands::Game {
            action:
                GameAction::Export {
                    id,
                    with_optiscaler,
                    output,
                },
        } => return commands::game::handle_export(&id, with_optiscaler, output),
        Commands::Game {
            action: GameAction::Import { path, force },
        } => return commands::game::handle_import(path, force),
        Commands::Game {
            action: GameAction::ImportProfile { path, game, force },
        } => return commands::game::handle_import_profile(path, game, force),
        Commands::Instance { action } => {
            return match action {
                InstanceAction::Create { name, data_dir } => {
                    commands::instance::handle_create(&name, data_dir)
                }
                InstanceAction::List => commands::instance::handle_list(),
                InstanceAction::Switch { name } => commands::instance::handle_switch(&name),
            };
        }
        Commands::Import => db_sync!(commands::import::handle()),
        Commands::Fomod { action } => return commands::fomod::handle(action),
        #[cfg(feature = "bethesda")]
        Commands::Loot { action } => {
            return match action {
                LootAction::Sort { game, data_dir } => commands::loot::handle_sort(&game, data_dir),
                LootAction::Validate { game } => commands::loot::handle_validate(&game),
            };
        }
        Commands::Tool {
            action: ToolAction::List { game },
        } => {
            db_sync!(commands::tool::handle_list(&game));
        }
        Commands::Tool {
            action: ToolAction::Status { game },
        } => {
            db_sync!(commands::tool::handle_status(&game));
        }
        Commands::Tool {
            action:
                ToolAction::Show {
                    tool_id,
                    game,
                    json,
                },
        } => {
            db_sync!(commands::tool::handle_show(&tool_id, &game, json));
        }
        Commands::Tool {
            action:
                ToolAction::Diagnose {
                    tool_id,
                    game,
                    json,
                },
        } => {
            db_sync!(commands::tool::handle_diagnose(&tool_id, &game, json));
        }
        Commands::Tool {
            action:
                ToolAction::Doctor {
                    tool_id,
                    game,
                    json,
                    fix,
                },
        } => {
            db_sync!(commands::tool::handle_doctor(
                tool_id.as_deref(),
                &game,
                json,
                fix,
            ));
        }
        Commands::Tool {
            action:
                ToolAction::Settings {
                    tool_id,
                    game,
                    json,
                },
        } => {
            db_sync!(commands::tool::handle_settings(&tool_id, &game, json));
        }
        Commands::Tool {
            action:
                ToolAction::Profiles {
                    tool_id,
                    game,
                    json,
                },
        } => {
            db_sync!(commands::tool::handle_profiles(&tool_id, &game, json));
        }
        Commands::Tool {
            action:
                ToolAction::Sources {
                    tool_id,
                    game,
                    json,
                },
        } => {
            db_sync!(commands::tool::handle_sources(&tool_id, &game, json));
        }
        Commands::Tool {
            action: ToolAction::Enable { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_enable(&tool_id, &game));
        }
        Commands::Tool {
            action: ToolAction::Disable { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_disable(&tool_id, &game));
        }
        Commands::Tool {
            action:
                ToolAction::Configure {
                    tool_id,
                    game,
                    settings,
                    reset_keys,
                },
        } => {
            db_sync!(commands::tool::handle_configure(
                &tool_id,
                &game,
                &settings,
                &reset_keys,
            ));
        }
        Commands::Tool {
            action:
                ToolAction::Setup {
                    tool_id,
                    game,
                    profile,
                    source,
                    release_tag,
                    release_asset,
                    local_source_dir,
                    hardware_tuning,
                    upgrade,
                    apply,
                    dry_run,
                    yes,
                },
        } => {
            db_sync!(commands::tool::handle_setup(
                commands::tool::ToolSetupOptions {
                    tool_id,
                    game,
                    profile,
                    source,
                    release_tag,
                    release_asset,
                    local_source_dir,
                    hardware_tuning,
                    upgrade,
                    apply,
                    dry_run,
                    yes,
                }
            ));
        }
        Commands::Tool {
            action:
                ToolAction::Apply {
                    tool_id,
                    game,
                    dry_run,
                },
        } => {
            db_sync!(commands::tool::handle_apply(&tool_id, &game, dry_run));
        }
        Commands::Tool {
            action: ToolAction::Preview { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_preview(&tool_id, &game));
        }
        Commands::Tool {
            action: ToolAction::Revert { tool_id, game },
        } => {
            db_sync!(commands::tool::handle_revert(&tool_id, &game));
        }
        Commands::Nxm {
            action: NxmAction::Install,
        } => {
            commands::nxm::install_handler()?;
            return Ok(());
        }
        Commands::Exec {
            action:
                ExecAction::Add {
                    name,
                    executable,
                    game,
                    working_dir,
                    output_mod,
                    wine_dll_overrides,
                    environment,
                    args,
                },
        } => {
            db_sync!(commands::tool::handle_add_executable(
                &name,
                executable,
                &game,
                working_dir,
                &output_mod,
                wine_dll_overrides,
                &environment,
                &args,
            ));
        }
        Commands::Exec {
            action: ExecAction::List { game },
        } => {
            db_sync!(commands::tool::handle_list_executables(&game));
        }
        Commands::Exec {
            action: ExecAction::Remove { name, game },
        } => {
            db_sync!(commands::tool::handle_remove_executable(&name, &game));
        }
        Commands::Skill { action } => {
            return commands::skill::handle(action);
        }
        Commands::Wabbajack { .. } | Commands::Lock { .. } => {}
        _ => {}
    }

    async_dispatch::dispatch_async(cli)
}
