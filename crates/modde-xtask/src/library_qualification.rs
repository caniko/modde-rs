//! Repeatable qualification gates, runnable by CI without live store credentials.
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};

fn cargo(root: &Path, jobs: u16, arguments: &[&str]) -> Result<()> {
    let program = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(program);
    command
        .current_dir(root)
        .args(arguments)
        .args(["--locked", "-j", &jobs.to_string()]);
    println!("Qualification: {command:?}");
    let status = command
        .status()
        .context("running Cargo qualification gate")?;
    ensure!(
        status.success(),
        "qualification gate failed: {arguments:?} ({status})"
    );
    Ok(())
}

pub(super) fn run(root: &Path, containment: bool, jobs: u16) -> Result<()> {
    for package in ["modde-core", "modde-games", "modde-sources"] {
        cargo(
            root,
            jobs,
            &["check", "-p", package, "--no-default-features", "--lib"],
        )?;
    }
    cargo(
        root,
        jobs,
        &[
            "check",
            "-p",
            "modde",
            "--no-default-features",
            "--bin",
            "modde",
        ],
    )?;
    for feature in [
        "bethesda",
        "gamebryo",
        "cyberpunk",
        "ue4",
        "bg3",
        "stardew",
        "bannerlord",
        "witcher3",
        "oblivion-remastered",
    ] {
        cargo(
            root,
            jobs,
            &[
                "check",
                "-p",
                "modde",
                "--no-default-features",
                "--features",
                feature,
                "--bin",
                "modde",
            ],
        )?;
    }
    cargo(
        root,
        jobs,
        &[
            "test",
            "-p",
            "modde-core",
            "--test",
            "installation_state_tests",
            "--test",
            "save_transition_tests",
            "--test",
            "repo_truth_tests",
            "--test",
            "diagnostic_retention_tests",
        ],
    )?;
    cargo(
        root,
        jobs,
        &[
            "test",
            "-p",
            "modde-games",
            "--test",
            "installation_context_tests",
            "--test",
            "installation_prefix_tests",
            "--test",
            "store_context_tests",
            "--test",
            "library_sandbox_commands",
        ],
    )?;
    cargo(
        root,
        jobs,
        &[
            "test",
            "-p",
            "modde",
            "--all-features",
            "--test",
            "cli_library_preparation",
            "--test",
            "cli_library_supervision",
        ],
    )?;
    cargo(
        root,
        jobs,
        &[
            "test",
            "-p",
            "modde",
            "--all-features",
            "--bin",
            "modde",
            "commands::perf",
        ],
    )?;
    cargo(
        root,
        jobs,
        &[
            "test",
            "-p",
            "modde",
            "--all-features",
            "--bin",
            "modde",
            "commands::bisect",
        ],
    )?;
    cargo(
        root,
        jobs,
        &[
            "test",
            "-p",
            "modde-ui",
            "--all-features",
            "--lib",
            "app::tests::library",
        ],
    )?;
    if containment {
        ensure!(
            cfg!(target_os = "linux"),
            "real containment gate requires Linux"
        );
        cargo(
            root,
            jobs,
            &["run", "-p", "modde-games", "--example", "qualify_sandbox"],
        )?;
    }
    println!(
        "Library regression qualification passed. Live providers, GPU rendering, distribution builds and paired game performance require separate evidence."
    );
    Ok(())
}
