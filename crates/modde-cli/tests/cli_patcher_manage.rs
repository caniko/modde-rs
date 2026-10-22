//! End-to-end tests for managing patcher stages: add, list, enable/disable,
//! reorder, validate, and remove.

#![cfg(all(unix, feature = "bethesda"))]

mod common;

use std::process::Output;

use common::Fixture;

const PROFILE: &[&str] = &["--profile", "main", "--game", "skyrim-se"];

fn patcher(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("patcher")
        .args(args)
        .args(PROFILE)
        .output()
        .expect("spawn modde patcher")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn err(output: &Output) -> String {
    assert!(!output.status.success(), "command unexpectedly succeeded");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Profile `main` with command stages `first`, `second` and a Synthesis
/// stage `synth`, in that order.
fn sandbox() -> Fixture {
    let fx = Fixture::new();
    ok(&fx
        .cmd()
        .args(["profile", "create", "main", "--game", "skyrim-se"])
        .output()
        .unwrap());
    let exe = fx.root().join("fake-patcher");
    std::fs::write(&exe, b"#!/bin/sh\nexit 0\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let exe = exe.to_str().unwrap();
    let settings = fx.root().join("pipeline.json");
    std::fs::write(&settings, "{}").unwrap();

    for (name, order) in [("first", "1"), ("second", "2")] {
        let out = ok(&patcher(
            &fx,
            &[
                "add-command",
                name,
                "--executable",
                exe,
                "--output-mod",
                &format!("out-{name}"),
                "--order",
                order,
            ],
        ));
        assert!(
            out.contains(&format!("Saved command patcher stage '{name}'")),
            "{out}"
        );
    }
    let synth = ok(&patcher(
        &fx,
        &[
            "add-synthesis",
            "synth",
            "--executable",
            exe,
            "--pipeline-settings",
            settings.to_str().unwrap(),
            "--synthesis-profile",
            "Default",
            "--output-mod",
            "out-synth",
            "--order",
            "3",
        ],
    ));
    assert!(
        synth.contains("Saved Synthesis patcher stage 'synth'"),
        "{synth}"
    );
    fx
}

fn stage_lines(fx: &Fixture) -> Vec<String> {
    ok(&patcher(fx, &["list"]))
        .lines()
        .skip(1)
        .map(|line| line.trim().to_string())
        .collect()
}

#[test]
fn stages_can_be_disabled_reordered_validated_and_removed() {
    let fx = sandbox();
    assert_eq!(
        stage_lines(&fx),
        [
            "first [command] enabled order=1 -> out-first",
            "second [command] enabled order=2 -> out-second",
            "synth [synthesis-cli] enabled order=3 -> out-synth",
        ]
    );

    ok(&patcher(&fx, &["disable", "second"]));
    assert!(stage_lines(&fx)[1].contains("second [command] disabled"));
    ok(&patcher(&fx, &["enable", "second"]));

    ok(&patcher(&fx, &["reorder", "synth", "first", "second"]));
    let order: Vec<String> = stage_lines(&fx)
        .iter()
        .map(|line| line.split_whitespace().next().unwrap().to_string())
        .collect();
    assert_eq!(order, ["synth", "first", "second"]);

    assert!(ok(&patcher(&fx, &["validate"])).contains("Validated 3 enabled patcher stage(s)"));

    assert!(ok(&patcher(&fx, &["remove", "synth"])).contains("Removed patcher stage 'synth'"));
    assert_eq!(stage_lines(&fx).len(), 2);
    assert!(err(&patcher(&fx, &["remove", "synth"])).contains("no patcher stage named 'synth'"));
}

#[test]
fn reorder_must_list_every_stage_exactly_once() {
    let fx = sandbox();
    let before = stage_lines(&fx);

    assert!(err(&patcher(&fx, &["reorder", "second"])).contains("every patcher stage"));
    assert!(
        err(&patcher(&fx, &["reorder", "synth", "first", "typo"])).contains("every patcher stage")
    );
    assert!(
        err(&patcher(
            &fx,
            &["reorder", "first", "first", "second", "synth"]
        ))
        .contains("every patcher stage")
    );
    assert_eq!(
        stage_lines(&fx),
        before,
        "a rejected reorder changes nothing"
    );
}
