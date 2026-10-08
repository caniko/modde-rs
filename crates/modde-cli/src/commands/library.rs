//! The GUI invokes this CLI entry point too: one profile/deploy/session pipeline.

use anyhow::{Context, Result, bail};
use modde_core::library::{LaunchSettings, LibraryPreferences, PendingSession, SessionPhase};
use modde_core::profile::ProfileManager;
use modde_core::resolver::GameId;
use modde_core::save::SaveManager;
use modde_core::settings::AppSettings;
use modde_games::library::context;
use modde_games::library::{LibraryGame, catalogue, launch};

use crate::cli::args::LibraryAction;

mod hooks;
mod process;

pub(crate) fn supervise(request: &std::path::Path) -> Result<()> {
    process::supervise(request)
}

/// A sibling of the observer, so waiting for the mutation lease cannot keep
/// the game's ExitType=cgroup service alive. The run-directory pin makes late
/// workers harmless after another command has finished or replaced a session.
pub(crate) fn complete_observed(directory: &std::path::Path) -> Result<()> {
    let started = std::time::Instant::now();
    let mut completed_at = None;
    loop {
        let Some(session) = PendingSession::load_blocking()? else {
            return Ok(());
        };
        let Some(observation) = session
            .observation
            .as_ref()
            .filter(|o| o.directory == directory)
        else {
            return Ok(());
        };
        session.require_owner()?;
        if session.phase.is_preparation() {
            return Ok(());
        }
        let evidence = process::evidence(observation)?;
        if evidence.as_ref().is_some_and(|e| e.completed && e.started) {
            let completed_at = completed_at.get_or_insert_with(std::time::Instant::now);
            if let Ok(_mutation) = modde_core::library::mutation_lock() {
                // Reload under ownership: the parent may have captured while
                // we were acquiring its lease.
                let Some(current) = PendingSession::load_blocking()? else {
                    return Ok(());
                };
                if current
                    .observation
                    .as_ref()
                    .is_none_or(|o| o.directory != directory)
                {
                    return Ok(());
                }
                match process::require_exit(&current, false) {
                    Ok(_observer) => {
                        tokio::runtime::Runtime::new()?.block_on(finish_loaded(&current, false))?;
                        let _ = modde_core::ipc::notify_refresh();
                        return Ok(());
                    }
                    Err(error) if completed_at.elapsed() >= std::time::Duration::from_secs(30) => {
                        return Err(error);
                    }
                    Err(_) => {}
                }
            }
        } else if evidence.is_some() {
            if modde_core::library::lock_file(&directory.join("observer.lock")).is_ok() {
                bail!(
                    "observation stopped without a completed game session; inspect `modde library status`"
                );
            }
        } else if started.elapsed() >= std::time::Duration::from_secs(60) {
            bail!("supervisor did not start; pending session retained");
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

pub(crate) fn pending_store_uri() -> Result<Option<String>> {
    let Some(session) = PendingSession::load()? else {
        return Ok(None);
    };
    if !matches!(
        session.phase,
        SessionPhase::AwaitingStore | SessionPhase::Requested
    ) {
        return Ok(None);
    }
    session.require_owner()?;
    let Some(request) = session.launch_request else {
        return Ok(None);
    };
    Ok(serde_json::from_value::<PlayOptions>(request)?.store_uri)
}

pub(crate) async fn handle(action: LibraryAction) -> Result<()> {
    match action {
        LibraryAction::List { json } => {
            let catalogue = catalogue(&AppSettings::load())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "games": catalogue.games, "notices": catalogue.notices,
                    }))?
                );
            } else {
                for game in catalogue.games {
                    println!(
                        "{}\t{}\t{}\t{}",
                        game.id,
                        game.store.label(),
                        if game.install_path.is_some() {
                            "installed"
                        } else {
                            "owned / uninstalled"
                        },
                        game.name
                    );
                }
                for notice in catalogue.notices {
                    eprintln!("{notice}");
                }
            }
        }
        LibraryAction::SyncSteam { steam_id } => {
            let key = std::env::var("MODDE_STEAM_API_KEY")
                .context("set MODDE_STEAM_API_KEY before syncing Steam")?;
            let count = modde_games::library::sync_steam(&steam_id, &key).await?;
            println!("Synced {count} owned Steam games.");
        }
        LibraryAction::Play { id } => {
            check_outcome(play(&id, PlayOptions::default()).await?)?;
        }
        LibraryAction::Finish {
            confirm_exited,
            skip_analysis,
        } => finish(confirm_exited, skip_analysis).await?,
        LibraryAction::Status => {
            let session = PendingSession::load_blocking()?;
            let evidence = session
                .as_ref()
                .and_then(|s| s.observation.as_ref())
                .map(process::evidence)
                .transpose()?
                .flatten();
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"session": session, "process": evidence})
                )?
            );
        }
        LibraryAction::Hook { id } => println!("{}", hooks::install(&find(&id)?)?),
        LibraryAction::Wrap { id, command } => {
            anyhow::ensure!(
                PendingSession::load_completion()?.is_none(),
                "complete the previous session with `modde library finish` before launching"
            );
            let mut options: PlayOptions = if let Some(session) = PendingSession::load()? {
                session.require_owner()?;
                anyhow::ensure!(
                    session.installation == id && session.phase == SessionPhase::AwaitingStore,
                    "another session is pending; refusing this store command"
                );
                serde_json::from_value(
                    session
                        .launch_request
                        .context("store request is missing options")?,
                )?
            } else {
                PlayOptions::default()
            };
            options.store_uri = None;
            check_outcome(play_inner(&id, options, Some(command)).await?)?;
        }
        LibraryAction::Supervise { request } => process::supervise(&request)?,
        LibraryAction::CompleteObserved { observation } => complete_observed(&observation)?,
        LibraryAction::ManagerWrap {
            id,
            root,
            prefix,
            inherit_env,
            command,
        } => manager_launch(id, root, prefix, inherit_env, command).await?,
        LibraryAction::Recover => recover().await?,
        LibraryAction::Install { id } => {
            launch::install(&find(&id)?)?;
            println!("Installation requested in the store. Refresh Library when it completes.");
        }
        LibraryAction::Configure { id, file } => {
            let game = find(&id)?;
            if game.install_path.is_none() {
                bail!("launch settings belong to an installed copy");
            }
            if let Some(file) = file {
                let settings: LaunchSettings = serde_json::from_slice(&std::fs::read(file)?)?;
                let games = catalogue(&AppSettings::load())?.games;
                launch::validate(&game, &settings, &games)?;
                let mut preferences = LibraryPreferences::load()?;
                preferences.launches.insert(id.clone(), settings.clone());
                context::effective_settings(&game, &games, &preferences)?;
                LibraryPreferences::update(|preferences| {
                    preferences.launches.insert(id, settings);
                })?;
                println!("Launch settings saved.");
            } else {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&LibraryPreferences::load()?.launch_for(&id))?
                );
            }
        }
        LibraryAction::Favorite { id, remove } => {
            let entitlement = find(&id)?.entitlement;
            LibraryPreferences::update(|preferences| {
                if remove {
                    preferences.favorites.remove(&entitlement);
                } else {
                    preferences.favorites.insert(entitlement);
                }
            })?;
        }
        LibraryAction::Adopt { id, profile } => adopt(&id, &profile).await?,
    }
    Ok(())
}

fn find(id: &str) -> Result<LibraryGame> {
    catalogue(&AppSettings::load())?
        .games
        .into_iter()
        .find(|game| game.id == id)
        .with_context(|| format!("library entry {id} is no longer available; refresh the library"))
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct PlayOptions {
    pub profile: Option<String>,
    pub no_deploy: bool,
    pub no_switch: bool,
    pub no_capture: bool,
    pub environment: std::collections::BTreeMap<String, String>,
    pub writable: Vec<std::path::PathBuf>,
    pub sandbox: Option<bool>,
    pub require_observed: bool,
    pub performance: Option<super::perf::Capture>,
    pub bisect: Option<super::bisect::Completion>,
    /// Perf/bisect may not silently change save scope at a provider boundary.
    pub expected_scope: Option<GameId>,
    /// Deferred until the command's mutation lease has been released.
    pub store_uri: Option<String>,
}

pub(super) async fn play(id: &str, options: PlayOptions) -> Result<launch::LaunchOutcome> {
    play_inner(id, options, None).await
}

pub(super) fn check_outcome(outcome: launch::LaunchOutcome) -> Result<()> {
    if let launch::LaunchOutcome::Exited(status) = outcome {
        anyhow::ensure!(status.success(), "game exited with {status}");
    }
    Ok(())
}

async fn play_inner(
    id: &str,
    mut options: PlayOptions,
    boundary: Option<Vec<std::ffi::OsString>>,
) -> Result<launch::LaunchOutcome> {
    if boundary.is_none()
        && let Some(session) = PendingSession::load_blocking()?
    {
        bail!(
            "{} has an unfinished session; use `modde library finish` after it exits",
            session.name
        );
    }
    let games = catalogue(&AppSettings::load())?.games;
    let game = games
        .iter()
        .find(|game| game.id == id)
        .context("installation is no longer available")?;
    let pm = ProfileManager::open().await?;
    let mut runtime_settings = LibraryPreferences::load()?.launch_for(id);
    runtime_settings
        .environment
        .extend(options.environment.clone());
    runtime_settings
        .sandbox
        .writable
        .extend(options.writable.clone());
    if let Some(enabled) = options.sandbox {
        runtime_settings.sandbox.enabled = enabled;
    }
    let boundary_prefix = if let Some(command) = &boundary {
        let prefix = context::installation_prefix(game, &runtime_settings)?;
        let prefix =
            hooks::prepare_boundary(game, &mut runtime_settings, prefix.as_deref(), command)?;
        launch::validate_boundary(command, &runtime_settings)?;
        Some(prefix)
    } else {
        None
    };
    let context = match boundary_prefix {
        Some(prefix) => {
            context::with_store_settings(game, &games, pm.db(), runtime_settings, prefix).await?
        }
        None => context::with_launch_settings(game, &games, pm.db(), runtime_settings).await?,
    };
    let scope = context.saves.scope;
    anyhow::ensure!(
        options
            .expected_scope
            .as_ref()
            .is_none_or(|expected| expected == &scope),
        "provider launch resolved a different save destination; save its prefix/HOME configuration before starting this experiment"
    );
    let mut settings = context.launch;
    if settings.executable.is_some() && settings.runner.is_some() {
        settings.prefix = context.prefix.clone();
    }
    let current = pm.db().get_active_profile(&scope).await?;
    let profile_name = selected_profile(
        &settings,
        options.profile.as_deref(),
        current.as_ref().map(|(_, name)| name.as_str()),
    );
    if settings.executable.is_none()
        && settings.store_hook
        && profile_name.is_some()
        && game
            .game_id
            .as_deref()
            .and_then(modde_games::resolve_game_plugin)
            .is_some_and(|plugin| plugin.supports_save_profiles())
    {
        // Check before dispatch, as well as at a directly invoked store hook.
        hooks::require_profile_save_boundary(game)?;
    }
    if boundary.is_none() && settings.executable.is_none() && settings.store_hook {
        launch::validate(game, &settings, &games)?;
        let mut session = PendingSession::for_save_operation(
            &pm,
            &modde_core::library::SaveContext {
                game_id: GameId::from(game.game_id.as_deref().unwrap_or("unmanaged")),
                scope: scope.clone(),
                directory: settings.save_directory.clone(),
            },
        )
        .await?;
        session.installation = id.to_string();
        session.name.clone_from(&game.name);
        session.install_path.clone_from(&game.install_path);
        session.prefix.clone_from(&context.prefix);
        session.phase = SessionPhase::AwaitingStore;
        options.store_uri = Some(launch::store_uri(game, false)?);
        session.launch_request = Some(serde_json::to_value(&options)?);
        session.save()?;
        println!(
            "{}: waiting for the store wrapper to start the game",
            game.name
        );
        return Ok(launch::LaunchOutcome::Requested);
    }
    if options.require_observed && boundary.is_none() && settings.executable.is_none() {
        bail!("this operation needs a direct executable or a configured store wrapper");
    }
    if profile_name.is_some() && boundary.is_none() && settings.executable.is_none() {
        bail!(
            "managed store launches need the game-command wrapper; run `modde library hook {id}` first"
        );
    }
    hooks::wait_for_idle_prefix(context.prefix.as_deref())?;
    if LibraryPreferences::load()?.needs_deploy.contains(id)
        && (options.no_deploy || profile_name.is_none())
    {
        bail!(
            "interrupted deployment requires a named or active profile and a full deploy before launching this installation"
        );
    }
    let mut resource_settings = settings.clone();
    resource_settings.prefix = context.prefix.clone();
    let _locks: Vec<_> = launch::lock_paths(game, &resource_settings)
        .iter()
        .map(|path| modde_core::library::lock_file(path))
        .collect::<Result<_>>()?;
    let profile = if let Some(profile_name) = &profile_name {
        let game_id = game
            .game_id
            .as_deref()
            .context("a mod profile requires a registered game plugin")?;
        let game_key = GameId::from(game_id);
        let profile = pm.load(profile_name, Some(&game_key)).await?;
        let profile_id = profile.id.context("profile has no database ID")?;
        if super::supports_save_profiles(game_id)? && settings.save_directory.is_none() {
            bail!(
                "configure save_directory for this installation before launching a save-managed profile"
            );
        }
        if !super::supports_save_profiles(game_id)? {
            settings.save_directory = None;
        }
        let sm = SaveManager::new(pm.db());
        let switching = current
            .as_ref()
            .is_none_or(|(active_id, _)| *active_id != profile_id);
        if options.no_switch && switching {
            bail!(
                "--no-switch requires the requested profile to already be active for this installation"
            );
        }
        if switching && let Some(dir) = &settings.save_directory {
            if sm.detect_unadopted(&scope, dir).await?.is_some() {
                bail!(
                    "existing saves need adoption: modde library adopt {id} --profile {profile_name}"
                );
            }
            if current.is_some() && !dir.is_dir() {
                bail!("active profile's save directory is missing; restore it before switching");
            }
        }
        if !options.no_deploy {
            super::deploy::validate_at(
                &pm,
                &profile,
                game.install_path
                    .as_deref()
                    .context("installation missing")?,
                context.prefix.as_deref(),
                true,
            )
            .await?;
        }
        if settings.executable.is_some() || boundary.is_some() {
            let mut environment: std::collections::BTreeMap<_, _> =
                modde_games::launcher::collect_tool_env_vars(&game_key, pm.db())
                    .await?
                    .into_iter()
                    .collect();
            let plugin =
                modde_games::resolve_game_plugin(game_id).context("game plugin unavailable")?;
            let mut overrides = plugin.wine_dll_overrides(
                game.install_path
                    .as_deref()
                    .context("installation missing")?,
            );
            overrides.extend(
                plugin.wine_dll_overrides_from_staging(&ProfileManager::staging_dir(profile_name)),
            );
            for entry in profile.mods.iter().filter(|entry| entry.enabled) {
                overrides.extend(plugin.wine_dll_overrides_from_staging(
                    &modde_core::paths::store_dir().join(&entry.mod_id),
                ));
            }
            overrides.extend(
                modde_games::launcher::collect_tool_dll_overrides(&game_key, pm.db()).await?,
            );
            environment.extend(settings.environment.clone());
            overrides.sort();
            overrides.dedup();
            if !overrides.is_empty() {
                let value = environment.entry("WINEDLLOVERRIDES".into()).or_default();
                if !value.is_empty() {
                    value.push(';');
                }
                value.push_str(
                    &overrides
                        .iter()
                        .map(|dll| format!("{dll}=n,b"))
                        .collect::<Vec<_>>()
                        .join(";"),
                );
            }
            settings.environment = environment;
            let wrappers = modde_games::launcher::collect_tool_wrappers(&game_key, pm.db()).await?;
            for wrapper in wrappers {
                let mut argv = vec![wrapper.exe];
                // Tool wrappers currently produce unquoted generated flags. Do
                // not guess shell semantics if a future tool changes that contract.
                if wrapper.args.contains(['\'', '"', '\\']) {
                    bail!("tool wrapper arguments require an explicit argv in launch settings");
                }
                argv.extend(wrapper.args.split_whitespace().map(str::to_string));
                settings.wrappers.push(argv);
            }
        }
        Some(profile)
    } else {
        None
    };
    // Includes generated wrappers, DLLs, arguments, mounts and namespace checks.
    launch::validate(game, &settings, &games)?;
    let mut session = PendingSession {
        installation: id.to_string(),
        name: game.name.clone(),
        game_id: game.game_id.clone(),
        scope: scope.to_string(),
        profile: profile_name.clone(),
        save_directory: settings
            .save_directory
            .as_deref()
            .map(modde_core::library::normalized_path),
        capture: !options.no_capture,
        phase: SessionPhase::Preparing,
        previous_profile: current.clone(),
        switched_profile: false,
        deployment_started: false,
        data_directory: Some(modde_core::library::normalized_path(
            &modde_core::paths::modde_data_dir(),
        )),
        install_path: game.install_path.clone(),
        prefix: context.prefix,
        save_transition: None,
        observation: None,
        launch_request: Some(serde_json::to_value(&options)?),
    };
    session.save()?;
    if let Some(capture) = &options.performance {
        modde_core::library::atomic_json(
            &capture.directory.join("effective-settings.json"),
            &settings,
        )?;
    }
    if let Some(profile) = &profile {
        modde_games::launcher::generate_tool_configs(&profile.game_id, pm.db()).await?;
    }
    if let Some(prefix) = &settings.prefix {
        std::fs::create_dir_all(prefix).context("creating Wine prefix directory")?;
    }
    if settings.sandbox.enabled
        && let Some(directory) = &settings.save_directory
    {
        // A first-run save destination must exist before bubblewrap can bind it.
        // The preparation journal already exists; no save contents are changed.
        std::fs::create_dir_all(directory).context("creating the sandbox save directory")?;
    }
    let mut prepared = if let Some(command) = &boundary {
        launch::PreparedLaunch::Direct(launch::prepare_boundary(game, &settings, &games, command)?)
    } else {
        launch::prepare(game, &settings, &games)?
    };
    if let Some(profile) = &profile {
        if !options.no_deploy {
            session.deployment_started = true;
            session.save()?;
            super::deploy::handle_at(
                Some(profile.name.clone()),
                Some(profile.game_id.to_string()),
                true,
                game.install_path.as_deref(),
                false,
                session.prefix.as_deref(),
            )
            .await?;
        }
        let profile_id = profile.id.context("profile has no database ID")?;
        if !options.no_switch
            && current
                .as_ref()
                .is_none_or(|(active_id, _)| *active_id != profile_id)
        {
            let fingerprint = if let Some((_, current_name)) = &current {
                super::compute_fingerprint(&pm, current_name, profile.game_id.as_str()).await
            } else {
                None
            };
            session
                .switch_profile(
                    &pm,
                    profile,
                    fingerprint.as_ref(),
                    modde_core::library::ExperimentChange::Keep,
                )
                .await?;
        }
        if options.no_deploy && boundary.is_some() {
            hooks::restore_deployed_dlls(
                profile,
                game.install_path
                    .as_deref()
                    .context("installation missing")?,
            )?;
        }
    }
    // Patchers may have started Wine services since the initial preflight. Such
    // a server cannot be adopted by the new observer's descendant tree.
    hooks::wait_for_idle_prefix(session.prefix.as_deref())?;
    if session.deployment_started {
        // Deployment can retarget executable/wrapper symlinks. Re-resolve the
        // actual command and its sandbox grants after those changes; executing
        // the preflight command could otherwise run the previous mod version.
        prepared = if let Some(command) = &boundary {
            launch::PreparedLaunch::Direct(launch::prepare_boundary(
                game, &settings, &games, command,
            )?)
        } else {
            launch::prepare(game, &settings, &games)?
        };
    }
    session.advance(SessionPhase::Ready)?;
    if session.deployment_started {
        LibraryPreferences::update(|preferences| {
            preferences.needs_deploy.remove(id);
        })?;
    }
    if let Some(completion) = &mut options.bisect {
        completion.started_unix_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis()
                .try_into()?,
        );
    }
    session.launch_request = Some(serde_json::to_value(&options)?);
    session.advance(SessionPhase::Launching)?;
    println!("Starting {}...", game.name);
    let _ = modde_core::ipc::notify_refresh();
    let mut _observer_lease = None;
    let outcome = match prepared {
        launch::PreparedLaunch::Direct(command) => process::run(
            command,
            &mut session,
            options
                .performance
                .as_ref()
                .map(|capture| capture.directory.as_path()),
        )
        .map(|(status, lease)| {
            _observer_lease = lease;
            launch::LaunchOutcome::Exited(status)
        }),
        launch::PreparedLaunch::Store(uri) => {
            options.store_uri = Some(uri);
            session.launch_request = Some(serde_json::to_value(&options)?);
            Ok(launch::LaunchOutcome::Requested)
        }
    };
    let outcome = match outcome {
        Ok(launch::LaunchOutcome::Requested) => {
            session.advance(SessionPhase::Requested)?;
            println!(
                "Launch requested in {}. When the game exits, choose End session & capture (or `modde library finish`).",
                game.store.label()
            );
            launch::LaunchOutcome::Requested
        }
        Ok(launch::LaunchOutcome::Exited(status)) => {
            complete_session(&session, &pm, Some(status), false).await?;
            println!("{} exited; session complete.", game.name);
            launch::LaunchOutcome::Exited(status)
        }
        Err(error) => {
            // A wait/IPC error does not prove that no child was started. Keep
            // recovery state until the user confirms the session has ended.
            return Err(error);
        }
    };
    Ok(outcome)
}

fn selected_profile(
    settings: &LaunchSettings,
    explicit: Option<&str>,
    active: Option<&str>,
) -> Option<String> {
    explicit
        .or(settings.profile.as_deref())
        .or_else(|| {
            if settings.use_active_profile {
                active
            } else {
                None
            }
        })
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::library::LegacySaveBinding;
    use modde_games::library::context::save_scope;

    fn game(id: &str) -> LibraryGame {
        LibraryGame {
            id: id.into(),
            entitlement: "local:example".into(),
            name: "Example".into(),
            game_id: Some("example".into()),
            store: modde_games::library::Store::Local,
            app_id: "example".into(),
            install_path: Some(format!("/games/{id}").into()),
        }
    }

    #[test]
    fn legacy_vault_belongs_only_to_bound_install_and_save_destination() {
        let settings = LaunchSettings {
            save_directory: Some("/saves/a".into()),
            ..LaunchSettings::default()
        };
        let mut preferences = LibraryPreferences::default();
        preferences.legacy_save_bindings.insert(
            "example".into(),
            LegacySaveBinding {
                installation: "a".into(),
                save_directory: settings.save_directory.clone(),
            },
        );
        assert_eq!(
            save_scope(&game("a"), &settings, &preferences).as_str(),
            "example"
        );
        assert_ne!(
            save_scope(&game("b"), &settings, &preferences).as_str(),
            "example"
        );
        let moved = LaunchSettings {
            save_directory: Some("/saves/b".into()),
            ..settings
        };
        assert_ne!(
            save_scope(&game("a"), &moved, &preferences).as_str(),
            "example"
        );
    }

    #[test]
    fn two_installations_never_share_an_unbound_active_profile_slot() {
        let preferences = LibraryPreferences::default();
        let settings = LaunchSettings::default();
        assert_ne!(
            save_scope(&game("a"), &settings, &preferences),
            save_scope(&game("b"), &settings, &preferences)
        );
    }

    #[test]
    fn profile_selection_distinguishes_active_named_and_unmanaged() {
        let mut settings = LaunchSettings::default();
        assert_eq!(
            selected_profile(&settings, None, Some("active")),
            Some("active".into())
        );
        settings.use_active_profile = false;
        assert_eq!(selected_profile(&settings, None, Some("active")), None);
        settings.profile = Some("named".into());
        assert_eq!(
            selected_profile(&settings, None, Some("active")),
            Some("named".into())
        );
        assert_eq!(
            selected_profile(&settings, Some("explicit"), Some("active")),
            Some("explicit".into())
        );
    }
}

pub(super) async fn adopt(id: &str, profile_name: &str) -> Result<()> {
    let game = find(id)?;
    let game_id = game
        .game_id
        .as_deref()
        .context("save adoption needs a mod plugin")?;
    anyhow::ensure!(
        super::supports_save_profiles(game_id)?,
        "this game does not support save profiles"
    );
    let pm = ProfileManager::open().await?;
    let context =
        context::for_installation(&game, &catalogue(&AppSettings::load())?.games, pm.db()).await?;
    let dir = context
        .saves
        .directory
        .as_deref()
        .context("configure save_directory first")?;
    let profile = pm.load(profile_name, Some(&GameId::from(game_id))).await?;
    let scope = context.saves.scope;
    if pm.db().get_active_profile(&scope).await?.is_some() {
        bail!("this installation already has active save state");
    }
    let count = SaveManager::new(pm.db()).adopt(&scope, profile_name, dir)?;
    pm.db()
        .set_active_profile(&scope, profile.id.context("profile ID missing")?)
        .await?;
    LibraryPreferences::update(|preferences| {
        let entry = preferences.launches.entry(id.to_string()).or_default();
        entry.profile = Some(profile_name.to_string());
        entry.save_directory = Some(dir.to_path_buf());
    })?;
    println!("Adopted {count} save files without changing live files.");
    Ok(())
}

async fn finish(confirm_exited: bool, skip_analysis: bool) -> Result<()> {
    let session = PendingSession::load_blocking()?.context("no unfinished game session")?;
    session.require_owner()?;
    let _observer = process::require_exit(&session, confirm_exited)?;
    finish_loaded(&session, skip_analysis).await
}

async fn finish_loaded(session: &PendingSession, skip_analysis: bool) -> Result<()> {
    let pm = ProfileManager::open().await?;
    complete_session(session, &pm, None, skip_analysis).await?;
    println!("{}: session complete.", session.name);
    Ok(())
}

pub(super) fn observed_status(
    session: &PendingSession,
) -> Result<Option<std::process::ExitStatus>> {
    Ok(session
        .observation
        .as_ref()
        .map(process::evidence)
        .transpose()?
        .flatten()
        .filter(|evidence| evidence.completed && evidence.started)
        .and_then(|evidence| evidence.raw_status)
        .map(process::from_raw))
}

async fn complete_session(
    session: &PendingSession,
    pm: &ProfileManager,
    status: Option<std::process::ExitStatus>,
    skip_analysis: bool,
) -> Result<()> {
    let session = capture_session(session, pm).await?;
    session.save_completion()?;
    clear_manager_marker(&session)?;
    process::remove_request(&session)?;
    PendingSession::clear()?;
    if skip_analysis {
        println!(
            "Session finalized. Performance ingestion and bisect results remain available for manual handling."
        );
        return PendingSession::clear_completion();
    }
    // Analysis output must not hold save capture hostage. A failed write keeps
    // the receipt, and --skip-analysis can release it without touching that path.
    let status = match status {
        Some(status) => Some(status),
        None => observed_status(&session)?,
    };
    record_performance_session(&session)?;
    if let Some(request) = &session.launch_request {
        let options: PlayOptions = serde_json::from_value(request.clone())?;
        if let Some(capture) = &options.performance {
            if let Some(status) = status {
                super::perf::complete_capture(pm, capture, status).await?;
            } else {
                println!(
                    "Performance run {} remains pending; ingest its CSV when available.",
                    capture.run_id
                );
            }
        }
        if let Some(completion) = &options.bisect {
            if let Some(status) = status {
                super::bisect::complete_launch(
                    pm,
                    completion,
                    status,
                    options
                        .performance
                        .as_ref()
                        .map(|capture| capture.run_id.as_str()),
                )
                .await?;
            } else {
                println!(
                    "Bisect {} needs a manual result because exit evidence is unavailable.",
                    completion.session
                );
            }
        }
    }
    PendingSession::clear_completion()
}

fn record_performance_session(session: &PendingSession) -> Result<()> {
    let Some(request) = &session.launch_request else {
        return Ok(());
    };
    let options: PlayOptions = serde_json::from_value(request.clone())?;
    if let Some(capture) = options.performance {
        modde_core::library::atomic_json(
            &capture.directory.join("session.json"),
            &serde_json::json!({
                "session": session,
                "process": session.observation.as_ref().map(process::evidence).transpose()?.flatten(),
            }),
        )?;
    }
    Ok(())
}

async fn capture_session(session: &PendingSession, pm: &ProfileManager) -> Result<PendingSession> {
    session.require_owner()?;
    anyhow::ensure!(
        !session.phase.is_preparation(),
        "preparation did not finish; run `modde library recover` instead of capturing partial saves"
    );
    let mut session = session.clone();
    if session.phase == SessionPhase::Captured {
        return Ok(session);
    }
    session.advance(SessionPhase::Capturing)?;
    if session.capture
        && let (Some(profile), Some(dir), Some(game_id)) =
            (&session.profile, &session.save_directory, &session.game_id)
    {
        anyhow::ensure!(
            dir.is_dir(),
            "save directory is missing; session retained until its saves are available: {}",
            dir.display()
        );
        let fingerprint = super::compute_fingerprint(pm, profile, game_id).await;
        SaveManager::new(pm.db())
            .capture_with_fingerprint(
                &GameId::from(session.scope.as_str()),
                profile,
                dir,
                fingerprint.as_ref(),
            )
            .context("save capture failed; session retained for retry")?;
    }
    session.advance(SessionPhase::Captured)?;
    Ok(session)
}

fn manager_marker(session: &PendingSession) -> Option<std::path::PathBuf> {
    session
        .installation
        .starts_with("manager:")
        .then(|| {
            session
                .install_path
                .as_ref()
                .map(|root| root.join(".modde-library-session.json"))
        })
        .flatten()
}

fn clear_manager_marker(session: &PendingSession) -> Result<()> {
    let Some(path) = manager_marker(session) else {
        return Ok(());
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let marker: PendingSession = serde_json::from_slice(&bytes)?;
    anyhow::ensure!(
        marker.installation == session.installation
            && marker.data_directory == session.data_directory,
        "manager session marker belongs to another launch"
    );
    std::fs::remove_file(&path)?;
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        std::fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}

async fn manager_launch(
    id: String,
    root: std::path::PathBuf,
    prefix: std::path::PathBuf,
    inherit_env: Vec<String>,
    command: Vec<std::ffi::OsString>,
) -> Result<()> {
    anyhow::ensure!(
        id.starts_with("manager:"),
        "invalid manager installation ID"
    );
    let pm = ProfileManager::open().await?;
    let mut settings = LibraryPreferences::load()?.launch_for(&id);
    anyhow::ensure!(
        settings.executable.is_none() && settings.runner.is_none() && settings.profile.is_none(),
        "manager owns the validated executable, runner and profile; configure wrappers, environment and sandbox only"
    );
    anyhow::ensure!(
        settings
            .prefix
            .as_deref()
            .is_none_or(|p| modde_core::library::normalized_path(p)
                == modde_core::library::normalized_path(&prefix)),
        "saved prefix differs from the manager's validated prefix"
    );
    let mut inherited = std::collections::BTreeMap::new();
    for key in inherit_env {
        inherited.insert(
            key.clone(),
            std::env::var(&key).with_context(|| format!("manager environment {key} is missing"))?,
        );
    }
    inherited.extend(settings.environment);
    settings.environment = inherited;
    settings.prefix = Some(prefix.clone());
    let [runner, executable] = command.as_slice() else {
        bail!("manager must supply its validated runner and executable");
    };
    settings.runner = Some(runner.into());
    settings.executable = Some(executable.into());
    if settings.working_directory.is_none() {
        settings.working_directory = Some(std::env::current_dir()?);
    }
    let mut game = LibraryGame::new(
        modde_games::library::Store::Local,
        id.clone(),
        id.clone(),
        Some(root),
    );
    game.id.clone_from(&id);
    game.game_id = None;
    hooks::wait_for_idle_prefix(Some(&prefix))?;
    let _locks: Vec<_> = launch::lock_paths(&game, &settings)
        .iter()
        .map(|path| modde_core::library::lock_file(path))
        .collect::<Result<_>>()?;
    launch::validate(&game, &settings, std::slice::from_ref(&game))?;
    let mut session = PendingSession::for_save_operation(
        &pm,
        &modde_core::library::SaveContext {
            game_id: GameId::from("unmanaged"),
            scope: GameId::from(id.as_str()),
            directory: None,
        },
    )
    .await?;
    session.installation = id;
    session.name.clone_from(&game.name);
    session.install_path.clone_from(&game.install_path);
    session.prefix = Some(prefix);
    if let Some(path) = manager_marker(&session) {
        match path.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("reading the manager session marker"),
            Ok(_) => bail!(
                "this manager installation already has a session marker; finish or recover its original Library session"
            ),
        }
    }
    session.save()?;
    // The manager's root lease lasts while its bridge process is alive. This
    // durable marker also blocks manager mutations after a bridge interruption.
    if let Some(path) = manager_marker(&session) {
        modde_core::library::atomic_json(&path, &session)?;
    }
    let command =
        launch::prepare_boundary(&game, &settings, std::slice::from_ref(&game), &command)?;
    let (status, _observer) = process::run(command, &mut session, None)?;
    complete_session(&session, &pm, Some(status), false).await?;
    check_outcome(launch::LaunchOutcome::Exited(status))
}

async fn recover() -> Result<()> {
    let session = PendingSession::load()?.context("no interrupted preparation")?;
    session.require_owner()?;
    anyhow::ensure!(
        session.phase.is_preparation(),
        "a game may have started; use `modde library finish` only after it exits"
    );
    let pm = ProfileManager::open().await?;
    session.restore_preparation(&pm).await?;
    if session.deployment_started {
        LibraryPreferences::update(|preferences| {
            preferences
                .needs_deploy
                .insert(session.installation.clone());
        })?;
    }
    clear_manager_marker(&session)?;
    process::remove_request(&session)?;
    PendingSession::clear()?;
    if session.deployment_started {
        println!(
            "{}: preparation recovered. Play will redeploy the selected profile before launching.",
            session.name
        );
    } else {
        println!("{}: save operation recovery complete.", session.name);
    }
    Ok(())
}
