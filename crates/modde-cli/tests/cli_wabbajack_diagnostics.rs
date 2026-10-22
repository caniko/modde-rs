//! `modde wabbajack analyze-diagnostics` over JSONL written by the real
//! install diagnostics recorder.

mod common;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use common::Fixture;
use modde_sources::wabbajack::diagnostics::{WabbajackDiagnostics, WabbajackDiagnosticsOptions};
use serde_json::{Value, json};

fn analyze(fx: &Fixture, dir: &Path, json: bool) -> std::process::Output {
    let mut cmd = fx.cmd();
    cmd.args(["wabbajack", "analyze-diagnostics", dir.to_str().unwrap()]);
    if json {
        cmd.arg("--json");
    }
    cmd.output().expect("spawn modde")
}

fn ok(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test]
async fn summarises_recorded_heartbeats_and_archive_batches() {
    let fx = Fixture::new();
    let dir = fx.root().join("diagnostics");
    let diagnostics = WabbajackDiagnostics::new(WabbajackDiagnosticsOptions {
        dir: dir.clone(),
        interval: Duration::from_millis(5),
        stall_warn: Duration::ZERO,
        stall_abort: Duration::from_secs(3600),
    })
    .await
    .unwrap();
    diagnostics.set_phase("download");
    let heartbeat = diagnostics.spawn_heartbeat(Arc::new(|| 3 * 1024 * 1024));
    let heartbeats = dir.join("heartbeat.jsonl");
    for _ in 0..500 {
        let lines = std::fs::read_to_string(&heartbeats).map_or(0, |s| s.lines().count());
        if lines >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    heartbeat.abort();

    // Heartbeats with no finished batch point at the pre-extraction wall.
    let early = ok(&analyze(&fx, &dir, false));
    assert!(early.contains("last phase: download\n"), "{early}");
    assert!(early.contains("byte cache 3.0 MiB"), "{early}");
    assert!(
        early.contains("bottleneck: archive trust/download verification wall"),
        "{early}"
    );

    let record = serde_json::from_value(json!({
        "kind": "archive-batch", "unix_ms": 1, "archive_hash": "0000000000000abc",
        "archive_size_bytes": 100, "directive_count": 2, "patch_count": 1,
        "elapsed_ms": 2500, "extraction_ms": 1000, "patch_ms": 500,
        "extracted_patch_source_bytes": 0, "byte_cache_used_before": 0,
        "byte_cache_used_after": 0, "success_count": 1, "error_count": 1,
        "first_error": "boom", "rss_before_kib": 1024, "rss_after_kib": 3072,
    }))
    .unwrap();
    diagnostics.record_archive_batch(&record).await.unwrap();

    let summary = ok(&analyze(&fx, &dir, false));
    assert!(!summary.contains("bottleneck"), "{summary}");
    assert!(
        summary.contains("0000000000000abc elapsed 2.5s trust 0.0s extract 1.0s patch 0.5s"),
        "{summary}"
    );
    assert!(
        summary.contains("rss-delta +2.0 MiB swap-delta n/a errors 1"),
        "{summary}"
    );
    assert!(summary.contains("first error: boom"), "{summary}");

    let json: Value = serde_json::from_str(&ok(&analyze(&fx, &dir, true))).unwrap();
    assert_eq!(json["archive_batch_count"], 1);
    assert_eq!(json["last_phase"], "download");
    assert_eq!(json["slowest_batches"][0]["rss_delta_kib"], 2048);
}

#[test]
fn empty_and_malformed_diagnostics() {
    let fx = Fixture::new();
    let empty = ok(&analyze(&fx, fx.root(), false));
    assert!(
        empty.contains("heartbeats: 0, archive batches: 0"),
        "{empty}"
    );
    assert!(empty.contains("last phase: unknown"), "{empty}");
    assert!(empty.contains("peaks: rss n/a, swap n/a"), "{empty}");

    std::fs::write(fx.root().join("archive-batches.jsonl"), "\nnot json\n").unwrap();
    let output = analyze(&fx, fx.root(), false);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("archive-batches.jsonl line 2"), "{stderr}");
}
