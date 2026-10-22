//! End-to-end tests for `modde bisect` with the manual oracle against a
//! fake Stardew Valley install. The sandbox has no launchable game, so each
//! `bisect run` records its candidate and then fails to launch; results are
//! entered with `bisect mark`, exactly as a manual run would.

#![cfg(feature = "stardew")]

mod common;

use std::path::PathBuf;
use std::process::Output;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::settings::AppSettings;
use modde_core::{GameId, ModdeDb};

const GAME: &str = "stardew-valley";

/// A configured install and a `broken` profile with mods `a`..`d`.
async fn sandbox() -> Fixture {
    let fx = Fixture::new();
    let install = fx.root().join("install");
    std::fs::create_dir_all(&install).unwrap();
    let mut settings = AppSettings::default();
    settings.set_game_path(&GameId::from(GAME), install);
    settings.save_to(&fx.home().join(".config/modde/settings.toml"));
    std::fs::create_dir_all(fx.home().join(".config/StardewValley/Saves")).unwrap();

    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let mods = ["a", "b", "c", "d"]
        .into_iter()
        .map(|id| EnabledMod {
            mod_id: id.to_string(),
            enabled: true,
            ..Default::default()
        })
        .collect();
    db.create_profile(&Profile {
        id: None,
        name: "broken".to_string(),
        game_id: GameId::from(GAME),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec::SmallVec::new(),
        load_order_lock: None,
    })
    .await
    .unwrap();
    fx
}

fn modde(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd().args(args).output().expect("spawn modde")
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

/// Start a manual session and return its id.
fn start(fx: &Fixture, extra: &[&str]) -> String {
    let mut args = vec![
        "bisect",
        "start",
        "--game",
        GAME,
        "--profile",
        "broken",
        "--oracle",
        "manual",
    ];
    args.extend_from_slice(extra);
    let out = ok(&modde(fx, &args));
    out.lines()
        .next()
        .and_then(|line| line.strip_prefix("Started bisect session: "))
        .expect("session id")
        .to_string()
}

fn candidate_profiles(fx: &Fixture) -> Vec<String> {
    ok(&modde(fx, &["profile", "list"]))
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| name.starts_with("__bisect_"))
        .map(str::to_string)
        .collect()
}

#[tokio::test]
async fn manual_bisect_narrows_down_to_the_culprit() {
    let fx = sandbox().await;
    let id = start(&fx, &["--force-save-risk"]);

    let status = ok(&modde(&fx, &["bisect", "status", &id]));
    assert!(status.contains("Status: active"), "{status}");
    assert!(status.contains("Suspects: 4"), "{status}");

    // The candidate is recorded before the (impossible) launch.
    assert!(err(&modde(&fx, &["bisect", "run", &id])).contains("direct executable"));
    let waiting = ok(&modde(&fx, &["bisect", "status", &id]));
    assert!(waiting.contains("Status: waiting"), "{waiting}");
    assert!(
        waiting.contains("result=pending disabled=a, b"),
        "{waiting}"
    );
    assert!(err(&modde(&fx, &["bisect", "run", &id])).contains("is waiting for a result"));

    let next = ok(&modde(
        &fx,
        &["bisect", "mark", &id, "bad", "--notes", "crashed"],
    ));
    assert!(next.contains("Next candidate"), "{next}");
    let history = ok(&modde(&fx, &["bisect", "history", &id]));
    assert!(history.contains("Current suspects: c, d"), "{history}");
    assert!(history.contains("notes: crashed"), "{history}");

    err(&modde(&fx, &["bisect", "run", &id]));
    let culprit = ok(&modde(&fx, &["bisect", "mark", &id, "good"]));
    assert!(culprit.contains("Culprit: c"), "{culprit}");
    let history = ok(&modde(&fx, &["bisect", "history", &id]));
    assert!(history.contains("Status: complete"), "{history}");
    assert!(err(&modde(&fx, &["bisect", "run", &id])).contains("not active"));
}

#[tokio::test]
async fn refused_unsafe_candidates_leave_no_profiles_behind() {
    let fx = sandbox().await;
    let id = start(&fx, &[]);

    assert!(err(&modde(&fx, &["bisect", "mark", &id, "bad"])).contains("has no pending candidate"));
    assert!(err(&modde(&fx, &["bisect", "run", &id])).contains("refusing unsafe bisect step"));
    assert_eq!(candidate_profiles(&fx), [] as [&str; 0]);

    assert!(ok(&modde(&fx, &["bisect", "abort", &id])).contains("Aborted bisect session"));
    assert!(ok(&modde(&fx, &["bisect", "status", &id])).contains("Status: aborted"));
}

#[tokio::test]
async fn abort_removes_recorded_candidates() {
    let fx = sandbox().await;
    let id = start(&fx, &["--force-save-risk"]);
    err(&modde(&fx, &["bisect", "run", &id]));
    assert_eq!(candidate_profiles(&fx).len(), 1);

    ok(&modde(&fx, &["bisect", "abort", &id]));
    assert_eq!(candidate_profiles(&fx), [] as [&str; 0]);
}

#[tokio::test]
async fn unknown_sessions_and_profiles_are_rejected() {
    let fx = sandbox().await;
    assert!(err(&modde(&fx, &["bisect", "status", "nope"])).contains("bisect session not found"));
    let missing = modde(
        &fx,
        &[
            "bisect",
            "start",
            "--game",
            GAME,
            "--profile",
            "nope",
            "--oracle",
            "manual",
        ],
    );
    assert!(!missing.status.success());
}
