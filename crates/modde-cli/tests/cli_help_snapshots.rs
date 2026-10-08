//! Snapshot tests for CLI help and error output.
//!
//! These pin the user-visible text emitted by `modde --help`, key
//! subcommand `--help` outputs, and the parse-error message for the
//! no-args invocation. A diff in any of them is a deliberate UX change
//! and forces a snapshot review (`cargo insta review`).
//!
//! Snapshots live in `crates/modde-cli/tests/snapshots/`.

mod common;

use common::Fixture;

fn stdout(cmd_args: &[&str]) -> String {
    let fx = Fixture::new();
    // Help output echoes the resolved value of `--data-dir`'s `env=`
    // annotation. Clearing the var here keeps the snapshot stable
    // regardless of the tempdir path the fixture chose. Help/error
    // commands never touch the data dir, so isolation is irrelevant.
    let output = fx
        .cmd()
        .env_remove("MODDE_DATA_DIR")
        .args(cmd_args)
        .output()
        .expect("spawn modde");
    // Help goes to stdout, parse errors go to stderr — concatenate so
    // we capture whichever the invocation produces. Order is fixed
    // (stdout first) so the snapshot is deterministic.
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        combined.push_str("\n--- stderr ---\n");
        combined.push_str(&stderr);
    }
    combined
}

fn without_optional_commands(mut output: String) -> String {
    for (command, enabled) in [
        (
            "  gui         Launch the graphical user interface\n",
            cfg!(feature = "gui"),
        ),
        (
            "  loot        LOOT masterlist integration (Bethesda plugin sorting)\n",
            cfg!(feature = "bethesda"),
        ),
    ] {
        assert_eq!(output.matches(command).count(), usize::from(enabled));
        output = output.replace(command, "");
    }
    output
}

#[test]
fn snapshot_top_level_help() {
    if cfg!(all(feature = "gui", feature = "bethesda")) {
        insta::assert_snapshot!(stdout(&["--help"]));
    } else {
        insta::assert_snapshot!(
            "snapshot_top_level_help_without_optional_commands",
            without_optional_commands(stdout(&["--help"]))
        );
    }
}

#[test]
fn snapshot_install_help() {
    insta::assert_snapshot!(stdout(&["install", "--help"]));
}

#[test]
fn snapshot_profile_help() {
    insta::assert_snapshot!(stdout(&["profile", "--help"]));
}

#[test]
fn snapshot_game_help() {
    insta::assert_snapshot!(stdout(&["game", "--help"]));
}

#[test]
fn snapshot_game_add_help() {
    insta::assert_snapshot!(stdout(&["game", "add", "--help"]));
}

#[test]
fn snapshot_game_export_help() {
    insta::assert_snapshot!(stdout(&["game", "export", "--help"]));
}

#[test]
fn snapshot_game_import_help() {
    insta::assert_snapshot!(stdout(&["game", "import", "--help"]));
}

#[test]
fn snapshot_game_import_profile_help() {
    insta::assert_snapshot!(stdout(&["game", "import-profile", "--help"]));
}

#[test]
fn snapshot_nxm_help() {
    insta::assert_snapshot!(stdout(&["nxm", "--help"]));
}

#[test]
fn snapshot_doctor_help() {
    insta::assert_snapshot!(stdout(&["doctor", "--help"]));
}

#[test]
fn snapshot_doctor_explain_help() {
    insta::assert_snapshot!(stdout(&["doctor", "explain", "--help"]));
}

#[test]
fn snapshot_tool_doctor_help() {
    insta::assert_snapshot!(stdout(&["tool", "doctor", "--help"]));
}

#[test]
fn snapshot_no_args_error() {
    // Capture clap's "missing subcommand" error so we notice when its
    // wording or exit code changes underneath us.
    if cfg!(all(feature = "gui", feature = "bethesda")) {
        insta::assert_snapshot!(stdout(&[]));
    } else {
        insta::assert_snapshot!(
            "snapshot_no_args_error_without_optional_commands",
            without_optional_commands(stdout(&[]))
        );
    }
}

#[test]
fn snapshot_unknown_subcommand_error() {
    insta::assert_snapshot!(stdout(&["nonexistent"]));
}
