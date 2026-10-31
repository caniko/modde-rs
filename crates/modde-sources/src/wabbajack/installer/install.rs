#![allow(clippy::wildcard_imports)]
use super::admission::{APPLY_ADMISSION_TIMEOUT, acquire_apply_memory};
use super::*;

impl WabbajackInstaller {
    /// Run the full install pipeline, sending progress updates via channel.
    pub async fn install(&self, progress_tx: mpsc::UnboundedSender<InstallProgress>) -> Result<()> {
        #[cfg(not(feature = "bethesda-archives"))]
        anyhow::ensure!(
            !self
                .manifest
                .install_directives()
                .iter()
                .any(|directive| matches!(directive, InstallDirective::CreateBSA { .. })),
            "CreateBSA requires the bethesda-archives feature"
        );
        let staging_store = StagingStore::new(&self.staging_dir);
        staging_store.prepare_resumable().await?;
        let _diagnostics_heartbeat = self.diagnostics.as_ref().map(|diagnostics| {
            let cache = Arc::clone(&self.byte_cache);
            DiagnosticsHeartbeatGuard(Some(
                diagnostics.spawn_heartbeat(Arc::new(move || cache.bytes_used())),
            ))
        });
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("preflight");
            diagnostics.record_progress(ProgressEvent::Other);
        }

        let downloads = self.manifest.download_directives();
        let installs = self.manifest.install_directives();

        self.validate_game_file_sources()?;
        self.verify_game_file_sources().await?;
        self.validate_download_sources(&downloads)?;
        self.preflight_authored_files().await?;
        self.check_diagnostics_abort()?;

        progress_tx
            .send(InstallProgress::Starting {
                total_downloads: downloads.len(),
            })
            .ok();

        // Apply install directives.
        //
        // FromArchive / InlineFile / PatchedFromArchive directives each touch a
        // unique destination path inside the staging tree, so they can run in
        // parallel. CreateBSA is held back to a second pass because it depends
        // on the staging directory being fully populated for its `temp_id`
        // bucket.
        let total = installs.len();
        let progress_for_parallel = progress_tx.clone();
        // Apply concurrency.
        //
        // Bsdiff patches and large archive extractions can each peak at
        // hundreds of MB of resident memory; multiplied by a fixed worker
        // count this can swamp swap on machines that are otherwise healthy.
        //
        // We use a host-RAM-aware admission gate (`memory-admission` crate) so
        // workers self-throttle when the system runs hot, and a generous
        // structural cap on `buffer_unordered` so the gate has waiters to
        // release once memory frees up. `MODDE_APPLY_MAX_IN_FLIGHT` overrides
        // the cap for benchmarking; `MODDE_APPLY_RAM_FRACTION` tunes the
        // throttle threshold (default 0.80).
        let max_in_flight = std::env::var("MODDE_APPLY_MAX_IN_FLIGHT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map(std::num::NonZeroUsize::get)
                    .unwrap_or(8)
                    .clamp(1, 4)
            });
        // RAM fraction defaults are aggressive: the recommended deployment is
        // inside a cgroup (`systemd-run --user --scope -p MemoryHigh=…`) so
        // the kernel is the hard ceiling. The application-level gate just
        // prevents pathological in-process spikes.
        let max_ram_fraction = std::env::var("MODDE_APPLY_RAM_FRACTION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v| v > 0.0 && v <= 1.0)
            .unwrap_or(0.85);
        let safety_reserve_bytes = std::env::var("MODDE_APPLY_SAFETY_RESERVE_GIB")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .map_or(2 * (1_u64 << 30), |g| g * (1_u64 << 30));
        // Page-cache throttle is an extra defence against the I/O-induced
        // thrash failure mode (huge readahead folios + slow reclaim). Default
        // is permissive since fadvise(DONTNEED) on archives already prevents
        // most readahead build-up; tighten it only if the safety reserve
        // alone is not enough.
        // Default page-cache check is OFF (1.0) — when modde runs inside a
        // cgroup MemoryHigh, the kernel handles page-cache pressure without
        // application-level help, and the in-application throttle just
        // misfires when the cache is hot from prior activity. Operators
        // running outside a cgroup can opt back in by setting
        // `MODDE_APPLY_PAGE_CACHE_FRACTION=0.6` or similar.
        let page_cache_fraction = std::env::var("MODDE_APPLY_PAGE_CACHE_FRACTION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|&v: &f64| v > 0.0 && v <= 1.0)
            .unwrap_or(1.0);
        let weighted_config = memory_admission::weighted::WeightedConfig {
            base: memory_admission::Config {
                max_ram_fraction,
                ..memory_admission::Config::default()
            }
            .validate()
            .expect("hard-coded default memory-admission config is valid"),
            safety_reserve_bytes,
            // Anything beyond this size runs alone — typical Wabbajack lists
            // top out around 8-12 GiB single archives.
            max_single_weight_bytes: 4 * (1_u64 << 30),
            max_page_cache_fraction: page_cache_fraction,
            ..memory_admission::weighted::WeightedConfig::default()
        };
        // Prefer the cgroup-v2 provider when modde runs inside a memory-bounded
        // scope (e.g. `systemd-run --user --scope -p MemoryHigh=…`). The host
        // `MemAvailable` view is the wrong signal in that case — the cgroup
        // gets throttled long before host memory runs out, and a host-level
        // gate happily admits work that the kernel then forces into swap.
        let provider: memory_admission::provider::SharedMemoryProvider = {
            #[cfg(target_os = "linux")]
            {
                memory_admission::providers::CgroupV2Provider::shared()
                    .unwrap_or_else(|| memory_admission::providers::ProcMeminfoProvider::shared())
            }
            #[cfg(not(target_os = "linux"))]
            {
                memory_admission::providers::ProcMeminfoProvider::shared()
            }
        };
        #[cfg(test)]
        let provider = self.apply_memory_provider.clone().unwrap_or(provider);
        let gate = memory_admission::weighted::AsyncWeightedAdmissionGate::new(
            weighted_config.clone(),
            Arc::clone(&provider),
        );

        // Build a hash → size table from the manifest so per-directive weight
        // estimation is a constant-time lookup.
        let archive_size_by_hash: HashMap<u64, u64> = self
            .manifest
            .archives
            .iter()
            .map(|a| (a.hash, a.size))
            .collect();
        let archive_size_by_hash = Arc::new(archive_size_by_hash);

        info!(
            max_in_flight,
            max_ram_fraction,
            safety_reserve_gib = safety_reserve_bytes / (1 << 30),
            page_cache_fraction,
            byte_cache_used = self.byte_cache.bytes_used(),
            "starting weighted parallel apply pass"
        );
        let inline_source = installs
            .iter()
            .any(|directive| {
                matches!(
                    directive,
                    InstallDirective::InlineFile { .. }
                        | InstallDirective::PatchedFromArchive { .. }
                )
            })
            .then(|| InlineSource::open(&self.wabbajack_path).map(Arc::new))
            .transpose()?;

        // Phase 1 (per-archive batching): group FromArchive and
        // PatchedFromArchive directives by archive_hash so each archive is
        // touched at most once concurrently. Phase 2's native decoder can
        // then turn each batch into a single archive reader session. InlineFile
        // directives are independent of archives so they fan out separately.
        let impact = MissingArchiveImpact::analyze(&self.manifest, &self.store_dir);
        let skip_plan = impact.skip_plan(&self.manifest, self.missing_archive_policy);
        if !skip_plan.is_empty() {
            warn!(
                policy = ?self.missing_archive_policy,
                missing_archives = impact.missing_archives.len(),
                skipped_directives = skip_plan.skipped_directives.len(),
                skipped_mod_roots = skip_plan.skipped_mod_roots.len(),
                "omitting Wabbajack outputs for missing optional archives"
            );
        }

        let mut inline_directives: Vec<(usize, &InstallDirective)> = Vec::new();
        for (i, directive) in installs.iter().enumerate() {
            if matches!(directive, InstallDirective::InlineFile { .. })
                && !skip_plan.should_skip_directive(i, directive)
            {
                inline_directives.push((i, directive));
            }
        }
        let mut archive_batches = self.manifest.install_directives_grouped_by_archive();
        if !skip_plan.is_empty() {
            for batch in &mut archive_batches {
                batch.directives.retain(|indexed| {
                    !skip_plan.should_skip_directive(indexed.directive_index, &indexed.directive)
                });
            }
            archive_batches.retain(|batch| !batch.directives.is_empty());
        }
        let archive_batch_count = archive_batches.len();
        let patch_max_in_flight = std::env::var("MODDE_PATCH_MAX_IN_FLIGHT")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|&n| n > 0)
            .unwrap_or(1);
        let patch_batch_gate = Arc::new(Semaphore::new(patch_max_in_flight));
        // Read apply-weight tunables once; env does not change mid-run, so we
        // avoid re-parsing it per directive/per batch in the fan-out below.
        let directive_weights = DirectiveWeights::from_env();
        let archive_batch_weights = ArchiveBatchWeights::from_env();
        info!(
            archive_batch_count,
            inline_directive_count = inline_directives.len(),
            patch_max_in_flight,
            "grouped apply directives by archive"
        );
        let adoption = self
            .adopt_existing_staging(&archive_batches, &installs)
            .await?;
        self.check_diagnostics_abort()?;
        if adoption.archive_batches > 0 || adoption.create_bsa > 0 {
            info!(
                archive_batches = adoption.archive_batches,
                create_bsa = adoption.create_bsa,
                "adopted existing Wabbajack staging outputs"
            );
            progress_tx
                .send(InstallProgress::StagingAdopted {
                    archive_batches: adoption.archive_batches,
                    create_bsa: adoption.create_bsa,
                })
                .ok();
        }

        // Inline-file pass — these have no archive contention so we can
        // saturate the gate's RAM budget without backpressuring on archives.
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("apply-inline");
        }
        let inline_results: Vec<(usize, Result<()>)> = stream::iter(inline_directives)
            .map(|(i, directive)| {
                let progress_tx = progress_for_parallel.clone();
                let gate = gate.clone();
                let sizes = Arc::clone(&archive_size_by_hash);
                let inline_source = inline_source.clone();
                let provider = Arc::clone(&provider);
                let weighted_config = &weighted_config;
                async move {
                    let weight = estimate_directive_weight(directive, &sizes, &directive_weights);
                    let _permit = match acquire_apply_memory(
                        &gate,
                        &provider,
                        weighted_config,
                        weight,
                        APPLY_ADMISSION_TIMEOUT,
                        || self.check_diagnostics_abort(),
                    )
                    .await
                    {
                        Ok(permit) => permit,
                        Err(error) => return (i, Err(error)),
                    };
                    if let Err(e) = self.check_diagnostics_abort() {
                        return (i, Err(e));
                    }
                    progress_tx
                        .send(InstallProgress::Applying {
                            directive_index: i,
                            total,
                        })
                        .ok();
                    let InstallDirective::InlineFile { source_data_id, to } = directive else {
                        unreachable!("inline_directives only contains InlineFile");
                    };
                    progress_tx
                        .send(InstallProgress::InlineFile { name: to.clone() })
                        .ok();
                    let result = match inline_source.as_deref() {
                        Some(inline_source) => {
                            self.apply_inline_file(inline_source, source_data_id, to)
                                .await
                        }
                        None => Err(anyhow::anyhow!("inline source was not initialized")),
                    };
                    (i, result)
                }
            })
            .buffer_unordered(max_in_flight)
            .collect()
            .await;

        // Archive-batch pass — one task per archive_hash, each task processes
        // its own directives sequentially. Concurrency is over batches, so
        // the same archive is never decompressed twice in parallel.
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("apply-archive-batches");
        }
        let archive_results: Vec<Vec<(usize, Result<()>)>> = stream::iter(archive_batches)
            .map(|batch| {
                let progress_tx = progress_for_parallel.clone();
                let gate = gate.clone();
                let inline_source = inline_source.clone();
                let patch_batch_gate = Arc::clone(&patch_batch_gate);
                let provider = Arc::clone(&provider);
                let weighted_config = &weighted_config;
                async move {
                    let _patch_permit = if archive_batch_has_patch(&batch) {
                        Some(
                            patch_batch_gate
                                .acquire_owned()
                                .await
                                .expect("semaphore open"),
                        )
                    } else {
                        None
                    };
                    // The batch's whole-archive memory cost is the archive
                    // size itself (decompression working set, conservatively
                    // half), regardless of how many directives it serves.
                    let archive_weight =
                        estimate_archive_batch_weight(&batch, &archive_batch_weights);
                    let _permit = match acquire_apply_memory(
                        &gate,
                        &provider,
                        weighted_config,
                        archive_weight,
                        APPLY_ADMISSION_TIMEOUT,
                        || self.check_diagnostics_abort(),
                    )
                    .await
                    {
                        Ok(permit) => permit,
                        Err(error) => {
                            return batch
                                .directives
                                .into_iter()
                                .map(|directive| {
                                    (directive.directive_index, Err(anyhow::anyhow!("{error:#}")))
                                })
                                .collect();
                        }
                    };
                    if let Err(e) = self.check_diagnostics_abort() {
                        return batch
                            .directives
                            .into_iter()
                            .map(|directive| {
                                (directive.directive_index, Err(anyhow::anyhow!("{e:#}")))
                            })
                            .collect();
                    }

                    for indexed in &batch.directives {
                        let i = indexed.directive_index;
                        progress_tx
                            .send(InstallProgress::Applying {
                                directive_index: i,
                                total,
                            })
                            .ok();
                    }
                    self.apply_archive_batch(batch, inline_source.as_deref(), &progress_tx)
                        .await
                }
            })
            .buffer_unordered(max_in_flight)
            .collect()
            .await;

        for (i, result) in inline_results
            .into_iter()
            .chain(archive_results.into_iter().flatten())
        {
            if let Err(e) = result {
                if self.continue_on_error {
                    warn!(directive_index = i, "skipping directive after error: {e:#}");
                } else {
                    return Err(e);
                }
            }
        }

        // Second pass: CreateBSA (depends on staged temp directories).
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("create-bsa");
        }
        for (i, directive) in installs.iter().enumerate() {
            self.check_diagnostics_abort()?;
            let InstallDirective::CreateBSA {
                temp_id,
                to,
                file_states,
            } = directive
            else {
                continue;
            };
            if skip_plan.should_skip_create_bsa(i, temp_id, to) {
                warn!(
                    directive_index = i,
                    temp_id,
                    to,
                    "skipping CreateBSA because an optional upstream archive is missing"
                );
                continue;
            }
            if self.create_bsa_sentinel_valid(i, temp_id, to).await {
                continue;
            }
            progress_tx
                .send(InstallProgress::Applying {
                    directive_index: i,
                    total,
                })
                .ok();
            progress_tx
                .send(InstallProgress::CreatingBSA { name: to.clone() })
                .ok();
            if let Err(e) = self.apply_create_bsa(temp_id, to, file_states).await {
                if self.continue_on_error {
                    warn!(directive_index = i, "skipping CreateBSA after error: {e:#}");
                } else {
                    return Err(e);
                }
            } else {
                self.write_create_bsa_sentinel(i, temp_id, to).await?;
                if let Some(diagnostics) = &self.diagnostics {
                    diagnostics.record_progress(ProgressEvent::CreateBsaComplete);
                }
            }
        }

        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("compress-staging");
        }
        self.check_diagnostics_abort()?;
        let summary = staging_store
            .compress_eligible_files(self.concurrency)
            .await?;
        info!(
            compressed_files = summary.compressed_files,
            skipped_files = summary.skipped_files,
            original_bytes = summary.original_bytes,
            compressed_bytes = summary.compressed_bytes,
            "compressed Wabbajack staging files"
        );

        progress_tx.send(InstallProgress::Complete).ok();
        if let Some(diagnostics) = &self.diagnostics {
            diagnostics.set_phase("complete");
            diagnostics.record_progress(ProgressEvent::Other);
        }
        info!("wabbajack installation complete");
        Ok(())
    }
}
