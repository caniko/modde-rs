//! Executable launch targets, performance runs, and UI test cleanup.
#![allow(clippy::wildcard_imports)]

use super::rows::*;
use super::*;

impl ModdeDb {
    pub async fn save_executable_config(&self, executable: &ExecutableConfigRow) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO executable_configs (
                    game_id, name, executable_path, arguments, working_dir,
                    environment, wine_dll_overrides, output_mod, enabled, updated_at
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, {NOW})
                 ON CONFLICT(game_id, name) DO UPDATE SET
                    executable_path = excluded.executable_path,
                    arguments = excluded.arguments,
                    working_dir = excluded.working_dir,
                    environment = excluded.environment,
                    wine_dll_overrides = excluded.wine_dll_overrides,
                    output_mod = excluded.output_mod,
                    enabled = excluded.enabled,
                    updated_at = excluded.updated_at",
                &vals![
                    executable.game_id.clone(),
                    executable.name.clone(),
                    executable.executable_path.to_string_lossy().to_string(),
                    executable.arguments_json.clone(),
                    executable
                        .working_dir
                        .as_ref()
                        .map(|p| p.to_string_lossy().to_string()),
                    executable.environment_json.clone(),
                    executable.wine_dll_overrides.clone(),
                    executable.output_mod.clone(),
                    executable.enabled,
                ],
            )
            .await?;
        Ok(())
    }

    /// Load every executable configured for a game, ordered by display name.
    pub async fn load_executable_configs(
        &self,
        game_id: &GameId,
    ) -> Result<Vec<ExecutableConfigRow>> {
        self.db
            .fetch_all(
                "SELECT game_id, name, executable_path, arguments, working_dir,
                        environment, wine_dll_overrides, output_mod, enabled
                 FROM executable_configs
                 WHERE game_id = ?
                 ORDER BY lower(name)",
                &vals![game_id],
                executable_from_row,
            )
            .await
    }

    /// Load a single named executable for a game.
    pub async fn load_executable_config(
        &self,
        game_id: &GameId,
        name: &str,
    ) -> Result<Option<ExecutableConfigRow>> {
        self.db
            .fetch_optional(
                "SELECT game_id, name, executable_path, arguments, working_dir,
                        environment, wine_dll_overrides, output_mod, enabled
                 FROM executable_configs
                 WHERE game_id = ? AND name = ?",
                &vals![game_id, name],
                executable_from_row,
            )
            .await
    }

    /// Delete a named executable for a game. Returns whether a row was removed.
    pub async fn delete_executable_config(&self, game_id: &GameId, name: &str) -> Result<bool> {
        let affected = self
            .db
            .execute(
                "DELETE FROM executable_configs WHERE game_id = ? AND name = ?",
                &vals![game_id, name],
            )
            .await?;
        Ok(affected > 0)
    }

    // ── Performance telemetry CRUD ───────────────────────────────

    /// Create a pending performance run before launching the game.
    pub async fn create_performance_run(&self, run: &NewPerformanceRun) -> Result<()> {
        let mod_snapshot_json = serde_json::to_string(&run.mod_snapshot)?;
        let hash = mod_set_hash(&run.mod_snapshot);
        self.db
            .execute(
                "INSERT INTO performance_runs (
                    run_id, game_id, profile_id, profile_name, mod_set_hash,
                    mod_snapshot, experiment_depth, label, status
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending')",
                &vals![
                    run.run_id.clone(),
                    &run.game_id,
                    run.profile_id,
                    run.profile_name.clone(),
                    hash,
                    mod_snapshot_json,
                    run.experiment_depth as i64,
                    run.label.clone(),
                ],
            )
            .await?;
        Ok(())
    }

    /// Mark a performance run complete and replace its parsed samples.
    pub async fn complete_performance_run(
        &self,
        run_id: &str,
        csv_path: &Path,
        exit_status: Option<i64>,
        summary: &PerformanceSummary,
        samples: &[PerformanceSample],
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;
        tx.execute(
            "UPDATE performance_runs SET
                status = 'complete',
                finished_at = {NOW},
                mangohud_csv_path = ?,
                exit_status = ?,
                sample_count = ?,
                duration_seconds = ?,
                median_fps = ?,
                average_fps = ?,
                one_percent_low_fps = ?,
                point_one_percent_low_fps = ?,
                median_frame_time_ms = ?,
                p95_frame_time_ms = ?,
                p99_frame_time_ms = ?
             WHERE run_id = ?",
            &vals![
                csv_path.to_string_lossy().to_string(),
                exit_status,
                summary.sample_count as i64,
                summary.duration_seconds,
                summary.median_fps,
                summary.average_fps,
                summary.one_percent_low_fps,
                summary.point_one_percent_low_fps,
                summary.median_frame_time_ms,
                summary.p95_frame_time_ms,
                summary.p99_frame_time_ms,
                run_id,
            ],
        )
        .await?;
        tx.execute(
            "DELETE FROM performance_samples WHERE run_id = ?",
            &vals![run_id],
        )
        .await?;
        for sample in samples {
            tx.execute(
                "INSERT INTO performance_samples (
                    run_id, elapsed_seconds, fps, frame_time_ms, cpu_load, gpu_load
                 )
                 VALUES (?, ?, ?, ?, ?, ?)",
                &vals![
                    run_id,
                    sample.elapsed_seconds,
                    sample.fps,
                    sample.frame_time_ms,
                    sample.cpu_load,
                    sample.gpu_load,
                ],
            )
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Attach a CSV path to a run that could not be observed to completion.
    pub async fn mark_performance_run_pending(
        &self,
        run_id: &str,
        csv_path: Option<&Path>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE performance_runs SET status = 'pending', mangohud_csv_path = ?
                 WHERE run_id = ?",
                &vals![csv_path.map(|p| p.to_string_lossy().to_string()), run_id,],
            )
            .await?;
        Ok(())
    }

    /// Load one performance run.
    pub async fn load_performance_run(&self, run_id: &str) -> Result<PerformanceRunRow> {
        self.db
            .fetch_optional(
                "SELECT run_id, game_id, profile_id, profile_name, mod_set_hash,
                        mod_snapshot, experiment_depth, label, status, started_at,
                        finished_at, mangohud_csv_path, exit_status, sample_count,
                        duration_seconds, median_fps, average_fps,
                        one_percent_low_fps, point_one_percent_low_fps,
                        median_frame_time_ms, p95_frame_time_ms, p99_frame_time_ms
                 FROM performance_runs
                 WHERE run_id = ?",
                &vals![run_id],
                performance_run_from_row,
            )
            .await?
            .ok_or_else(|| CoreError::Other(format!("performance run not found: {run_id}").into()))
    }

    /// List parsed `MangoHud` samples for a performance run in capture order.
    pub async fn list_performance_samples(&self, run_id: &str) -> Result<Vec<PerformanceSample>> {
        self.db
            .fetch_all(
                "SELECT elapsed_seconds, fps, frame_time_ms, cpu_load, gpu_load
                 FROM performance_samples
                 WHERE run_id = ?
                 ORDER BY id",
                &vals![run_id],
                performance_sample_from_row,
            )
            .await
    }

    /// List performance runs for a game, newest first.
    pub async fn list_performance_runs(
        &self,
        game_id: &GameId,
        profile_name: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PerformanceRunRow>> {
        match profile_name {
            Some(profile_name) => {
                self.db
                    .fetch_all(
                        "SELECT run_id, game_id, profile_id, profile_name, mod_set_hash,
                                mod_snapshot, experiment_depth, label, status, started_at,
                                finished_at, mangohud_csv_path, exit_status, sample_count,
                                duration_seconds, median_fps, average_fps,
                                one_percent_low_fps, point_one_percent_low_fps,
                                median_frame_time_ms, p95_frame_time_ms, p99_frame_time_ms
                         FROM performance_runs
                         WHERE game_id = ? AND profile_name = ?
                         ORDER BY started_at DESC
                         LIMIT ?",
                        &vals![game_id, profile_name, limit as i64],
                        performance_run_from_row,
                    )
                    .await
            }
            None => {
                self.db
                    .fetch_all(
                        "SELECT run_id, game_id, profile_id, profile_name, mod_set_hash,
                                mod_snapshot, experiment_depth, label, status, started_at,
                                finished_at, mangohud_csv_path, exit_status, sample_count,
                                duration_seconds, median_fps, average_fps,
                                one_percent_low_fps, point_one_percent_low_fps,
                                median_frame_time_ms, p95_frame_time_ms, p99_frame_time_ms
                         FROM performance_runs
                         WHERE game_id = ?
                         ORDER BY started_at DESC
                         LIMIT ?",
                        &vals![game_id, limit as i64],
                        performance_run_from_row,
                    )
                    .await
            }
        }
    }

    /// Clear cross-crate UI test state stored outside profiles.
    ///
    /// The UI integration tests share one isolated on-disk database because
    /// the data directory override is process-global. This keeps test cleanup
    /// in the database layer, where table ownership and ordering are explicit.
    #[doc(hidden)]
    pub async fn clear_ui_test_state(&self) -> Result<()> {
        #[cfg(feature = "postgres")]
        if matches!(&self.db, Db::Postgres(_)) {
            return Err(CoreError::Other(
                "UI test cleanup requires an isolated SQLite database".into(),
            ));
        }
        for table in [
            "tool_setting_edges",
            "tool_setting_nodes",
            "tool_applied_files",
            "profile_patcher_stage_outputs",
            "profile_patcher_stages",
            "game_tools",
            "executable_configs",
            "performance_samples",
            "performance_runs",
            "bisect_steps",
            "bisect_sessions",
        ] {
            self.db
                .execute(&format!("DELETE FROM {table}"), &vals![])
                .await?;
        }
        Ok(())
    }
}
