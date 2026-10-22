//! End-to-end tests for `modde perf` ingest/list/show/compare against
//! seeded pending runs and synthetic `MangoHud` CSV captures.

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::Fixture;
use modde_core::{GameId, ModdeDb, NewPerformanceRun, PerformanceModSnapshot};

const GAME: &str = "stardew-valley";

fn snapshot(mods: &[(&str, &str)]) -> Vec<PerformanceModSnapshot> {
    mods.iter()
        .map(|(mod_id, version)| PerformanceModSnapshot {
            mod_id: (*mod_id).to_string(),
            version: Some((*version).to_string()),
            enabled: true,
        })
        .collect()
}

/// Seed pending runs `run-base` and `run-cand` with different mod sets.
async fn seeded() -> Fixture {
    let fx = Fixture::new();
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    for (run_id, label, mods) in [
        ("run-base", "base", snapshot(&[("a", "1.0"), ("b", "1.0")])),
        ("run-cand", "cand", snapshot(&[("a", "2.0"), ("c", "1.0")])),
    ] {
        db.create_performance_run(&NewPerformanceRun {
            run_id: run_id.to_string(),
            game_id: GameId::from(GAME),
            profile_id: None,
            profile_name: "main".to_string(),
            mod_snapshot: mods,
            experiment_depth: 0,
            label: Some(label.to_string()),
        })
        .await
        .unwrap();
    }
    fx
}

/// A 40-second capture at a constant `fps`, one sample per second.
fn capture(fx: &Fixture, name: &str, fps: f64) -> PathBuf {
    let mut csv = String::from("fps,frametime,time\n");
    for second in 0..40 {
        csv.push_str(&format!("{fps},{},{second}\n", 1000.0 / fps));
    }
    let path = fx.root().join(name);
    std::fs::write(&path, csv).unwrap();
    path
}

fn perf(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("perf")
        .args(args)
        .output()
        .expect("spawn modde perf")
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

fn ingest(fx: &Fixture, run: &str, csv: &Path) -> Output {
    perf(
        fx,
        &[
            "ingest",
            "--run",
            run,
            "--csv",
            csv.to_str().unwrap(),
            "--warmup-seconds",
            "5",
        ],
    )
}

#[tokio::test]
async fn ingest_summarises_captures_after_warmup() {
    let fx = seeded().await;

    let pending = ok(&perf(&fx, &["show", "run-base"]));
    assert!(pending.contains("Status: pending"), "{pending}");
    assert!(pending.contains("Median FPS: unavailable"), "{pending}");

    let ingested = ok(&ingest(&fx, "run-base", &capture(&fx, "base.csv", 60.0)));
    assert!(
        ingested.contains("Ingested performance run: run-base"),
        "{ingested}"
    );
    assert!(ingested.contains("Samples: 35"), "{ingested}");
    assert!(ingested.contains("p99 frame time: 16.67 ms"), "{ingested}");

    let shown = ok(&perf(&fx, &["show", "run-base"]));
    assert!(shown.contains("Status: complete"), "{shown}");
    assert!(shown.contains("Label: base"), "{shown}");

    let list = ok(&perf(&fx, &["list", "--game", GAME]));
    assert!(
        list.contains("run-base") && list.contains("1% low=60.00 fps"),
        "{list}"
    );
    assert!(
        list.contains("run-cand") && list.contains("pending"),
        "{list}"
    );
}

#[tokio::test]
async fn compare_reports_metric_deltas_and_mod_changes() {
    let fx = seeded().await;
    ok(&ingest(&fx, "run-base", &capture(&fx, "base.csv", 60.0)));
    ok(&ingest(&fx, "run-cand", &capture(&fx, "cand.csv", 40.0)));

    let compare = ok(&perf(
        &fx,
        &[
            "compare",
            "--baseline",
            "run-base",
            "--candidate",
            "run-cand",
        ],
    ));
    assert!(
        compare.contains("Median FPS: 60.00 -> 40.00 fps (-20.00, -33.3%)"),
        "{compare}"
    );
    assert!(
        compare.contains("p99 frame time: 16.67 -> 25.00 ms (+8.33, +50.0%)"),
        "{compare}"
    );
    assert!(
        compare.contains(
            "Added:\n    c (1.0)\n  Removed:\n    b (1.0)\n  Changed:\n    a: 1.0 -> 2.0"
        ),
        "{compare}"
    );
}

#[tokio::test]
async fn ingest_rejects_bad_captures_and_unknown_runs() {
    let fx = seeded().await;

    let not_mangohud = fx.root().join("bad.csv");
    std::fs::write(&not_mangohud, "foo,bar\n1,2\n").unwrap();
    assert!(err(&ingest(&fx, "run-base", &not_mangohud)).contains("parsing performance capture"));

    let csv = capture(&fx, "base.csv", 60.0);
    assert!(err(&ingest(&fx, "nope", &csv)).contains("performance run not found: nope"));
    assert!(err(&perf(&fx, &["show", "nope"])).contains("performance run not found: nope"));

    // A recorded capture configuration pins the warmup used for analysis.
    let config = fx
        .data_dir()
        .join("performance")
        .join(GAME)
        .join("run-base")
        .join("configuration.json");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, r#"{"warmup_seconds": 30.0}"#).unwrap();
    assert!(err(&ingest(&fx, "run-base", &csv)).contains("use the recorded warmup (30)"));
}
