//! End-to-end dispatch tests for `modde scan`.
//!
//! These pin the scanner CLI's error and dry-run paths so a future
//! refactor that breaks scanner resolution or argument validation
//! gets caught before it ships. They run fully offline.

mod common;

use common::Fixture;

#[test]
fn scan_rejects_unsupported_game() {
    let fx = Fixture::new();
    let output = fx
        .cmd()
        .args(["scan", "--game", "not-a-real-game"])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "unsupported game must surface as failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported game") || stderr.contains("not-a-real-game"),
        "expected unsupported-game error in stderr; got:\n{stderr}"
    );
}

#[test]
fn scan_rejects_nonexistent_game_dir() {
    let fx = Fixture::new();
    let bogus = fx.root().join("does-not-exist");
    let output = fx
        .cmd()
        .args([
            "scan",
            "--game",
            "cyberpunk2077",
            "--game-dir",
            bogus.to_str().unwrap(),
        ])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "missing --game-dir path must surface as failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let expected = if cfg!(feature = "cyberpunk") {
        "does not exist"
    } else {
        "unsupported game 'cyberpunk2077'"
    };
    assert!(
        stderr.contains(expected),
        "expected '{expected}' message; got:\n{stderr}"
    );
}

#[test]
fn scan_prune_duplicates_requires_manifest() {
    // `--prune-duplicates` is the sharp tool: it deletes profile rows
    // classified as leaked. The handler refuses to run it without a
    // manifest to anchor the classification — pin that gate so a
    // future refactor doesn't accidentally drop it.
    let fx = Fixture::new();
    let game_dir = fx.root().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();

    let output = fx
        .cmd()
        .args([
            "scan",
            "--game",
            "cyberpunk2077",
            "--game-dir",
            game_dir.to_str().unwrap(),
            "--prune-duplicates",
        ])
        .output()
        .expect("spawn modde");
    assert!(
        !output.status.success(),
        "--prune-duplicates without --manifest must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires --manifest"),
        "expected manifest-required message; got:\n{stderr}"
    );
}

#[test]
fn scan_dry_run_on_empty_dir_succeeds() {
    // Empty directory: scanner finds nothing, no manifest, no
    // --import-to → exits 0 with a "use --import-to" hint.
    let fx = Fixture::new();
    let game_dir = fx.root().join("game");
    std::fs::create_dir_all(&game_dir).unwrap();

    let output = fx
        .cmd()
        .args([
            "scan",
            "--game",
            "cyberpunk2077",
            "--game-dir",
            game_dir.to_str().unwrap(),
        ])
        .output()
        .expect("spawn modde");
    if cfg!(feature = "cyberpunk") {
        assert!(
            output.status.success(),
            "empty-dir scan should succeed; stderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("Filesystem scan: 0 mods discovered"),
            "expected zero-mod scan output; got:\n{stdout}"
        );
    } else {
        assert!(!output.status.success(), "disabled game must fail closed");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("unsupported game 'cyberpunk2077'")
        );
    }
}
