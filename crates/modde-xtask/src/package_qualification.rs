//! Opt-in exact-package fixtures. Every operation uses fresh config/data roots;
//! provider credentials, user saves and production databases are never read.
use std::path::Path;
use std::process::{Command, Output};

use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};

#[cfg(unix)]
pub(super) fn run(binary: &Path, output: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    ensure!(
        cfg!(target_os = "linux"),
        "native containment qualification requires Linux"
    );
    ensure!(
        binary.is_absolute() && binary.is_file(),
        "provide an absolute package CLI"
    );
    ensure!(
        !output.exists(),
        "qualification output already exists; choose a new directory"
    );
    let output = std::path::absolute(output)?;
    std::fs::create_dir_all(&output)?;
    std::fs::set_permissions(&output, std::fs::Permissions::from_mode(0o700))?;
    for subdirectory in ["home", "config/modde", "data", "game", "prefix/drive_c"] {
        std::fs::create_dir_all(output.join(subdirectory))?;
    }
    let game = output.join("game");
    let script = game.join("synthetic-game");
    std::fs::write(
        output.join("config/modde/settings.toml"),
        format!(
            "[[game_paths]]\ngame_id = \"cyberpunk2077\"\npath = {}\n",
            serde_json::to_string(&game)?
        ),
    )?;
    let invoke = |arguments: &[&str]| -> Result<Output> {
        let result = Command::new(binary)
            .env_clear()
            .env("HOME", output.join("home"))
            .env("XDG_CONFIG_HOME", output.join("home/.config"))
            .env("XDG_DATA_HOME", output.join("home/.local/share"))
            .env("XDG_CACHE_HOME", output.join("home/.cache"))
            .env("MODDE_DATABASE_BACKEND", "sqlite")
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .arg("--config-dir")
            .arg(output.join("config"))
            .arg("--data-dir")
            .arg(output.join("data"))
            .args(["library"])
            .args(arguments)
            .output()?;
        Ok(result)
    };
    let list = invoke(&["list", "--json"])?;
    ensure!(
        list.status.success(),
        "package catalogue failed: {}",
        String::from_utf8_lossy(&list.stderr)
    );
    let catalogue: Value = serde_json::from_slice(&list.stdout)?;
    let id = catalogue["games"]
        .as_array()
        .context("catalogue missing games")?
        .iter()
        .find(|g| g["install_path"] == json!(game))
        .and_then(|g| g["id"].as_str())
        .context("synthetic installation not discovered")?;
    let launch = output.join("launch.json");
    std::fs::write(
        &launch,
        serde_json::to_vec_pretty(&json!({
            "executable": script, "use_active_profile": false,
            "environment": {"MODDE_FIXTURE_SECRET": "private-fixture-token"},
            "sandbox": {"enabled": true, "network": false},
        }))?,
    )?;
    let mut runs = Vec::new();
    for (label, body, expected_raw) in [
        (
            "success",
            "(sleep 0.1; printf done > descendant-finished) & exit 0",
            0,
        ),
        (
            "descendant-failure",
            "(sleep 0.1; printf done > descendant-finished; exit 7) & exit 0",
            1792,
        ),
        (
            "signal",
            "(sleep 0.1; printf done > descendant-finished) & kill -TERM $$",
            15,
        ),
    ] {
        let marker = game.join("descendant-finished");
        if marker.exists() {
            std::fs::remove_file(&marker)?;
        }
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\n[ ! -e /proc/self/fd/3 ] || exit 99\nprintf 'game-output %s\\n' \"$MODDE_FIXTURE_SECRET\"\n{body}\n"
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700))?;
        let configured = invoke(&[
            "configure",
            id,
            "--file",
            launch.to_str().context("non-UTF8 fixture path")?,
        ])?;
        ensure!(
            configured.status.success(),
            "configuration failed: {}",
            String::from_utf8_lossy(&configured.stderr)
        );
        let played = invoke(&["play", id])?;
        std::fs::write(output.join(format!("{label}.stdout")), &played.stdout)?;
        std::fs::write(output.join(format!("{label}.stderr")), &played.stderr)?;
        ensure!(
            played.status.success() == (expected_raw == 0),
            "unexpected package exit for {label}: {}",
            String::from_utf8_lossy(&played.stderr)
        );
        ensure!(
            marker.exists(),
            "package truncated detached descendants for {label}"
        );
        let status = invoke(&["status"])?;
        ensure!(status.status.success(), "status command failed");
        let status: Value = serde_json::from_slice(&status.stdout)?;
        ensure!(
            status["session"].is_null(),
            "package retained a completed journal for {label}"
        );
        let record = &status["recent"][0];
        let directory = Path::new(
            record["directory"]
                .as_str()
                .context("missing diagnostics directory")?,
        );
        let evidence: Value =
            serde_json::from_slice(&std::fs::read(directory.join("evidence.json"))?)?;
        ensure!(
            evidence["started"] == true
                && evidence["boundary_started"] == true
                && evidence["completed"] == true,
            "missing inner-command/lifetime evidence for {label}"
        );
        ensure!(
            evidence["raw_status"] == expected_raw && evidence["inner_raw_status"] == expected_raw,
            "lost raw wait status for {label}: {evidence}"
        );
        ensure!(
            !directory.join("request.json").exists(),
            "private request was not consumed"
        );
        let log = directory.join("game.log");
        ensure!(
            std::fs::read_to_string(&log)?.contains("game-output private-fixture-token"),
            "missing supervisor-owned output"
        );
        ensure!(
            log.metadata()?.permissions().mode() & 0o777 == 0o600,
            "game output is not private"
        );
        let exported = invoke(&[
            "diagnostics",
            "--run",
            record["id"].as_str().context("missing run ID")?,
        ])?;
        ensure!(
            exported.status.success()
                && !String::from_utf8_lossy(&exported.stdout).contains("private-fixture-token"),
            "diagnostic export leaked raw values"
        );
        runs.push(json!({"label": label, "directory": directory, "evidence": evidence}));
    }
    // An executable with a missing interpreter passes pathname preflight but
    // fails inside the namespace before a game starts. It must recover rather
    // than capture the selected saves as a played session.
    std::fs::write(&script, "#!/missing-modde-fixture-interpreter\n")?;
    let failed = invoke(&["play", id])?;
    ensure!(
        !failed.status.success(),
        "inner startup failure became success"
    );
    let status: Value = serde_json::from_slice(&invoke(&["status"])?.stdout)?;
    ensure!(
        status["session"]["phase"] == "ready",
        "inner exec failure did not retain preparation"
    );
    let directory = Path::new(
        status["recent"][0]["directory"]
            .as_str()
            .context("missing failed run")?,
    );
    let evidence: Value = serde_json::from_slice(&std::fs::read(directory.join("evidence.json"))?)?;
    ensure!(
        evidence["boundary_started"] == true
            && evidence["started"] == false
            && evidence["completed"] == true,
        "bad inner startup failure evidence"
    );
    ensure!(
        invoke(&["recover"])?.status.success(),
        "inner startup recovery failed"
    );
    runs.push(json!({"label": "inner-exec-failure", "directory": directory, "evidence": evidence}));
    std::fs::write(
        output.join("receipt.json"),
        serde_json::to_vec_pretty(&json!({
            "binary": binary, "platform": std::env::consts::OS, "runs": runs,
            "actual_game_rendering": "unqualified", "performance": "unmeasured",
        }))?,
    )?;
    println!(
        "Exact-package lifecycle qualification passed: {}",
        output.join("receipt.json").display()
    );
    Ok(())
}

#[cfg(not(unix))]
pub(super) fn run(_binary: &Path, _output: &Path) -> Result<()> {
    anyhow::bail!("native containment qualification requires Linux")
}
