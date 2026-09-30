#![allow(clippy::wildcard_imports)]
use super::*;

impl WabbajackInstaller {
    /// Extract a file from a downloaded archive and place it in the staging directory.
    pub(super) async fn apply_from_archive(
        &self,
        archive_hash: u64,
        from: &str,
        to: &str,
        expected_size: u64,
    ) -> Result<()> {
        // Validate paths against traversal attacks
        validate_archive_entry(from)?;
        validate_archive_entry(to)?;

        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        if let Some(source) = self.game_file_source_path(archive_hash)? {
            if game_file_source_is_whole_file(&source.path, &source.rel_path, from) {
                modde_core::link::link_or_copy(&source.path, &output_path).await?;
            } else {
                let archive_path = source.path;
                let from = from.to_string();
                tokio::task::spawn_blocking(move || {
                    ArchiveBatchExtractor::extract_selected(
                        &archive_path,
                        &[ArchiveRequest {
                            directive_index: 0,
                            from,
                            inner_path: None,
                            kind: ArchiveRequestKind::WriteFile {
                                to: output_path,
                                expected_size: (expected_size > 0).then_some(expected_size),
                            },
                        }],
                    )
                })
                .await??;
            }
            info!(from = %from, to = %to, "extracted game-file source");
            return Ok(());
        }

        // Extract the specific file from a downloaded archive or resolve a
        // Wabbajack game-file source from the local game install.
        let data = self
            .read_archive_source(archive_hash, from)
            .await
            .with_context(|| {
                format!("failed to extract '{from}' from archive {archive_hash:016x}")
            })?;

        tokio::fs::write(&output_path, &data).await?;

        info!(from = %from, to = %to, "extracted file from archive");
        Ok(())
    }

    /// Extract inline file data from the `.wabbajack` zip and write to staging.
    pub(super) async fn apply_inline_file(
        &self,
        inline_source: &InlineSource,
        source_data_id: &str,
        to: &str,
    ) -> Result<()> {
        let staging_store = StagingStore::new(&self.staging_dir);

        // Validate the output path against traversal attacks
        validate_archive_entry(to)?;

        let data = inline_source.read(source_data_id)?;
        let output_path = staging_store.write_path_for_logical(to, Some(data.len() as u64));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        write_bytes_maybe_zstd(&output_path, &data).await?;

        info!(source_data_id = %source_data_id, to = %to, "wrote inline file");
        Ok(())
    }

    /// Extract a file from archive, apply a binary delta patch, and write to staging.
    pub(super) async fn apply_patched_from_archive(
        &self,
        inline_source: &InlineSource,
        archive_hash: u64,
        from: &str,
        to: &str,
        patch_id: &str,
        expected_size: u64,
    ) -> Result<()> {
        // Extract source file from a downloaded archive or local game-file source.
        let source_data = self
            .read_archive_source_cached(archive_hash, from)
            .await
            .with_context(|| {
                format!("failed to extract '{from}' from archive {archive_hash:016x} for patching")
            })?;

        self.write_patched_output(inline_source, source_data, to, patch_id, expected_size)
            .await
    }

    pub(super) async fn write_patched_output(
        &self,
        inline_source: &InlineSource,
        source_data: Bytes,
        to: &str,
        patch_id: &str,
        expected_size: u64,
    ) -> Result<()> {
        // Validate the output path against traversal attacks
        validate_archive_entry(to)?;

        // Patched outputs can expand far beyond the source size. Keep the hot
        // patch path plain and let the compatibility compression sweep handle
        // eligible files later with bounded worker controls.
        let output_path = self.staging_dir.join(normalize_path(to));
        let part_path = output_path.with_extension(format!(
            "{}modde-part",
            output_path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(|extension| format!("{extension}."))
                .unwrap_or_default()
        ));
        let stale_compressed = super::super::staging::compressed_path(&output_path);

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::metadata(&stale_compressed).await.is_ok() {
            tokio::fs::remove_file(&stale_compressed).await?;
        }
        if tokio::fs::metadata(&part_path).await.is_ok() {
            tokio::fs::remove_file(&part_path).await?;
        }

        let patch_data = inline_source.read(patch_id)?;
        let _patch_guard = self.diagnostics.as_ref().map(|diagnostics| {
            diagnostics.start_patch(
                to.to_string(),
                patch_id.to_string(),
                source_data.len() as u64,
                expected_size,
            )
        });

        let to_owned = to.to_string();
        let output_path_for_task = output_path.clone();
        let part_path_for_task = part_path.clone();
        let patch_result = tokio::task::spawn_blocking(move || {
            let mut output = std::fs::File::create(&part_path_for_task).with_context(|| {
                format!(
                    "failed to create patched output: {}",
                    part_path_for_task.display()
                )
            })?;
            let expected = (expected_size > 0).then_some(expected_size);
            let output_bytes = patcher::apply_patch_to_writer_limited(
                &source_data,
                &patch_data,
                &mut output,
                expected,
            )
            .with_context(|| format!("failed to apply patch for: {to_owned}"))?;
            output
                .sync_all()
                .with_context(|| format!("failed to sync patched output: {to_owned}"))?;
            drop(output);
            std::fs::rename(&part_path_for_task, &output_path_for_task).with_context(|| {
                format!(
                    "failed to move patched output into place: {}",
                    output_path_for_task.display()
                )
            })?;
            Ok::<u64, anyhow::Error>(output_bytes)
        })
        .await?;
        let output_bytes = match patch_result {
            Ok(output_bytes) => output_bytes,
            Err(e) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                return Err(e);
            }
        };
        trim_process_allocator();

        info!(
            to = %to,
            patch_id = %patch_id,
            expected_size,
            output_bytes,
            "applied binary patch"
        );
        Ok(())
    }

    /// Create a BSA/BA2 archive from file states.
    #[cfg(feature = "bethesda-archives")]
    pub(super) async fn apply_create_bsa(
        &self,
        temp_id: &str,
        to: &str,
        file_states: &[modde_core::manifest::wabbajack::BSAFileState],
    ) -> Result<()> {
        // BSA source files are expected to be in a temp subdirectory of staging
        let bsa_staging = self.staging_dir.join(format!("bsa_temp_{temp_id}"));
        let output_path = self.staging_dir.join(normalize_path(to));

        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        bsa_repack::create_bsa(file_states, &bsa_staging, &output_path)
            .await
            .with_context(|| format!("failed to create BSA: {to}"))?;

        info!(to = %to, files = file_states.len(), "created BSA archive");
        Ok(())
    }

    #[cfg(not(feature = "bethesda-archives"))]
    pub(super) async fn apply_create_bsa(
        &self,
        _temp_id: &str,
        _to: &str,
        _file_states: &[modde_core::manifest::wabbajack::BSAFileState],
    ) -> Result<()> {
        anyhow::bail!("CreateBSA requires the bethesda-archives feature")
    }
}
