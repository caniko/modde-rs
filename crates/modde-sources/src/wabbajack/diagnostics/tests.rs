use super::*;

#[test]
fn memory_saturation_requires_high_and_swap_pressure() {
    let saturated = CgroupSnapshot {
        memory_current: Some(100),
        memory_high: Some(100),
        memory_swap_current: Some(95),
        memory_swap_max: Some(100),
        ..CgroupSnapshot::default()
    };
    assert!(memory_is_saturated(Some(&saturated)));

    let no_swap = CgroupSnapshot {
        memory_current: Some(100),
        memory_high: Some(100),
        memory_swap_current: Some(10),
        memory_swap_max: Some(100),
        ..CgroupSnapshot::default()
    };
    assert!(!memory_is_saturated(Some(&no_swap)));

    let no_high = CgroupSnapshot {
        memory_current: Some(99),
        memory_high: Some(100),
        memory_swap_current: Some(95),
        memory_swap_max: Some(100),
        ..CgroupSnapshot::default()
    };
    assert!(!memory_is_saturated(Some(&no_high)));
}

#[test]
fn cgroup_pressure_without_cgroup_is_false_or_bounded() {
    let _ = cgroup_memory_pressure_high(0.80);
}

#[tokio::test]
async fn heartbeat_writes_json_line() {
    let temp = tempfile::tempdir().unwrap();
    let diagnostics = WabbajackDiagnostics::new(WabbajackDiagnosticsOptions {
        dir: temp.path().to_path_buf(),
        interval: Duration::from_secs(1),
        stall_warn: Duration::from_mins(1),
        stall_abort: Duration::from_mins(2),
    })
    .await
    .unwrap();

    diagnostics.set_phase("apply-archive-batches");
    diagnostics.record_progress(ProgressEvent::Other);
    diagnostics.write_heartbeat(1234).await.unwrap();

    let heartbeat = std::fs::read_to_string(temp.path().join("heartbeat.jsonl")).unwrap();
    let record: serde_json::Value = serde_json::from_str(heartbeat.trim()).unwrap();
    assert_eq!(record["kind"], "heartbeat");
    assert_eq!(record["phase"], "apply-archive-batches");
    assert_eq!(record["byte_cache_used"], 1234);
}

#[tokio::test]
async fn archive_batch_record_writes_json_line() {
    let temp = tempfile::tempdir().unwrap();
    let diagnostics = WabbajackDiagnostics::new(WabbajackDiagnosticsOptions {
        dir: temp.path().to_path_buf(),
        interval: Duration::from_secs(1),
        stall_warn: Duration::from_mins(1),
        stall_abort: Duration::from_mins(2),
    })
    .await
    .unwrap();
    diagnostics
        .record_archive_batch(&ArchiveBatchRecord {
            kind: "archive_batch".to_string(),
            unix_ms: 1,
            archive_hash: "0000000000000001".to_string(),
            archive_size_bytes: 2,
            directive_count: 3,
            patch_count: 1,
            elapsed_ms: 4,
            trust_check_ms: 0,
            extraction_ms: 5,
            patch_ms: 6,
            prune_ms: 0,
            extracted_patch_source_bytes: 7,
            sidecar_hit: false,
            streamed_hash_bytes: 0,
            memory_archive_hit: false,
            disk_archive_fallback: false,
            pruned_bytes: 0,
            byte_cache_used_before: 8,
            byte_cache_used_after: 9,
            success_count: 10,
            error_count: 0,
            first_error: None,
            rss_before_kib: Some(11),
            rss_after_kib: Some(12),
            swap_before_kib: Some(13),
            swap_after_kib: Some(14),
        })
        .await
        .unwrap();
    let records = std::fs::read_to_string(temp.path().join("archive-batches.jsonl")).unwrap();
    let record: ArchiveBatchRecord = serde_json::from_str(records.trim()).unwrap();
    assert_eq!(record.archive_hash, "0000000000000001");
    assert_eq!(record.extracted_patch_source_bytes, 7);
}

fn heartbeats(dir: &Path) -> Vec<HeartbeatRecord> {
    std::fs::read_to_string(dir.join("heartbeat.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn heartbeats_track_active_work_until_guards_drop() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("nested");
    let diagnostics = WabbajackDiagnostics::new(WabbajackDiagnosticsOptions {
        dir: dir.clone(),
        interval: Duration::from_millis(5),
        stall_warn: Duration::ZERO,
        stall_abort: Duration::from_mins(2),
    })
    .await
    .unwrap();

    let batch = diagnostics.start_archive_batch(0xabc, 2, 1, 100);
    let patch = diagnostics.start_patch("Data/x.esp", "p1", 10, 20);
    diagnostics.write_heartbeat(0).await.unwrap();
    drop((patch, batch));
    diagnostics.record_progress(ProgressEvent::ArchiveBatchComplete);
    diagnostics.record_progress(ProgressEvent::CreateBsaComplete);
    diagnostics.write_heartbeat(0).await.unwrap();

    let records = heartbeats(&dir);
    assert_eq!(
        records[0].active_archive_batches[0].archive_hash,
        "0000000000000abc"
    );
    assert_eq!(records[0].active_patches[0].patch_id, "p1");
    assert!(matches!(
        (
            records[1].active_archive_batches.as_slice(),
            records[1].active_patches.as_slice()
        ),
        ([], [])
    ));
    assert_eq!(
        (
            records[1].progress_events,
            records[1].completed_archive_batches,
            records[1].completed_create_bsa
        ),
        (2, 1, 1)
    );

    let heartbeat = diagnostics.spawn_heartbeat(Arc::new(|| 7));
    for _ in 0..500 {
        if heartbeats(&dir).len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    heartbeat.abort();
    assert_eq!(heartbeats(&dir)[2].byte_cache_used, 7);

    diagnostics.check_abort().unwrap();
    diagnostics.inner.abort.store(true, Ordering::Relaxed);
    assert!(diagnostics.check_abort().is_err());
}
