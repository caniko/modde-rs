#![allow(clippy::wildcard_imports)]
use super::*;

impl WabbajackInstaller {
    pub(super) async fn write_archive_batch_metrics(
        &self,
        batch: &ArchiveInstallBatch,
        patch_count: usize,
        batch_started: Instant,
        trust_check_ms: u128,
        extraction_ms: u128,
        patch_ms: u128,
        prune_ms: u128,
        extracted_patch_source_bytes: u64,
        trust_stats: ArchiveTrustStats,
        pruned_bytes: u64,
        byte_cache_used_before: u64,
        process_before: ProcessSnapshot,
        results: &[(usize, Result<()>)],
    ) {
        let Some(diagnostics) = &self.diagnostics else {
            return;
        };
        let process_after = current_process_snapshot();
        let record = ArchiveBatchRecord {
            kind: "archive_batch".to_string(),
            unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            archive_hash: format!("{:016x}", batch.archive_hash),
            archive_size_bytes: batch.archive_size_bytes,
            directive_count: batch.directives.len(),
            patch_count,
            elapsed_ms: batch_started.elapsed().as_millis(),
            trust_check_ms,
            extraction_ms,
            patch_ms,
            prune_ms,
            extracted_patch_source_bytes,
            sidecar_hit: trust_stats.sidecar_hit,
            streamed_hash_bytes: trust_stats.streamed_hash_bytes,
            memory_archive_hit: trust_stats.memory_archive_hit,
            disk_archive_fallback: trust_stats.disk_fallback,
            pruned_bytes,
            byte_cache_used_before,
            byte_cache_used_after: self.byte_cache.bytes_used(),
            success_count: results.iter().filter(|(_, result)| result.is_ok()).count(),
            error_count: results.iter().filter(|(_, result)| result.is_err()).count(),
            first_error: results
                .iter()
                .find_map(|(_, result)| result.as_ref().err().map(|error| format!("{error:#}"))),
            rss_before_kib: process_before.vm_rss_kib,
            rss_after_kib: process_after.vm_rss_kib,
            swap_before_kib: process_before.vm_swap_kib,
            swap_after_kib: process_after.vm_swap_kib,
        };
        if let Err(e) = diagnostics.record_archive_batch(&record).await {
            warn!("failed to write Wabbajack archive batch diagnostics: {e:#}");
        }
    }

    pub(super) async fn adopt_existing_staging(
        &self,
        archive_batches: &[ArchiveInstallBatch],
        installs: &[InstallDirective],
    ) -> Result<StagingAdoptionSummary> {
        let mut summary = StagingAdoptionSummary::default();

        for batch in archive_batches {
            if self.archive_batch_sentinel_valid(batch).await {
                continue;
            }
            if self.archive_batch_outputs_exist(batch).await {
                self.write_archive_batch_sentinel(batch).await?;
                summary.archive_batches += 1;
            }
        }

        for (directive_index, directive) in installs.iter().enumerate() {
            let InstallDirective::CreateBSA { temp_id, to, .. } = directive else {
                continue;
            };
            if self
                .create_bsa_sentinel_valid(directive_index, temp_id, to)
                .await
            {
                continue;
            }
            if StagingStore::new(&self.staging_dir)
                .logical_exists(to)
                .await
            {
                self.write_create_bsa_sentinel(directive_index, temp_id, to)
                    .await?;
                summary.create_bsa += 1;
            }
        }

        Ok(summary)
    }

    pub(super) async fn archive_batch_sentinel_valid(&self, batch: &ArchiveInstallBatch) -> bool {
        let path = self.archive_batch_sentinel_path(batch.archive_hash);
        let Ok(data) = tokio::fs::read_to_string(&path).await else {
            return false;
        };
        let Ok(sentinel) = serde_json::from_str::<ArchiveBatchSentinel>(&data) else {
            return false;
        };
        let expected_indices = batch
            .directives
            .iter()
            .map(|d| d.directive_index)
            .collect::<Vec<_>>();
        sentinel.pipeline_version == APPLY_STATE_VERSION
            && sentinel.archive_hash == batch.archive_hash
            && sentinel.archive_size_bytes == batch.archive_size_bytes
            && sentinel.directive_indices == expected_indices
            && self.archive_batch_outputs_exist(batch).await
    }

    pub(super) async fn archive_batch_outputs_exist(&self, batch: &ArchiveInstallBatch) -> bool {
        let staging_store = StagingStore::new(&self.staging_dir);
        for indexed in &batch.directives {
            let (to, size) = match &indexed.directive {
                InstallDirective::FromArchive { to, size, .. }
                | InstallDirective::PatchedFromArchive { to, size, .. } => (to, *size),
                _ => continue,
            };
            // A wrong-sized output (e.g. a whole outer BSA written by an old
            // build) invalidates the batch so it is re-extracted.
            if !staging_store.logical_exists_with_size(to, size).await {
                return false;
            }
        }
        true
    }

    pub(super) async fn write_archive_batch_sentinel(
        &self,
        batch: &ArchiveInstallBatch,
    ) -> Result<()> {
        let path = self.archive_batch_sentinel_path(batch.archive_hash);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let sentinel = ArchiveBatchSentinel {
            pipeline_version: APPLY_STATE_VERSION,
            archive_hash: batch.archive_hash,
            archive_size_bytes: batch.archive_size_bytes,
            directive_indices: batch.directives.iter().map(|d| d.directive_index).collect(),
        };
        tokio::fs::write(&path, serde_json::to_vec_pretty(&sentinel)?).await?;
        Ok(())
    }

    pub(super) fn archive_batch_sentinel_path(&self, archive_hash: u64) -> PathBuf {
        self.staging_dir
            .join("_state/archive-batches")
            .join(format!("{archive_hash:016x}.json"))
    }

    pub(super) async fn create_bsa_sentinel_valid(
        &self,
        directive_index: usize,
        temp_id: &str,
        to: &str,
    ) -> bool {
        let path = self.create_bsa_sentinel_path(directive_index);
        let Ok(data) = tokio::fs::read_to_string(&path).await else {
            return false;
        };
        let Ok(sentinel) = serde_json::from_str::<CreateBsaSentinel>(&data) else {
            return false;
        };
        sentinel.pipeline_version == APPLY_STATE_VERSION
            && sentinel.directive_index == directive_index
            && sentinel.temp_id == temp_id
            && sentinel.to == to
            && StagingStore::new(&self.staging_dir)
                .logical_exists(to)
                .await
    }

    pub(super) async fn write_create_bsa_sentinel(
        &self,
        directive_index: usize,
        temp_id: &str,
        to: &str,
    ) -> Result<()> {
        let path = self.create_bsa_sentinel_path(directive_index);
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let sentinel = CreateBsaSentinel {
            pipeline_version: APPLY_STATE_VERSION,
            directive_index,
            temp_id: temp_id.to_string(),
            to: to.to_string(),
        };
        tokio::fs::write(&path, serde_json::to_vec_pretty(&sentinel)?).await?;
        Ok(())
    }

    pub(super) fn create_bsa_sentinel_path(&self, directive_index: usize) -> PathBuf {
        self.staging_dir
            .join("_state/create-bsa")
            .join(format!("{directive_index}.json"))
    }
}

/// Prefix of extraction dirs created by Modde builds before 2b82500. Those
/// builds `keep()`-leaked them (never cleaned); current code creates none.
const LEGACY_EXTRACT_PREFIX: &str = "modde-extract-";
/// Wabbajack installs take no instance lock, so ownership is decided by age: a
/// dir untouched this long cannot belong to a live install.
const STALE_EXTRACT_AGE: std::time::Duration = std::time::Duration::from_hours(168);

/// Remove stale `modde-extract-*` dirs under `tmp_base`; returns how many.
/// Best-effort: failures are logged, never fatal to the install.
pub(super) fn sweep_stale_extract_dirs(tmp_base: &Path, max_age: std::time::Duration) -> usize {
    let Ok(entries) = std::fs::read_dir(tmp_base) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = entry
            .file_name()
            .to_string_lossy()
            .starts_with(LEGACY_EXTRACT_PREFIX)
            && entry.file_type().is_ok_and(|t| t.is_dir())
            && entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|m| m.elapsed().is_ok_and(|age| age >= max_age));
        if stale {
            match std::fs::remove_dir_all(&path) {
                Ok(()) => removed += 1,
                Err(e) => warn!(
                    "failed to remove stale extraction dir {}: {e}",
                    path.display()
                ),
            }
        }
    }
    removed
}

impl WabbajackInstaller {
    /// Install-start sweep of leaked extraction dirs in `<modde data>/tmp`.
    pub(super) fn sweep_stale_extraction_temp(&self) {
        #[cfg(test)]
        let Some(base) = self.tmp_dir.clone() else {
            return;
        };
        #[cfg(not(test))]
        let base = modde_core::paths::modde_data_dir().join("tmp");
        let removed = sweep_stale_extract_dirs(&base, STALE_EXTRACT_AGE);
        if removed > 0 {
            info!(removed, "removed stale extraction temp dirs");
        }
    }
}
