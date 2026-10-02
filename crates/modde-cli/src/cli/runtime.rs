//! CLI runtime bootstrap and process-wide setup.

#[cfg(feature = "remote-telemetry")]
use std::{env, time::Duration};

#[cfg(feature = "remote-telemetry")]
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
#[cfg(feature = "remote-telemetry")]
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;
#[cfg(feature = "remote-telemetry")]
use tracing_subscriber::prelude::*;

#[cfg(feature = "remote-telemetry")]
use crate::telemetry;

use super::args::Cli;
#[cfg(feature = "gui")]
use super::args::Commands;
use super::dispatch::run_command;
use super::mutation::{
    command_mutates_state, command_runs_lazy_product_update_check,
    maybe_print_product_update_notice,
};

pub(crate) fn run() -> Result<()> {
    let cli = Cli::parse();
    if let super::args::Commands::Library {
        action: super::args::LibraryAction::Reap { command },
    } = &cli.command
    {
        let status = modde_games::library::observer::reap(command)?;
        #[cfg(unix)]
        let code = {
            use std::os::unix::process::ExitStatusExt;
            status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(1))
        };
        #[cfg(not(unix))]
        let code = status.code().unwrap_or(1);
        std::process::exit(code);
    }
    if let Some(dir) = cli.config_dir.clone() {
        modde_core::paths::set_config_dir(std::path::absolute(dir)?);
    }
    if let Some(dir) = cli.data_dir.clone() {
        modde_core::paths::set_data_dir(std::path::absolute(dir)?);
    }
    if let super::args::Commands::Library {
        action: super::args::LibraryAction::Supervise { request },
    } = &cli.command
    {
        // The subreaper must own only game descendants, without another runtime
        // or telemetry worker sharing its process-wide child-reaping policy.
        return crate::commands::library::supervise(request);
    }
    if let super::args::Commands::Library {
        action: super::args::LibraryAction::CompleteObserved { observation },
    } = &cli.command
    {
        return crate::commands::library::complete_observed(observation);
    }
    #[cfg(not(feature = "remote-telemetry"))]
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    #[cfg(feature = "remote-telemetry")]
    let telemetry_runtime = tokio::runtime::Runtime::new()?;
    #[cfg(feature = "remote-telemetry")]
    let _telemetry_runtime_guard = telemetry_runtime.enter();
    #[cfg(feature = "remote-telemetry")]
    init_tracing()?;
    #[cfg(feature = "remote-telemetry")]
    init_remote_telemetry(&telemetry_runtime)?;

    #[cfg(feature = "remote-telemetry")]
    {
        assert!(!cli.debug_panic, "remote telemetry debug panic");
    }

    let _heap_profiler = start_heap_profiler(cli.heap_profile.as_deref())?;

    // GUI launches its own runtime (iced), so handle it outside tokio.
    #[cfg(feature = "gui")]
    if matches!(cli.command, Commands::Gui) {
        modde_ui::app::run().map_err(|e| anyhow::anyhow!("GUI error: {e}"))?;
        return Ok(());
    }

    // Whether this command may have mutated the profile DB / store.
    // Used after the dispatch below to push a refresh signal to any
    // running GUI(s). Read-only commands skip the notify so we don't
    // spam GUIs on `modde profile show` or `modde update check`.
    let mutates_state = command_mutates_state(&cli.command);
    let lazy_update_check = !mutates_state && command_runs_lazy_product_update_check(&cli.command);

    let mutation_guard = if mutates_state {
        let wrapping = matches!(
            &cli.command,
            super::args::Commands::Library {
                action: super::args::LibraryAction::Wrap { .. }
            }
        );
        let start = std::time::Instant::now();
        let guard = loop {
            match modde_core::library::mutation_lock() {
                Ok(guard) => break guard,
                Err(_) if wrapping && start.elapsed() < std::time::Duration::from_secs(5) => {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
                Err(error) => return Err(error),
            }
        };
        if !matches!(
            &cli.command,
            super::args::Commands::Library {
                action: super::args::LibraryAction::Finish { .. }
                    | super::args::LibraryAction::Recover
                    | super::args::LibraryAction::Wrap { .. }
            }
        ) && let Some(session) = modde_core::library::PendingSession::load_blocking()?
        {
            let ingesting_completion = matches!(&cli.command, super::args::Commands::Perf { action: super::args::PerfAction::Ingest { run_id, .. } }
                if session.phase == modde_core::library::SessionPhase::Captured
                && session.launch_request.as_ref().and_then(|r| r.get("performance")).and_then(|r| r.get("run_id")).and_then(serde_json::Value::as_str) == Some(run_id.as_str()));
            if !ingesting_completion {
                anyhow::bail!(
                    "{} has an unfinished {:?} session; use `modde library recover` for preparation, or `modde library finish` after the game exits",
                    session.name,
                    session.phase
                );
            }
        }
        Some(guard)
    } else {
        None
    };

    let result = run_command(cli);
    // The request is durable before releasing this lease. Some URI handlers
    // wait for the store/game; its wrapper must be free to claim the request.
    let store_uri = if mutates_state && result.is_ok() {
        crate::commands::library::pending_store_uri()?
    } else {
        None
    };
    drop(mutation_guard);
    if let Some(uri) = store_uri {
        open::that(uri)?;
    }
    if mutates_state && result.is_ok() {
        let _ = modde_core::ipc::notify_refresh();
    }
    if lazy_update_check && result.is_ok() {
        maybe_print_product_update_notice();
    }
    result
}

#[cfg(feature = "remote-telemetry")]
fn init_tracing() -> Result<()> {
    let fmt_layer = tracing_subscriber::fmt::layer();
    let filter = EnvFilter::from_default_env();
    let registry = tracing_subscriber::registry().with(filter).with(fmt_layer);

    if let Some(config) = remote_telemetry_config()? {
        let layer = detritus::Layer::builder()
            .endpoint(config.endpoint.clone())
            .token(config.token.clone())
            .source(config.source.clone())
            .queue_dir(config.logs_dir.clone())
            .build()
            .context("failed to initialize remote telemetry tracing layer")?;

        registry
            .with(layer)
            .try_init()
            .context("failed to initialize tracing subscriber")?;
    } else {
        registry
            .try_init()
            .context("failed to initialize tracing subscriber")?;
    }

    Ok(())
}

#[cfg(feature = "remote-telemetry")]
#[derive(Clone)]
struct RemoteTelemetryConfig {
    endpoint: url::Url,
    token: secrecy::SecretString,
    source: detritus::SourceId,
    logs_dir: PathBuf,
    crashes_dir: PathBuf,
}

#[cfg(feature = "remote-telemetry")]
fn remote_telemetry_config() -> Result<Option<RemoteTelemetryConfig>> {
    // ponytail: legacy RS_MODDE_* names kept as fallback until 0.8.0; then drop.
    let Some(endpoint) = env::var("MODDE_TELEMETRY_ENDPOINT")
        .or_else(|_| env::var("RS_MODDE_TELEMETRY_ENDPOINT"))
        .ok()
    else {
        return Ok(None);
    };
    let Some(token) = env::var("MODDE_TELEMETRY_TOKEN")
        .or_else(|_| env::var("RS_MODDE_TELEMETRY_TOKEN"))
        .ok()
    else {
        return Ok(None);
    };

    let endpoint = url::Url::parse(&endpoint).context("invalid MODDE_TELEMETRY_ENDPOINT")?;
    let source = detritus::SourceId {
        project: "modde-rs".to_owned(),
        platform: target_platform(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
        install_id: telemetry::persistent_install_id()?,
    };
    let telemetry_dir = telemetry::telemetry_dir()?;

    Ok(Some(RemoteTelemetryConfig {
        endpoint,
        token: secrecy::SecretString::from(token),
        source,
        logs_dir: telemetry_dir.join("logs"),
        crashes_dir: telemetry_dir.join("crashes"),
    }))
}

#[cfg(feature = "remote-telemetry")]
fn init_remote_telemetry(runtime: &tokio::runtime::Runtime) -> Result<()> {
    let Some(config) = remote_telemetry_config()? else {
        return Ok(());
    };

    detritus::install_panic_hook(detritus::PanicHookConfig {
        endpoint: config.endpoint.clone(),
        token: config.token.clone(),
        source: config.source.clone(),
        spool_dir: config.crashes_dir.clone(),
        kind: detritus::PanicKind::PanicTarball,
        build: detritus_protocol::BuildInfo {
            git_sha: option_env!("MODDE_GIT_SHA").unwrap_or("unknown").to_owned(),
            profile: option_env!("PROFILE").unwrap_or("unknown").to_owned(),
            target_triple: target_platform(),
        },
        context: serde_json::json!({}),
        sent_retention_days: 90,
    })
    .context("failed to install remote telemetry panic hook")?;

    let ship_result = runtime.block_on(async {
        tokio::time::timeout(
            Duration::from_secs(3),
            detritus::ship_pending_crashes(
                &config.crashes_dir,
                config.endpoint.clone(),
                config.token.clone(),
            ),
        )
        .await
    });

    match ship_result {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => tracing::warn!(%error, "failed to ship pending remote telemetry crashes"),
        Err(_) => tracing::warn!("timed out shipping pending remote telemetry crashes"),
    }

    Ok(())
}

#[cfg(feature = "remote-telemetry")]
fn target_platform() -> String {
    format!("{}-{}", env::consts::ARCH, env::consts::OS)
}

#[cfg(feature = "heap-profile")]
fn start_heap_profiler(path: Option<&std::path::Path>) -> Result<Option<()>> {
    if let Some(path) = path {
        anyhow::bail!(
            "--heap-profile={} is not available in this build because the `turso` dependency already defines the process global allocator; use `--diagnostics-dir` for bounded-memory telemetry",
            path.display()
        );
    }
    Ok(None)
}

#[cfg(not(feature = "heap-profile"))]
fn start_heap_profiler(path: Option<&std::path::Path>) -> Result<Option<()>> {
    if let Some(path) = path {
        anyhow::bail!(
            "--heap-profile={} requires building modde with `--features heap-profile`",
            path.display()
        );
    }
    Ok(None)
}
