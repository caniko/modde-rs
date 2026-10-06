//! Real OpenGL rendering and optional Vulkan enumeration through both boundaries.
//! Usage: `qualify_gpu /path/to/glxinfo /path/to/report.json [PCI_NODE] [VULKANINFO]`
use std::path::PathBuf;

use anyhow::{Context, Result, ensure};
use modde_core::library::LaunchSettings;
use modde_games::library::{LibraryGame, Store, gpu, launch};

fn renderer(output: &str) -> Result<&str> {
    ensure!(
        output.lines().any(|line| line.trim() == "Accelerated: yes"),
        "renderer is not hardware accelerated"
    );
    output
        .lines()
        .find_map(|line| line.strip_prefix("OpenGL renderer string: "))
        .context("OpenGL renderer string unavailable")
}

fn vulkan_device(output: &str) -> Result<(&str, &str)> {
    let first = output
        .split_once("GPU0:")
        .context("Vulkan device unavailable")?
        .1
        .split("GPU1:")
        .next()
        .context("Vulkan device block unavailable")?;
    ensure!(
        first.contains("PHYSICAL_DEVICE_TYPE_DISCRETE_GPU")
            || first.contains("PHYSICAL_DEVICE_TYPE_INTEGRATED_GPU"),
        "preferred Vulkan device is not hardware"
    );
    let name = first
        .lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("deviceName")
                .and_then(|s| s.trim().strip_prefix('='))
        })
        .context("Vulkan device name unavailable")?
        .trim();
    Ok((name, first))
}

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let program = PathBuf::from(args.next().context("supply the absolute glxinfo path")?);
    let report = PathBuf::from(args.next().context("supply the report path")?);
    let node = args.next().map(PathBuf::from);
    let vulkan = args.next().map(PathBuf::from);
    ensure!(args.next().is_none(), "too many qualification arguments");
    ensure!(
        program.is_absolute() && report.is_absolute(),
        "program and report paths must be absolute"
    );
    let root = tempfile::tempdir()?;
    let install = root.path().join("game");
    std::fs::create_dir_all(&install)?;
    modde_core::paths::set_data_dir(root.path().join("data"));
    modde_core::paths::set_config_dir(root.path().join("config"));
    let game = LibraryGame::new(
        Store::Local,
        "gpu-probe".into(),
        "GPU qualification".into(),
        Some(install),
    );
    let mut settings = LaunchSettings {
        gpu_render_node: node,
        use_active_profile: false,
        ..Default::default()
    };
    let gpu = gpu::snapshot(&settings)?;
    let mut runs = Vec::new();
    let mut programs = vec![("OpenGL", program, "-B")];
    if let Some(vulkan) = vulkan {
        programs.push(("Vulkan", vulkan, "--summary"));
    }
    for (api, program, argument) in programs {
        let mut expected = None;
        settings.executable = Some(program);
        settings.arguments = vec![argument.into()];
        for sandbox in [false, true] {
            settings.sandbox.enabled = sandbox;
            let launch::PreparedLaunch::Direct(mut command) =
                launch::prepare(&game, &settings, std::slice::from_ref(&game))?
            else {
                anyhow::bail!("GPU probe did not produce a direct command");
            };
            let output = command.output()?;
            let stdout = String::from_utf8(output.stdout)?;
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            ensure!(
                output.status.success(),
                "GPU probe (sandbox={sandbox}) failed: {stderr}"
            );
            let (actual, device_output) = if api == "OpenGL" {
                (renderer(&stdout)?, stdout.as_str())
            } else {
                vulkan_device(&stdout)?
            };
            let actual = actual.to_owned();
            if let Some(device) = &gpu.selected {
                // glxinfo reports PCI device/vendor IDs in its extended renderer info.
                for id in [&device.vendor_id, &device.device_id] {
                    ensure!(
                        device_output.contains(id),
                        "{api} device did not report selected PCI ID {id}"
                    );
                }
            }
            if let Some(expected) = &expected {
                ensure!(&actual == expected, "sandbox changed the renderer");
            }
            expected = Some(actual.clone());
            runs.push(serde_json::json!({"api": api, "sandbox": sandbox, "device": actual, "stdout": stdout, "stderr": stderr, "exit_code": output.status.code()}));
        }
        println!(
            "{api} hardware probe passed outside and inside bubblewrap: {}",
            expected.context("no device")?
        );
    }
    modde_core::library::atomic_json(
        &report,
        &serde_json::json!({"gpu": gpu, "runs": runs, "scope": "OpenGL hardware rendering and optional Vulkan device enumeration; not a game or performance qualification"}),
    )?;
    println!("Evidence: {}", report.display());
    Ok(())
}
