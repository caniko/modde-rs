//! Host qualification gate: `cargo run -p modde-games --example qualify_sandbox`.
//! Requires real bubblewrap and Linux user namespaces; failures are never skipped.
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use modde_core::library::LaunchSettings;
use modde_games::library::{LibraryGame, Store, launch};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct Probe {
    hidden: PathBuf,
    readonly: PathBuf,
    writable: PathBuf,
    saves: PathBuf,
    prefix: PathBuf,
    net_namespace: PathBuf,
    pid_namespace: PathBuf,
    network: bool,
}

fn probe(path: &Path) -> Result<()> {
    let probe: Probe = serde_json::from_slice(&std::fs::read(path)?)?;
    ensure!(!probe.hidden.exists(), "ungranted home file is visible");
    ensure!(
        std::fs::read_to_string(probe.readonly.join("asset"))? == "asset",
        "read grant failed"
    );
    ensure!(
        std::fs::write(probe.readonly.join("changed"), "bad").is_err(),
        "read-only grant permits writing"
    );
    for directory in [&probe.writable, &probe.saves, &probe.prefix] {
        std::fs::write(directory.join("changed"), "saved")?;
    }
    ensure!(
        std::env::var_os("MODDE_STEAM_API_KEY").is_none(),
        "inherited credential leaked"
    );
    ensure!(
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none(),
        "session bus environment leaked"
    );
    ensure!(
        std::fs::read_link("/proc/self/ns/pid")? != probe.pid_namespace,
        "PID namespace is shared"
    );
    let network_shared = std::fs::read_link("/proc/self/ns/net")? == probe.net_namespace;
    ensure!(
        network_shared == probe.network,
        "network namespace does not match setting"
    );
    std::fs::write(
        PathBuf::from(std::env::var_os("HOME").context("HOME missing")?).join("private-write"),
        "private",
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn qualify() -> Result<()> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let install = root.path().join("game");
    let readonly = root.path().join("assets");
    let writable = root.path().join("writable");
    let saves = home.join("saves");
    let prefix = home.join("prefix");
    for path in [&install, &readonly, &writable, &saves, &prefix] {
        std::fs::create_dir_all(path)?;
    }
    std::fs::write(home.join("secret"), "ungranted")?;
    std::fs::write(readonly.join("asset"), "asset")?;
    modde_core::paths::set_data_dir(root.path().join("modde-data"));
    modde_core::paths::set_config_dir(root.path().join("modde-config"));
    let game = LibraryGame::new(
        Store::Local,
        "qualification".into(),
        "Sandbox fixture".into(),
        Some(install.clone()),
    );
    // A private-HOME alias verifies file-only grants without changing argv[0].
    let alias = home.join("fixture-probe");
    std::os::unix::fs::symlink(std::env::current_exe()?, &alias)?;
    let request = install.join("probe.json");
    let mut settings = LaunchSettings {
        executable: Some(alias),
        save_directory: Some(saves.clone()),
        prefix: Some(prefix.clone()),
        arguments: vec!["--probe".into(), request.to_string_lossy().into_owned()],
        ..Default::default()
    };
    settings
        .environment
        .insert("HOME".into(), home.to_string_lossy().into_owned());
    settings.sandbox.enabled = true;
    settings.sandbox.read_only.push(readonly.clone());
    settings.sandbox.writable.push(writable.clone());
    for network in [false, true] {
        settings.sandbox.network = network;
        modde_core::library::atomic_json(
            &request,
            &Probe {
                hidden: home.join("secret"),
                readonly: readonly.clone(),
                writable: writable.clone(),
                saves: saves.clone(),
                prefix: prefix.clone(),
                network,
                net_namespace: std::fs::read_link("/proc/self/ns/net")?,
                pid_namespace: std::fs::read_link("/proc/self/ns/pid")?,
            },
        )?;
        let launch::PreparedLaunch::Direct(mut command) =
            launch::prepare(&game, &settings, std::slice::from_ref(&game))?
        else {
            anyhow::bail!("qualification must use a direct executable");
        };
        // Deliberately inherited by the bwrap process, excluded by --clearenv.
        command
            .env("MODDE_STEAM_API_KEY", "qualification-secret")
            .env(
                "DBUS_SESSION_BUS_ADDRESS",
                "unix:path=/nonexistent-qualification-bus",
            );
        let output = command.output()?;
        ensure!(
            output.status.success(),
            "sandbox probe failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        for directory in [&writable, &saves, &prefix] {
            ensure!(
                std::fs::read_to_string(directory.join("changed"))? == "saved",
                "writable grant did not reach host"
            );
        }
        ensure!(
            !home.join("private-write").exists(),
            "private HOME write reached host HOME"
        );
        ensure!(
            !readonly.join("changed").exists(),
            "read-only write reached host"
        );
    }
    // The in-namespace observer must outlive the launcher and keep a detached
    // game's failure from becoming a successful performance result.
    for code in [0, 7] {
        std::fs::remove_file(writable.join("descendant")).ok();
        settings.arguments = vec![
            "--detach".into(),
            writable.to_string_lossy().into_owned(),
            code.to_string(),
        ];
        let launch::PreparedLaunch::Direct(mut command) = launch::prepare_observed(
            &game,
            &settings,
            std::slice::from_ref(&game),
            None,
            &std::env::current_exe()?,
        )?
        else {
            anyhow::bail!("qualification must use a direct executable");
        };
        let output = command.output()?;
        ensure!(
            output.status.code() == Some(code),
            "detached sandbox exit mismatch: {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        ensure!(
            std::fs::read_to_string(writable.join("descendant"))
                .context("detached game did not complete")?
                == "completed",
            "sandbox ended before detached descendant"
        );
    }
    println!(
        "Real bubblewrap qualification passed: file grants, private HOME, credential exclusion, PID and network namespaces, detached lifetime and failure"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn qualify() -> Result<()> {
    anyhow::bail!("sandbox qualification requires Linux")
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    match args.next().as_deref() {
        Some(mode) if mode == "library" => {
            ensure!(
                args.next().as_deref() == Some(std::ffi::OsStr::new("reap")),
                "observer mode missing"
            );
            ensure!(
                args.next().as_deref() == Some(std::ffi::OsStr::new("--")),
                "observer boundary missing"
            );
            let status = modde_games::library::observer::reap(&args.collect::<Vec<_>>())?;
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
        Some(mode) if mode == "--probe" => {
            probe(&PathBuf::from(args.next().context("probe path missing")?))
        }
        Some(mode) if mode == "--detach" => {
            let destination = args.next().context("descendant destination missing")?;
            let code = args.next().context("descendant exit missing")?;
            std::process::Command::new(std::env::current_exe()?)
                .args([std::ffi::OsString::from("--descendant"), destination, code])
                .spawn()?;
            Ok(())
        }
        Some(mode) if mode == "--descendant" => {
            let destination = PathBuf::from(args.next().context("descendant destination missing")?);
            let code: i32 = args
                .next()
                .context("descendant exit missing")?
                .to_str()
                .context("invalid exit")?
                .parse()?;
            std::thread::sleep(std::time::Duration::from_millis(250));
            std::fs::write(destination.join("descendant"), "completed")?;
            std::process::exit(code);
        }
        None => qualify(),
        Some(_) => anyhow::bail!("unknown qualification argument"),
    }
}
