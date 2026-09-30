//! Save assignments, active profile state, experiments, and stock snapshots.
#![allow(clippy::wildcard_imports)]

use super::*;

impl ModdeDb {
    pub async fn assign_save(
        &self,
        profile_id: i64,
        path: &Path,
        label: Option<&str>,
    ) -> Result<()> {
        let path_str = path.to_string_lossy().to_string();

        let existing = self
            .db
            .fetch_optional(
                "SELECT s.profile_id, p.name FROM saves s
                 JOIN profiles p ON p.id = s.profile_id
                 WHERE s.path = ?",
                &vals![path_str.clone()],
                |r| Ok((r.i64(0)?, r.string(1)?)),
            )
            .await?;

        if let Some((existing_id, existing_name)) = existing {
            if existing_id != profile_id {
                return Err(CoreError::SaveAlreadyAssigned {
                    path: path_str,
                    profile: existing_name,
                });
            }
            self.db
                .execute(
                    "UPDATE saves SET label = ? WHERE path = ?",
                    &vals![label.map(str::to_string), path_str],
                )
                .await?;
            return Ok(());
        }

        self.db
            .execute(
                "INSERT INTO saves (profile_id, path, label) VALUES (?, ?, ?)",
                &vals![profile_id, path_str, label.map(str::to_string)],
            )
            .await?;
        Ok(())
    }

    /// Remove a save assignment.
    pub async fn unassign_save(&self, path: &Path) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM saves WHERE path = ?",
                &vals![path.to_string_lossy().to_string()],
            )
            .await?;
        Ok(())
    }

    /// List all saves assigned to a profile.
    pub async fn list_saves(&self, profile_id: i64) -> Result<Vec<SaveEntry>> {
        self.db
            .fetch_all(
                "SELECT path, label, assigned_at FROM saves WHERE profile_id = ? ORDER BY assigned_at",
                &vals![profile_id],
                |r| {
                    Ok(SaveEntry {
                        path: PathBuf::from(r.string(0)?),
                        label: r.opt_string(1)?,
                        assigned_at: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Check if a save path is assigned to any profile.
    pub async fn is_save_assigned(&self, path: &Path) -> Result<bool> {
        let count = self
            .db
            .fetch_one(
                "SELECT COUNT(*) FROM saves WHERE path = ?",
                &vals![path.to_string_lossy().to_string()],
                |r| r.i64(0),
            )
            .await?;
        Ok(count > 0)
    }

    // ── Active Profile Tracking ────────────────────────────────────

    /// Set the active profile for a game, replacing any previous one.
    pub async fn set_active_profile(&self, game_id: &GameId, profile_id: i64) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO active_profiles (game_id, profile_id)
                 VALUES (?, ?)
                 ON CONFLICT(game_id) DO UPDATE SET
                    profile_id = excluded.profile_id,
                    activated_at = {NOW}",
                &vals![game_id, profile_id],
            )
            .await?;
        Ok(())
    }

    /// Get the active profile for a game, returning (`profile_id`, `profile_name`).
    pub async fn get_active_profile(&self, game_id: &GameId) -> Result<Option<(i64, String)>> {
        self.db
            .fetch_optional(
                "SELECT a.profile_id, p.name FROM active_profiles a
                 JOIN profiles p ON p.id = a.profile_id
                 WHERE a.game_id = ?",
                &vals![game_id],
                |r| Ok((r.i64(0)?, r.string(1)?)),
            )
            .await
    }

    /// Clear the active profile for a game.
    pub async fn clear_active_profile(&self, game_id: &GameId) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM active_profiles WHERE game_id = ?",
                &vals![game_id],
            )
            .await?;
        Ok(())
    }

    // ── Experiment Stack ──────────────────────────────────────────

    /// Ordered bottom-to-top snapshot used by save-transition recovery.
    pub async fn experiment_profiles(&self, game_id: &GameId) -> Result<Vec<i64>> {
        self.db.fetch_all(
            "SELECT profile_id FROM experiment_stack WHERE game_id = ? ORDER BY depth",
            &vals![game_id], |r| r.i64(0),
        ).await
    }

    /// Idempotent replacement while the mutation lease and recovery journal
    /// protect the operation. A failed write leaves the journal for replay.
    pub async fn replace_experiment_profiles(&self, game_id: &GameId, profiles: &[i64]) -> Result<()> {
        self.clear_experiment_stack(game_id).await?;
        for (depth, profile_id) in profiles.iter().enumerate() {
            self.db.execute(
                "INSERT INTO experiment_stack (game_id, profile_id, depth) VALUES (?, ?, ?)",
                &vals![game_id, *profile_id, depth as i64],
            ).await?;
        }
        Ok(())
    }

    /// Push a profile onto the experiment stack for a game.
    pub async fn push_experiment(&self, game_id: &GameId, profile_id: i64) -> Result<()> {
        let depth = self.experiment_depth(game_id).await?;
        self.db
            .execute(
                "INSERT INTO experiment_stack (game_id, profile_id, depth)
                 VALUES (?, ?, ?)",
                &vals![game_id, profile_id, depth as i64],
            )
            .await?;
        Ok(())
    }

    /// Read the top experiment without removing the rollback destination.
    pub async fn peek_experiment(&self, game_id: &GameId) -> Result<Option<i64>> {
        self.db.fetch_optional(
            "SELECT profile_id FROM experiment_stack WHERE game_id = ? ORDER BY depth DESC LIMIT 1",
            &vals![game_id], |r| r.i64(0),
        ).await
    }

    /// Pop only after a successful save/profile switch.
    pub async fn pop_experiment(&self, game_id: &GameId) -> Result<Option<i64>> {
        let top = self
            .db
            .fetch_optional(
                "SELECT id, profile_id FROM experiment_stack
                 WHERE game_id = ? ORDER BY depth DESC LIMIT 1",
                &vals![game_id],
                |r| Ok((r.i64(0)?, r.i64(1)?)),
            )
            .await?;

        match top {
            Some((id, profile_id)) => {
                self.db
                    .execute("DELETE FROM experiment_stack WHERE id = ?", &vals![id])
                    .await?;
                Ok(Some(profile_id))
            }
            None => Ok(None),
        }
    }

    /// Get the experiment stack depth for a game.
    pub async fn experiment_depth(&self, game_id: &GameId) -> Result<usize> {
        let count = self
            .db
            .fetch_one(
                "SELECT COUNT(*) FROM experiment_stack WHERE game_id = ?",
                &vals![game_id],
                |r| r.i64(0),
            )
            .await?;
        Ok(count as usize)
    }

    /// Clear the entire experiment stack for a game.
    pub async fn clear_experiment_stack(&self, game_id: &GameId) -> Result<()> {
        self.db
            .execute(
                "DELETE FROM experiment_stack WHERE game_id = ?",
                &vals![game_id],
            )
            .await?;
        Ok(())
    }

    // ── Stock Snapshots ───────────────────────────────────────────

    /// Insert or update a stock snapshot record.
    pub async fn upsert_snapshot(
        &self,
        game_id: &GameId,
        snapshot_path: &Path,
        tree_hash: &str,
        file_count: usize,
    ) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO stock_snapshots (game_id, snapshot_path, tree_hash, file_count)
                 VALUES (?, ?, ?, ?)
                 ON CONFLICT(game_id) DO UPDATE SET
                    snapshot_path = excluded.snapshot_path,
                    tree_hash = excluded.tree_hash,
                    file_count = excluded.file_count,
                    created_at = {NOW}",
                &vals![
                    game_id,
                    snapshot_path.to_string_lossy().to_string(),
                    tree_hash,
                    file_count as i64,
                ],
            )
            .await?;
        Ok(())
    }

    /// Get snapshot metadata for a game.
    pub async fn get_snapshot(&self, game_id: &GameId) -> Result<Option<SnapshotMeta>> {
        self.db
            .fetch_optional(
                "SELECT game_id, snapshot_path, tree_hash, file_count, created_at
                 FROM stock_snapshots WHERE game_id = ?",
                &vals![game_id],
                |r| {
                    Ok(SnapshotMeta {
                        game_id: GameId::from(r.string(0)?),
                        snapshot_path: PathBuf::from(r.string(1)?),
                        tree_hash: r.string(2)?,
                        file_count: r.i64(3)? as usize,
                        created_at: r.string(4)?,
                    })
                },
            )
            .await
    }
}
