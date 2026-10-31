use super::super::admission::acquire_apply_memory;
use super::*;
use memory_admission::provider::{
    MemoryProvider, MemoryStats, ProviderError, SharedMemoryProvider,
};
use memory_admission::weighted::{AsyncWeightedAdmissionGate, WeightedConfig};
use std::sync::atomic::AtomicU64;
use std::time::Duration;

const GIB: u64 = 1 << 30;
const WEIGHT: u64 = 8 << 20;

pub(super) struct FixtureMemoryProvider {
    total: u64,
    available: AtomicU64,
}

impl MemoryProvider for FixtureMemoryProvider {
    fn used_fraction(&self) -> Result<f64, ProviderError> {
        Ok(self.stats()?.used_fraction())
    }

    fn stats(&self) -> Result<MemoryStats, ProviderError> {
        Ok(MemoryStats {
            total_bytes: self.total,
            available_bytes: self.available.load(Ordering::Relaxed),
            page_cache_bytes: 0,
        })
    }
}

pub(super) fn fixture_provider() -> SharedMemoryProvider {
    provider(8 * GIB, 7 * GIB)
}

fn provider(total: u64, available: u64) -> Arc<FixtureMemoryProvider> {
    Arc::new(FixtureMemoryProvider {
        total,
        available: AtomicU64::new(available),
    })
}

fn config() -> WeightedConfig {
    WeightedConfig {
        safety_reserve_bytes: 2 * GIB,
        stats_max_age: Duration::ZERO,
        base: memory_admission::Config {
            poll_interval: Duration::from_millis(1),
            ..Default::default()
        },
        ..Default::default()
    }
}

#[tokio::test(start_paused = true)]
async fn admission_rejects_weight_that_cannot_fit_after_reserve() {
    let provider: SharedMemoryProvider = provider(GIB, GIB);
    let config = config();
    let gate = AsyncWeightedAdmissionGate::new(config.clone(), Arc::clone(&provider));
    let result = tokio::time::timeout(
        Duration::from_millis(100),
        acquire_apply_memory(
            &gate,
            &provider,
            &config,
            WEIGHT,
            Duration::from_secs(300),
            || Ok(()),
        ),
    )
    .await
    .expect("impossible admission must fail before its wait deadline");
    assert!(
        result
            .err()
            .expect("reserve cannot fit")
            .to_string()
            .contains("cannot fit")
    );
}

#[tokio::test(start_paused = true)]
async fn admission_timeout_can_retry_after_capacity_recovers() {
    let concrete = provider(8 * GIB, GIB);
    let provider: SharedMemoryProvider = concrete.clone();
    let config = config();
    let gate = AsyncWeightedAdmissionGate::new(config.clone(), Arc::clone(&provider));
    let result = acquire_apply_memory(
        &gate,
        &provider,
        &config,
        WEIGHT,
        Duration::from_millis(20),
        || Ok(()),
    )
    .await;
    assert!(
        result
            .err()
            .expect("busy gate must time out")
            .to_string()
            .contains("timed out")
    );
    concrete.available.store(7 * GIB, Ordering::Relaxed);
    let permit = acquire_apply_memory(
        &gate,
        &provider,
        &config,
        WEIGHT,
        Duration::from_secs(1),
        || Ok(()),
    )
    .await
    .expect("a timed-out wait must not retain a permit");
    drop(permit);
}

#[tokio::test(start_paused = true)]
async fn admission_wait_checks_diagnostics_abort() {
    let provider: SharedMemoryProvider = provider(8 * GIB, GIB);
    let config = config();
    let gate = AsyncWeightedAdmissionGate::new(config.clone(), Arc::clone(&provider));
    let checks = AtomicUsize::new(0);
    let result = tokio::time::timeout(
        Duration::from_secs(2),
        acquire_apply_memory(
            &gate,
            &provider,
            &config,
            WEIGHT,
            Duration::from_secs(300),
            || {
                if checks.fetch_add(1, Ordering::Relaxed) >= 3 {
                    bail!("requested abort");
                }
                Ok(())
            },
        ),
    )
    .await
    .expect("diagnostics abort must interrupt admission");
    assert_eq!(
        result.err().expect("abort must stop admission").to_string(),
        "requested abort"
    );
}

#[tokio::test(start_paused = true)]
async fn admission_keeps_reserve_when_capacity_is_available() {
    let provider = fixture_provider();
    let config = config();
    let gate = AsyncWeightedAdmissionGate::new(config.clone(), Arc::clone(&provider));
    let permit = acquire_apply_memory(
        &gate,
        &provider,
        &config,
        WEIGHT,
        Duration::from_millis(100),
        || Ok(()),
    )
    .await
    .expect("available capacity should admit ordinary work");
    drop(permit);
}

#[tokio::test(start_paused = true)]
async fn admission_preserves_oversize_gate_policy() {
    let provider: SharedMemoryProvider = provider(GIB, GIB);
    let config = config();
    let gate = AsyncWeightedAdmissionGate::new(config.clone(), Arc::clone(&provider));
    let permit = acquire_apply_memory(
        &gate,
        &provider,
        &config,
        config.max_single_weight_bytes + 1,
        Duration::from_millis(100),
        || Ok(()),
    )
    .await
    .expect("the gate owns its existing oversize/thread-only policy");
    drop(permit);
}

#[tokio::test]
async fn install_admission_failure_preserves_resumable_staging() {
    let dir = tempfile::tempdir().unwrap();
    let game_dir = dir.path().join("game");
    let staging = dir.path().join("staging");
    std::fs::create_dir_all(game_dir.join("Data")).unwrap();
    let bytes = b"game file bytes";
    std::fs::write(game_dir.join("Data/Update.esm"), bytes).unwrap();
    let manifest =
        manifest_with_game_file(xxh64(bytes, 0), "Data/Update.esm", "mods/Base/Update.esm");
    let mut installer = WabbajackInstaller::new(
        manifest,
        dir.path().join("test.wabbajack"),
        dir.path().join("store"),
        staging.clone(),
    );
    installer.set_game_dir(game_dir);
    installer.apply_memory_provider = Some(provider(GIB, GIB));
    let (tx, mut rx) = mpsc::unbounded_channel();
    let result = tokio::time::timeout(Duration::from_secs(2), installer.install(tx))
        .await
        .expect("an impossible install must report its admission failure");
    assert!(
        result
            .expect_err("small scope must fail")
            .to_string()
            .contains("cannot fit")
    );
    while let Ok(progress) = rx.try_recv() {
        assert!(!matches!(progress, InstallProgress::Complete));
    }
    assert!(!staging.join("mods/Base/Update.esm").exists());
    installer.apply_memory_provider = Some(fixture_provider());
    let (tx, _rx) = mpsc::unbounded_channel();
    installer
        .install(tx)
        .await
        .expect("same staging must remain resumable");
    assert_eq!(
        std::fs::read(staging.join("mods/Base/Update.esm")).unwrap(),
        bytes
    );
}
