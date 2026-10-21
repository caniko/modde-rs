//! Mod metadata, hidden files, plugin order, categories, installs, and crash logs.
#![allow(clippy::wildcard_imports)]

use super::rows::*;
use super::*;

impl ModdeDb {
    pub async fn hide_file(&self, profile_id: i64, mod_id: &ModId, rel_path: &str) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO hidden_files (profile_id, mod_id, rel_path)
                 VALUES (?, ?, ?)
                 ON CONFLICT(profile_id, mod_id, rel_path) DO NOTHING",
                &vals![profile_id, mod_id, rel_path],
            )
            .await?;
        Ok(())
    }

    /// Unhide a previously hidden file.
    pub async fn unhide_file(&self, profile_id: i64, mod_id: &ModId, rel_path: &str) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM hidden_files WHERE profile_id = ? AND mod_id = ? AND rel_path = ?",
                &vals![profile_id, mod_id, rel_path],
            )
            .await?;
        Ok(())
    }

    /// List all hidden files for a profile.
    pub async fn list_hidden_files(&self, profile_id: i64) -> Result<Vec<HiddenFile>> {
        self.db
            .fetch_all(
                "SELECT mod_id, rel_path FROM hidden_files WHERE profile_id = ?",
                &vals![profile_id],
                |r| {
                    Ok(HiddenFile {
                        mod_id: r.string(0)?,
                        rel_path: r.string(1)?,
                    })
                },
            )
            .await
    }

    /// List hidden files for a specific mod in a profile.
    pub async fn list_hidden_files_for_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT rel_path FROM hidden_files WHERE profile_id = ? AND mod_id = ?",
                &vals![profile_id, mod_id],
                |r| r.string(0),
            )
            .await
    }

    // ── Plugin Order ─────────────────────────────────────────────

    /// Set the plugin order for a profile (replaces any existing order).
    pub async fn set_plugin_order(&self, profile_id: i64, plugins: &[PluginEntry]) -> Result<()> {
        let mut tx = self.db.begin().await?;
        tx.execute(
            "DELETE FROM plugin_order WHERE profile_id = ?",
            &vals![profile_id],
        )
        .await?;
        for plugin in plugins {
            tx.execute(
                "INSERT INTO plugin_order (profile_id, plugin_name, sort_index, enabled)
                 VALUES (?, ?, ?, ?)",
                &vals![
                    profile_id,
                    plugin.plugin_name.clone(),
                    plugin.sort_index,
                    plugin.enabled,
                ],
            )
            .await?;
        }
        tx.commit().await
    }

    /// Get the plugin order for a profile.
    pub async fn get_plugin_order(&self, profile_id: i64) -> Result<Vec<PluginEntry>> {
        self.db
            .fetch_all(
                "SELECT plugin_name, sort_index, enabled FROM plugin_order
                 WHERE profile_id = ? ORDER BY sort_index",
                &vals![profile_id],
                |r| {
                    Ok(PluginEntry {
                        plugin_name: r.string(0)?,
                        sort_index: r.i64(1)?,
                        enabled: r.bool(2)?,
                    })
                },
            )
            .await
    }

    /// Copy profile-adjacent state that is not represented inside [`Profile`].
    ///
    /// Used by bisect candidate profiles so hidden-file exclusions, native
    /// plugin order, and installer file manifests remain consistent with the
    /// source profile.
    pub async fn copy_profile_auxiliary_state(
        &self,
        source_profile_id: i64,
        target_profile_id: i64,
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;
        tx.execute(
            "INSERT INTO hidden_files (profile_id, mod_id, rel_path)
             SELECT ?, mod_id, rel_path FROM hidden_files WHERE profile_id = ?",
            &vals![target_profile_id, source_profile_id],
        )
        .await?;
        tx.execute(
            "INSERT INTO plugin_order (profile_id, plugin_name, sort_index, enabled)
             SELECT ?, plugin_name, sort_index, enabled FROM plugin_order WHERE profile_id = ?",
            &vals![target_profile_id, source_profile_id],
        )
        .await?;
        tx.execute(
            "INSERT INTO installed_mod_files
                (profile_id, mod_id, rel_path, origin_rel_path, size, merge_group)
             SELECT ?, mod_id, rel_path, origin_rel_path, size, merge_group
               FROM installed_mod_files WHERE profile_id = ?",
            &vals![target_profile_id, source_profile_id],
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Toggle a plugin's enabled state.
    pub async fn toggle_plugin(
        &self,
        profile_id: i64,
        plugin_name: &str,
        enabled: bool,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE plugin_order SET enabled = ? WHERE profile_id = ? AND plugin_name = ?",
                &vals![enabled, profile_id, plugin_name],
            )
            .await?;
        Ok(())
    }

    // ── Mod Categories ───────────────────────────────────────────

    /// Create a mod category, returning its ID.
    pub async fn create_category(&self, profile_id: i64, category: &ModCategory) -> Result<i64> {
        self.db
            .fetch_one(
                "INSERT INTO mod_categories (profile_id, name, color, sort_index)
                 VALUES (?, ?, ?, ?) RETURNING id",
                &vals![
                    profile_id,
                    category.name.clone(),
                    category.color.clone(),
                    category.sort_index,
                ],
                |r| r.i64(0),
            )
            .await
    }

    /// Update a category.
    pub async fn update_category(
        &self,
        category_id: i64,
        name: &str,
        color: Option<&str>,
        sort_index: i64,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE mod_categories SET name = ?, color = ?, sort_index = ? WHERE id = ?",
                &vals![name, color.map(str::to_string), sort_index, category_id],
            )
            .await?;
        Ok(())
    }

    /// Delete a category (nullifies `category_id` on affected mods).
    pub async fn delete_category(&self, category_id: i64) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET category_id = NULL WHERE category_id = ?",
                &vals![category_id],
            )
            .await?;
        self.db
            .execute(
                "DELETE FROM mod_categories WHERE id = ?",
                &vals![category_id],
            )
            .await?;
        Ok(())
    }

    /// List categories for a profile.
    pub async fn list_categories(&self, profile_id: i64) -> Result<Vec<ModCategory>> {
        self.db
            .fetch_all(
                "SELECT id, name, color, sort_index FROM mod_categories
                 WHERE profile_id = ? ORDER BY sort_index",
                &vals![profile_id],
                |r| {
                    Ok(ModCategory {
                        id: Some(r.i64(0)?),
                        name: r.string(1)?,
                        color: r.opt_string(2)?,
                        sort_index: r.i64(3)?,
                    })
                },
            )
            .await
    }

    /// Assign a mod to a category.
    pub async fn set_mod_category(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        category_id: Option<i64>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET category_id = ? WHERE profile_id = ? AND mod_id = ?",
                &vals![category_id, profile_id, mod_id],
            )
            .await?;
        Ok(())
    }

    /// Set notes for a mod.
    pub async fn set_mod_notes(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        notes: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET notes = ? WHERE profile_id = ? AND mod_id = ?",
                &vals![notes.map(str::to_string), profile_id, mod_id],
            )
            .await?;
        Ok(())
    }

    /// Set tags for a mod (stored as a JSON array in the TEXT column).
    pub async fn set_mod_tags(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        tags: &[String],
    ) -> Result<()> {
        let encoded_tags = encode_tags(tags)?;
        self.db
            .execute(
                "UPDATE profile_mods SET tags = ? WHERE profile_id = ? AND mod_id = ?",
                &vals![encoded_tags, profile_id, mod_id],
            )
            .await?;
        Ok(())
    }

    /// Set Nexus metadata for a mod.
    pub async fn set_mod_nexus_meta(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        nexus_mod_id: NexusModId,
        nexus_file_id: NexusFileId,
        nexus_game_domain: &str,
        installed_timestamp: i64,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE profile_mods SET nexus_mod_id = ?, nexus_file_id = ?,
                        nexus_game_domain = ?, installed_timestamp = ?
                 WHERE profile_id = ? AND mod_id = ?",
                &vals![
                    nexus_mod_id.to_i64()?,
                    nexus_file_id.to_i64()?,
                    nexus_game_domain,
                    installed_timestamp,
                    profile_id,
                    mod_id,
                ],
            )
            .await?;
        Ok(())
    }

    // ── Installer tracking (V8) ───────────────────────────────────

    /// Persist an installer's decision and file manifest for a single mod row,
    /// atomically (in one transaction).
    pub async fn record_install(
        &self,
        profile_id: i64,
        mod_id: &ModId,
        plan: &InstallPlan,
        status: InstallStatus,
    ) -> Result<()> {
        let method_toml = encode_install_method(&plan.method)?;
        let mut tx = self.db.begin().await?;

        tx.execute(
            "UPDATE profile_mods
                SET install_method = ?, source_archive_hash = ?, install_status = ?
              WHERE profile_id = ? AND mod_id = ?",
            &vals![
                method_toml,
                plan.source_archive_hash.clone(),
                status.as_str(),
                profile_id,
                mod_id,
            ],
        )
        .await?;

        tx.execute(
            "DELETE FROM installed_mod_files WHERE profile_id = ? AND mod_id = ?",
            &vals![profile_id, mod_id],
        )
        .await?;

        for file in &plan.staged_files {
            tx.execute(
                "INSERT INTO installed_mod_files
                    (profile_id, mod_id, rel_path, origin_rel_path, size, merge_group)
                 VALUES (?, ?, ?, ?, ?, ?)",
                &vals![
                    profile_id,
                    mod_id,
                    file.rel_path.clone(),
                    file.origin_rel_path.clone(),
                    file.size as i64,
                    file.merge_group.clone(),
                ],
            )
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Return every file staged by `mod_id` in `profile_id`, sorted by relative
    /// path for deterministic uninstall order.
    pub async fn installed_files_for_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<StagedFile>> {
        self.db
            .fetch_all(
                "SELECT rel_path, origin_rel_path, size, merge_group
                   FROM installed_mod_files
                  WHERE profile_id = ? AND mod_id = ?
               ORDER BY rel_path",
                &vals![profile_id, mod_id],
                |r| {
                    Ok(StagedFile {
                        rel_path: r.string(0)?,
                        origin_rel_path: r.string(1)?,
                        size: r.i64(2)?.max(0) as u64,
                        merge_group: r.opt_string(3)?,
                    })
                },
            )
            .await
    }

    /// Remove a mod from `profile_mods` and return its staged files so the
    /// caller can unlink them. Runs in one transaction.
    pub async fn remove_installed_mod(
        &self,
        profile_id: i64,
        mod_id: &ModId,
    ) -> Result<Vec<StagedFile>> {
        let files = self.installed_files_for_mod(profile_id, mod_id).await?;
        let mut tx = self.db.begin().await?;
        tx.execute(
            "DELETE FROM installed_mod_files WHERE profile_id = ? AND mod_id = ?",
            &vals![profile_id, mod_id],
        )
        .await?;
        tx.execute(
            "DELETE FROM profile_mods WHERE profile_id = ? AND mod_id = ?",
            &vals![profile_id, mod_id],
        )
        .await?;
        tx.commit().await?;
        Ok(files)
    }

    /// Return every file tagged with `merge_group`, across all mods in `profile_id`.
    pub async fn files_in_merge_group(
        &self,
        profile_id: i64,
        merge_group: &str,
    ) -> Result<Vec<(String, StagedFile)>> {
        self.db
            .fetch_all(
                "SELECT mod_id, rel_path, origin_rel_path, size, merge_group
                   FROM installed_mod_files
                  WHERE profile_id = ? AND merge_group = ?
               ORDER BY mod_id, rel_path",
                &vals![profile_id, merge_group],
                |r| {
                    Ok((
                        r.string(0)?,
                        StagedFile {
                            rel_path: r.string(1)?,
                            origin_rel_path: r.string(2)?,
                            size: r.i64(3)?.max(0) as u64,
                            merge_group: r.opt_string(4)?,
                        },
                    ))
                },
            )
            .await
    }

    /// Return every installer-tracked file in a profile, paired with its owning
    /// mod id. Crash-log correlation uses this to map mentioned DLLs/assets
    /// back to the managed install database.
    pub async fn installed_files_for_profile(
        &self,
        profile_id: i64,
    ) -> Result<Vec<(String, StagedFile)>> {
        self.db
            .fetch_all(
                "SELECT mod_id, rel_path, origin_rel_path, size, merge_group
                   FROM installed_mod_files
                  WHERE profile_id = ?
               ORDER BY mod_id, rel_path",
                &vals![profile_id],
                |r| {
                    Ok((
                        r.string(0)?,
                        StagedFile {
                            rel_path: r.string(1)?,
                            origin_rel_path: r.string(2)?,
                            size: r.i64(3)?.max(0) as u64,
                            merge_group: r.opt_string(4)?,
                        },
                    ))
                },
            )
            .await
    }
}
