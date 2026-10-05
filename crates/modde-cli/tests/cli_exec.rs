//! End-to-end tests for the `modde exec` alias.
//!
//! `modde exec` is a thin alias for the executable-management subset
//! of `modde tool`. The two surfaces share storage, so writing through
//! `exec add` and reading through `tool list-executables` (and vice
//! versa) must round-trip cleanly. These tests pin that contract.

mod common;

use common::Fixture;

#[cfg(feature = "bethesda")]
fn fake_executable(fx: &Fixture, name: &str) -> std::path::PathBuf {
    let path = fx.root().join(name);
    std::fs::write(&path, b"#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

#[test]
#[cfg(feature = "bethesda")]
fn exec_add_then_list_round_trips() {
    let fx = Fixture::new();
    let exe = fake_executable(&fx, "fake-xedit");

    let add = fx
        .cmd()
        .args([
            "exec",
            "add",
            "xEdit",
            exe.to_str().unwrap(),
            "--game",
            "skyrim-se",
            "--output-mod",
            "xEdit-output",
        ])
        .args(["--", "-quickautoclean"])
        .output()
        .expect("spawn modde exec add");
    assert!(
        add.status.success(),
        "exec add failed: {}",
        String::from_utf8_lossy(&add.stderr)
    );

    let list = fx
        .cmd()
        .args(["exec", "list", "--game", "skyrim-se"])
        .output()
        .expect("spawn modde exec list");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("xEdit"), "list missing 'xEdit':\n{stdout}");
    assert!(
        stdout.contains("xEdit-output"),
        "list missing output_mod 'xEdit-output':\n{stdout}"
    );
    assert!(
        stdout.contains("-quickautoclean"),
        "list missing stored args:\n{stdout}"
    );
}

#[test]
#[cfg(feature = "bethesda")]
fn exec_add_is_upsert() {
    // Re-running `add` with the same name overwrites the previous row.
    // This is what makes `add` double as `edit` per the TODO.
    let fx = Fixture::new();
    let exe = fake_executable(&fx, "fake");

    fx.cmd()
        .args([
            "exec",
            "add",
            "Tool",
            exe.to_str().unwrap(),
            "--game",
            "skyrim-se",
            "--output-mod",
            "first-output",
        ])
        .output()
        .expect("first add");

    fx.cmd()
        .args([
            "exec",
            "add",
            "Tool",
            exe.to_str().unwrap(),
            "--game",
            "skyrim-se",
            "--output-mod",
            "second-output",
        ])
        .output()
        .expect("second add");

    let stdout = {
        let out = fx
            .cmd()
            .args(["exec", "list", "--game", "skyrim-se"])
            .output()
            .expect("list");
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    assert!(
        stdout.contains("second-output"),
        "second add should win:\n{stdout}"
    );
    assert!(
        !stdout.contains("first-output"),
        "first add should be overwritten:\n{stdout}"
    );
}

#[test]
#[cfg(feature = "bethesda")]
fn exec_remove_drops_entry() {
    let fx = Fixture::new();
    let exe = fake_executable(&fx, "fake");

    fx.cmd()
        .args([
            "exec",
            "add",
            "Trash",
            exe.to_str().unwrap(),
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("add");

    let rm = fx
        .cmd()
        .args(["exec", "remove", "Trash", "--game", "skyrim-se"])
        .output()
        .expect("remove");
    assert!(rm.status.success());

    let list = fx
        .cmd()
        .args(["exec", "list", "--game", "skyrim-se"])
        .output()
        .expect("list");
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(
        !stdout.contains("Trash"),
        "removed entry should be gone:\n{stdout}"
    );
}

#[test]
#[cfg(feature = "bethesda")]
fn exec_alias_shares_storage_with_tool() {
    // Add via `exec`, list via `tool list-executables` — same row.
    let fx = Fixture::new();
    let exe = fake_executable(&fx, "fake");

    fx.cmd()
        .args([
            "exec",
            "add",
            "Shared",
            exe.to_str().unwrap(),
            "--game",
            "skyrim-se",
        ])
        .output()
        .expect("exec add");

    let out = fx
        .cmd()
        .args(["tool", "list-executables", "--game", "skyrim-se"])
        .output()
        .expect("tool list-executables");
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("Shared"),
        "exec/tool surfaces should share storage:\n{stdout}"
    );
}

#[test]
fn exec_run_unknown_name_fails_clean() {
    let fx = Fixture::new();
    let out = fx
        .cmd()
        .args(["exec", "run", "ghost", "--game", "skyrim-se"])
        .output()
        .expect("run");
    assert!(!out.status.success(), "running an unknown exec must fail");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("ghost") || stderr.contains("no executable"),
        "expected friendly error; got:\n{stderr}"
    );
}
