//! Patcher stage configuration and generated-output rows.
#![allow(clippy::wildcard_imports)]

use super::rows::*;
use super::*;

impl ModdeDb {
    pub async fn save_patcher_stage(&self, stage: &PatcherStageRow) -> Result<()> {
        if stage.name.trim().is_empty() {
            return Err(CoreError::Validation(
                "patcher stage name cannot be empty".into(),
            ));
        }
        if stage.output_mod.trim().is_empty() {
            return Err(CoreError::Validation(
                "patcher output mod cannot be empty".into(),
            ));
        }
        if matches!(stage.settings, PatcherStageSettings::RustNative) {
            return Err(CoreError::Validation(
                "rust-native patcher stages are reserved for a future release".into(),
            ));
        }
        if self
            .db
            .fetch_optional(
                "SELECT name FROM profile_patcher_stages
                 WHERE profile_id = ? AND output_mod = ? AND name <> ?",
                &vals![
                    stage.profile_id,
                    stage.output_mod.clone(),
                    stage.name.clone()
                ],
                |r| r.string(0),
            )
            .await?
            .is_some()
        {
            return Err(CoreError::Validation(
                format!(
                    "patcher output mod '{}' is already used by another stage in this profile",
                    stage.output_mod
                )
                .into(),
            ));
        }
        let settings_json = serde_json::to_string(&stage.settings).map_err(|e| {
            CoreError::Other(format!("failed to serialize patcher settings: {e}").into())
        })?;
        self.db
            .execute(
                "INSERT INTO profile_patcher_stages (
                    profile_id, name, stage_kind, enabled, sort_index,
                    settings_json, output_mod, timeout_seconds, updated_at
                 )
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, {NOW})
                 ON CONFLICT(profile_id, name) DO UPDATE SET
                    stage_kind = excluded.stage_kind,
                    enabled = excluded.enabled,
                    sort_index = excluded.sort_index,
                    settings_json = excluded.settings_json,
                    output_mod = excluded.output_mod,
                    timeout_seconds = excluded.timeout_seconds,
                    updated_at = excluded.updated_at",
                &vals![
                    stage.profile_id,
                    stage.name.clone(),
                    stage.stage_kind.as_str(),
                    stage.enabled,
                    stage.sort_index,
                    settings_json,
                    stage.output_mod.clone(),
                    stage.timeout_seconds as i64,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn mark_patcher_stage_cache_success(
        &self,
        profile_id: i64,
        stage_name: &str,
        cache_key: &str,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_patcher_stages
                 SET last_cache_key = ?, last_success_at = {NOW}, updated_at = {NOW}
                 WHERE profile_id = ? AND name = ?",
                &vals![cache_key, profile_id, stage_name],
            )
            .await?;
        Ok(())
    }

    /// List patcher stages for a profile in execution order.
    pub async fn list_patcher_stages(&self, profile_id: i64) -> Result<Vec<PatcherStageRow>> {
        self.db
            .fetch_all(
                "SELECT profile_id, name, stage_kind, enabled, sort_index, settings_json, output_mod,
                        last_cache_key, last_success_at, timeout_seconds
                 FROM profile_patcher_stages
                 WHERE profile_id = ?
                 ORDER BY sort_index, lower(name)",
                &vals![profile_id],
                patcher_stage_from_row,
            )
            .await
    }

    /// Load one patcher stage by name.
    pub async fn load_patcher_stage(
        &self,
        profile_id: i64,
        name: &str,
    ) -> Result<Option<PatcherStageRow>> {
        self.db
            .fetch_optional(
                "SELECT profile_id, name, stage_kind, enabled, sort_index, settings_json, output_mod,
                        last_cache_key, last_success_at, timeout_seconds
                 FROM profile_patcher_stages
                 WHERE profile_id = ? AND name = ?",
                &vals![profile_id, name],
                patcher_stage_from_row,
            )
            .await
    }

    /// Delete a patcher stage. Returns whether a row was removed.
    pub async fn delete_patcher_stage(&self, profile_id: i64, name: &str) -> Result<bool> {
        let affected = self
            .db
            .execute(
                "DELETE FROM profile_patcher_stages WHERE profile_id = ? AND name = ?",
                &vals![profile_id, name],
            )
            .await?;
        Ok(affected > 0)
    }

    /// Toggle a patcher stage's enabled state.
    pub async fn set_patcher_stage_enabled(
        &self,
        profile_id: i64,
        name: &str,
        enabled: bool,
    ) -> Result<bool> {
        let affected = self
            .db
            .execute(
                "UPDATE profile_patcher_stages SET enabled = ?, updated_at = {NOW}
                 WHERE profile_id = ? AND name = ?",
                &vals![enabled, profile_id, name],
            )
            .await?;
        Ok(affected > 0)
    }

    /// Replace stage ordering for the supplied names.
    pub async fn reorder_patcher_stages(&self, profile_id: i64, names: &[String]) -> Result<()> {
        // A reorder replaces the whole order: a missing, unknown or repeated
        // name would otherwise be skipped silently or leave tied positions.
        let mut expected: Vec<String> = self
            .list_patcher_stages(profile_id)
            .await?
            .into_iter()
            .map(|stage| stage.name)
            .collect();
        let mut supplied = names.to_vec();
        expected.sort();
        supplied.sort();
        if supplied != expected {
            return Err(CoreError::Validation(
                format!(
                    "reorder must list every patcher stage exactly once: {}",
                    expected.join(", ")
                )
                .into(),
            ));
        }

        let mut tx = self.db.begin().await?;
        for (idx, name) in names.iter().enumerate() {
            tx.execute(
                "UPDATE profile_patcher_stages SET sort_index = ?, updated_at = {NOW}
                 WHERE profile_id = ? AND name = ?",
                &vals![idx as i64, profile_id, name.clone()],
            )
            .await?;
        }
        tx.commit().await
    }

    /// Load every managed output path recorded for one patcher stage.
    pub async fn list_patcher_stage_outputs(
        &self,
        profile_id: i64,
        stage_name: &str,
    ) -> Result<Vec<PatcherStageOutputRow>> {
        self.db
            .fetch_all(
                "SELECT profile_id, stage_name, rel_path
                 FROM profile_patcher_stage_outputs
                 WHERE profile_id = ? AND stage_name = ?
                 ORDER BY rel_path",
                &vals![profile_id, stage_name],
                patcher_stage_output_from_row,
            )
            .await
    }

    /// Replace the recorded managed output manifest for one stage.
    pub async fn replace_patcher_stage_outputs(
        &self,
        profile_id: i64,
        stage_name: &str,
        rel_paths: &[String],
    ) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM profile_patcher_stage_outputs
                 WHERE profile_id = ? AND stage_name = ?",
                &vals![profile_id, stage_name],
            )
            .await?;
        for rel_path in rel_paths {
            self.db
                .execute(
                    "INSERT INTO profile_patcher_stage_outputs (
                        profile_id, stage_name, rel_path, updated_at
                     ) VALUES (?, ?, ?, {NOW})",
                    &vals![profile_id, stage_name, rel_path.clone()],
                )
                .await?;
        }
        Ok(())
    }

    /// Load every managed output path for all stages on this profile.
    pub async fn list_all_patcher_stage_outputs(
        &self,
        profile_id: i64,
    ) -> Result<Vec<PatcherStageOutputRow>> {
        self.db
            .fetch_all(
                "SELECT profile_id, stage_name, rel_path
                 FROM profile_patcher_stage_outputs
                 WHERE profile_id = ?
                 ORDER BY stage_name, rel_path",
                &vals![profile_id],
                patcher_stage_output_from_row,
            )
            .await
    }
}
