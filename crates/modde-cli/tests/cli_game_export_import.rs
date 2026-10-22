mod common;

use common::Fixture;

fn add_game(fx: &Fixture, id: &str) -> std::process::Output {
    fx.cmd()
        .args([
            "game",
            "add",
            id,
            "--display-name",
            "Custom Game",
            "--executable-dir",
            "bin/x64",
        ])
        .output()
        .expect("spawn modde game add")
}

#[test]
fn export_user_game_round_trips_via_import() {
    let fx = Fixture::new();
    let out = add_game(&fx, "round-trip");
    assert!(
        out.status.success(),
        "add failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let export_path = fx.root().join("round-trip.toml");
    let exported = fx
        .cmd()
        .args([
            "game",
            "export",
            "round-trip",
            "--output",
            export_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn modde game export");
    assert!(
        exported.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&exported.stderr)
    );

    let removed = fx
        .cmd()
        .args(["game", "remove", "round-trip", "--yes"])
        .output()
        .expect("spawn modde game remove");
    assert!(
        removed.status.success(),
        "remove failed: {}",
        String::from_utf8_lossy(&removed.stderr)
    );

    let imported = fx
        .cmd()
        .args(["game", "import", export_path.to_str().unwrap()])
        .output()
        .expect("spawn modde game import");
    assert!(
        imported.status.success(),
        "import failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );

    let list = fx
        .cmd()
        .args(["game", "list"])
        .output()
        .expect("spawn modde game list");
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("round-trip"), "missing game id:\n{stdout}");
}

#[test]
#[cfg(feature = "bethesda")]
fn import_rejects_built_in_id_collision() {
    let fx = Fixture::new();
    let export_path = fx.root().join("skyrim.toml");
    let out = fx
        .cmd()
        .args([
            "game",
            "export",
            "skyrim-se",
            "--output",
            export_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn modde game export");
    assert!(
        out.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let imported = fx
        .cmd()
        .args(["game", "import", export_path.to_str().unwrap(), "--force"])
        .output()
        .expect("spawn modde game import");
    assert!(!imported.status.success(), "built-in import should fail");
    let stderr = String::from_utf8_lossy(&imported.stderr);
    assert!(
        stderr.contains("collides with a built-in game"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
#[cfg(feature = "ue4")]
fn export_with_optiscaler_includes_profiles() {
    let fx = Fixture::new();
    let export_path = fx.root().join("stellar-blade.toml");
    let out = fx
        .cmd()
        .args([
            "game",
            "export",
            "stellar-blade",
            "--with-optiscaler",
            "--output",
            export_path.to_str().unwrap(),
        ])
        .output()
        .expect("spawn modde game export");
    assert!(
        out.status.success(),
        "export failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let content = std::fs::read_to_string(export_path).expect("read export");
    assert!(
        content.contains("[optiscaler]"),
        "missing optiscaler section:\n{content}"
    );
    assert!(
        content.contains("[[optiscaler.profile]]"),
        "missing profile array:\n{content}"
    );
}

#[test]
fn import_profile_attaches_to_built_in_game() {
    let fx = Fixture::new();
    let profile_path = fx.root().join("profile.toml");
    std::fs::write(
        &profile_path,
        r#"
            [optiscaler]
            [[optiscaler.profile]]
            id = "test-profile"
            name = "Test Profile"
            source_url = "https://example.invalid/profile"
            tested_optiscaler_version = "1.0"
            proxy_dll = "dxgi.dll"
            notes = ""
        "#,
    )
    .unwrap();

    let imported = fx
        .cmd()
        .args([
            "game",
            "import-profile",
            profile_path.to_str().unwrap(),
            "--for",
            "stellar-blade",
        ])
        .output()
        .expect("spawn modde game import-profile");
    assert!(
        imported.status.success(),
        "import-profile failed: {}",
        String::from_utf8_lossy(&imported.stderr)
    );
    assert!(
        fx.data_dir()
            .join("games/stellar-blade.optiscaler.toml")
            .exists()
    );
}

#[test]
fn game_commands_reject_bad_input_and_respect_the_remove_prompt() {
    let fx = Fixture::new();
    let run = |args: &[&str], stdin: &str| {
        fx.cmd()
            .args(args)
            .write_stdin(stdin)
            .output()
            .expect("spawn modde game")
    };
    let stderr =
        |output: &std::process::Output| String::from_utf8_lossy(&output.stderr).into_owned();
    let stdout =
        |output: &std::process::Output| String::from_utf8_lossy(&output.stdout).into_owned();

    assert!(
        stderr(&run(&["game", "export", "no-such-game"], ""))
            .contains("unknown game 'no-such-game'")
    );
    assert!(
        stderr(&run(&["game", "show", "no-such-game"], "")).contains("unknown game 'no-such-game'")
    );

    let missing = fx.root().join("missing.toml");
    let bad = fx.root().join("bad.toml");
    std::fs::write(&bad, "id = [unterminated").unwrap();
    for (path, expected) in [(&missing, "failed to read"), (&bad, "failed to parse")] {
        let path = path.to_str().unwrap();
        assert!(stderr(&run(&["game", "import", path], "")).contains(expected));
        assert!(
            stderr(&run(
                &["game", "import-profile", path, "--for", "custom"],
                ""
            ))
            .contains(expected)
        );
    }

    assert!(add_game(&fx, "dup").status.success());
    let spec = fx.root().join("dup.toml");
    let spec_path = spec.to_str().unwrap();
    assert!(
        run(&["game", "export", "dup", "--output", spec_path], "")
            .status
            .success()
    );
    assert!(
        stderr(&run(&["game", "import", spec_path], ""))
            .contains("already exists; re-run with --force")
    );
    assert!(
        run(&["game", "import", spec_path, "--force"], "")
            .status
            .success()
    );

    let profile = fx.root().join("profile.toml");
    std::fs::write(&profile, "[optiscaler]\nprofile = []\n").unwrap();
    let profile_path = profile.to_str().unwrap();
    let first = run(
        &["game", "import-profile", profile_path, "--for", "custom"],
        "",
    );
    assert!(
        stdout(&first).contains("Imported 0 OptiScaler profiles"),
        "{}",
        stderr(&first)
    );
    assert!(
        stderr(&run(
            &["game", "import-profile", profile_path, "--for", "custom"],
            ""
        ))
        .contains("already exists; re-run with --force")
    );

    let declined = run(&["game", "remove", "dup"], "n\n");
    assert!(stdout(&declined).contains("Remove user-defined game 'dup'? [y/N]"));
    assert!(stderr(&declined).contains("aborted"));
    let accepted = run(&["game", "remove", "dup"], "yes\n");
    assert!(stdout(&accepted).contains("Removed user-defined game 'dup'"));
    assert!(stdout(&run(&["game", "list"], "")).contains("No user-defined games configured."));

    let empty = fx.root().join("empty-install");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(
        stdout(&run(&["game", "detect", empty.to_str().unwrap()], ""))
            .contains("No executable-bearing directories found")
    );
}
