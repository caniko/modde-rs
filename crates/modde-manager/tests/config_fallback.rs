//! Subprocess tests for the bare-token entry points: config-fallback
//! boundedness and the Lutris gate passthrough. These must spawn the real
//! binary — `exec` replaces the process image, so the positive paths can
//! never be unit-tested in-process.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_ID: AtomicU64 = AtomicU64::new(0);

fn binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_modde-manager"))
}

/// Fresh scratch root per test (the harness runs tests in threads of one
/// process, so the counter — not just the pid — keeps them disjoint).
fn unique_root(tag: &str) -> PathBuf {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let root =
        std::env::temp_dir().join(format!("modde-manager-{tag}-{}-{id}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    root
}

fn write_exe(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// Minimal instance config: system wine, the game executable, integrity
/// pins only when the test needs a `wow-exe` finding. Plain concatenation
/// (no format placeholders) so JSON braces stay literal.
fn write_config(root: &Path, client: &Path, wow_exe: Option<&str>) -> PathBuf {
    let mut body = String::from("{\"version\": 1, \"instances\": {\"test\": {\"root\": \"");
    body.push_str(&client.display().to_string());
    body.push_str(
        "\", \"client\": \"wow-classic\", \"processes\": [], \"addons\": [], \
         \"wiring\": {\"runtime\": {\"kind\": \"wine\", \"version\": \"system\", \
         \"arch\": \"wow64\", \"anticheat\": false}, \
         \"launch\": {\"executable\": \"VanillaFixes.exe\"}, ",
    );
    match wow_exe {
        Some(pin) => {
            body.push_str(
                "\"client_integrity\": {\"require_files\": \
             [\"WoW.exe\", \"VanillaFixes.exe\"], \"wow_exe\": ",
            );
            body.push_str(pin);
            body.push('}');
        }
        None => body.push_str(
            "\"client_integrity\": {\"require_files\": [\"WoW.exe\", \"VanillaFixes.exe\"]}",
        ),
    }
    body.push_str("}}}}");
    let path = root.join("manager.json");
    fs::write(&path, body).unwrap();
    path
}

/// Sandbox the child: fake system wine on `PATH`, throwaway home/XDG, no
/// ambient manager config. Returns the child `PATH` value.
fn sandbox_env(cmd: &mut Command, root: &Path) -> String {
    let bin = root.join("bin");
    write_exe(&bin.join("wine"), "#!/bin/sh\nexit 0\n");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var_os("PATH")
            .unwrap_or_default()
            .to_string_lossy()
    );
    cmd.env("PATH", &path);
    cmd.env("HOME", root.join("home"));
    cmd.env("XDG_DATA_HOME", root.join("home/.local/share"));
    cmd.env("XDG_CONFIG_HOME", root.join("home/.config"));
    cmd.env_remove("MODDE_MANAGER_CONFIG");
    cmd.env_remove("MODDE_MANAGER_CONFIG_RESOLVED");
    path
}

#[test]
fn missing_config_fails_fast() {
    let root = unique_root("no-config");
    let empty = root.join("empty-path");
    fs::create_dir_all(&empty).unwrap();
    let output = Command::new(binary())
        .arg("list")
        .env("PATH", &empty)
        .env_remove("MODDE_MANAGER_CONFIG")
        .env_remove("MODDE_MANAGER_CONFIG_RESOLVED")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no --config given"),
        "unexpected stderr: {stderr}"
    );
}

/// Two config-less wrappers re-invoking the real binary must terminate:
/// the first re-exec sets the marker, the second refuses. A 20s watchdog
/// turns a ping-pong regression into a failure instead of a hung suite.
#[test]
fn config_fallback_does_not_ping_pong() {
    let root = unique_root("ping-pong");
    let real = binary();
    for dir in ["first", "second"] {
        write_exe(
            &root.join(dir).join("modde-manager"),
            &format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", real.display()),
        );
    }
    let path = format!(
        "{}:{}",
        root.join("first").display(),
        root.join("second").display()
    );
    let mut child = Command::new(&real)
        .arg("list")
        .env("PATH", &path)
        .env_remove("MODDE_MANAGER_CONFIG")
        .env_remove("MODDE_MANAGER_CONFIG_RESOLVED")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            let remaining = child.wait_with_output().unwrap();
            let stderr = String::from_utf8_lossy(&remaining.stderr);
            assert!(
                stderr.contains("already attempted"),
                "unexpected stderr: {stderr}"
            );
            return;
        }
        if std::time::Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("config fallback ping-ponged: still running after 20s");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

#[test]
fn gate_execs_harmless_command_after_readiness() {
    let root = unique_root("gate-pass");
    let client = root.join("client");
    fs::create_dir_all(&client).unwrap();
    fs::write(client.join("WoW.exe"), "fake-wow-bytes").unwrap();
    fs::write(client.join("VanillaFixes.exe"), "fake-fixes-bytes").unwrap();
    let config = write_config(&root, &client, None);
    let sentinel = root.join("sentinel");
    let probe = root.join("probe");
    write_exe(
        &probe,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nexit 0\n",
            sentinel.display()
        ),
    );
    let mut cmd = Command::new(binary());
    sandbox_env(&mut cmd, &root);
    let output = cmd
        .arg("--config")
        .arg(&config)
        .arg("onboard")
        .arg("gate")
        .arg("--instance")
        .arg("test")
        .arg("--")
        .arg(&probe)
        .arg("arg1")
        .arg("arg2")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "gate refused a ready client: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Arguments survive the gate unchanged and the exit status passes
    // through — the game runs exactly as Lutris declared it.
    assert_eq!(fs::read_to_string(&sentinel).unwrap(), "arg1\narg2\n");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("launch gate passed"),
        "gate banner missing"
    );
}

#[test]
fn gate_refuses_drifted_client_without_exec() {
    let root = unique_root("gate-refuse");
    let client = root.join("client");
    fs::create_dir_all(&client).unwrap();
    fs::write(client.join("WoW.exe"), "fake-wow-bytes").unwrap();
    fs::write(client.join("VanillaFixes.exe"), "fake-fixes-bytes").unwrap();
    // Size pin that can never match: the refusal names `wow-exe` before
    // any exec, so the sentinel must stay absent.
    let pin = r#"{"size": 1, "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "laa": false}"#;
    let config = write_config(&root, &client, Some(pin));
    let sentinel = root.join("sentinel");
    let probe = root.join("probe");
    write_exe(
        &probe,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"{}\"\nexit 0\n",
            sentinel.display()
        ),
    );
    let mut cmd = Command::new(binary());
    sandbox_env(&mut cmd, &root);
    let output = cmd
        .arg("--config")
        .arg(&config)
        .arg("onboard")
        .arg("gate")
        .arg("--instance")
        .arg("test")
        .arg("--")
        .arg(&probe)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("wow-exe"), "unexpected stderr: {stderr}");
    assert!(!sentinel.exists(), "drifted client was executed");
}
