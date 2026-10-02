use anyhow::{Result, bail};
use memory_admission::provider::SharedMemoryProvider;
use memory_admission::weighted::{AsyncWeightedAdmissionGate, WeightedConfig, WeightedPermit};
use std::time::Duration;

pub(super) const APPLY_ADMISSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Preserve the weighted gate's reserve and throttle policy while bounding a
/// wait that cannot make progress. Dropping the acquire future never retains a
/// permit; diagnostics cancellation remains observable while the gate waits.
pub(super) async fn acquire_apply_memory(
    gate: &AsyncWeightedAdmissionGate,
    provider: &SharedMemoryProvider,
    config: &WeightedConfig,
    weight_bytes: u64,
    timeout: Duration,
    check_abort: impl Fn() -> Result<()>,
) -> Result<WeightedPermit> {
    check_abort()?;
    if weight_bytes <= config.max_single_weight_bytes
        && let Ok(stats) = provider.stats()
        && stats
            .total_bytes
            .saturating_sub(config.safety_reserve_bytes)
            < weight_bytes
    {
        tracing::warn!(
            weight_bytes,
            total_bytes = stats.total_bytes,
            safety_reserve_bytes = config.safety_reserve_bytes,
            "Wabbajack apply memory reservation cannot fit in the memory scope"
        );
        bail!(
            "Wabbajack apply memory reservation cannot fit: scope total {} bytes, safety reserve {} bytes, requested {} bytes; use a larger memory scope or explicitly configure MODDE_APPLY_SAFETY_RESERVE_GIB",
            stats.total_bytes,
            config.safety_reserve_bytes,
            weight_bytes
        );
    }

    let acquisition = gate.acquire(weight_bytes);
    tokio::pin!(acquisition);
    let deadline = tokio::time::sleep(timeout);
    tokio::pin!(deadline);
    let mut abort_poll = tokio::time::interval(Duration::from_secs(1));
    loop {
        check_abort()?;
        tokio::select! {
            permit = &mut acquisition => return Ok(permit),
            () = &mut deadline => {
                tracing::warn!(
                    weight_bytes,
                    safety_reserve_bytes = config.safety_reserve_bytes,
                    timeout_secs = timeout.as_secs_f64(),
                    "Wabbajack apply memory admission timed out"
                );
                bail!(
                    "Wabbajack apply memory admission timed out after {:.3} seconds while reserving {} bytes; free memory or use a larger memory scope and resume the install",
                    timeout.as_secs_f64(), weight_bytes
                );
            }
            _ = abort_poll.tick() => {}
        }
    }
}
