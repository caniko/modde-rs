use std::path::{Path, PathBuf};

use super::SaveManager;
use super::helpers::is_live_metadata;
use crate::db::SaveEntry;
use crate::error::Result;
use crate::resolver::GameId;

impl SaveManager<'_> {
    pub async fn detect_unadopted(
        &self,
        game_id: &GameId,
        game_save_dir: &Path,
    ) -> Result<Option<usize>> {
        if self.db.get_active_profile(game_id).await?.is_some() {
            return Ok(None);
        }

        if !game_save_dir.exists() {
            return Ok(None);
        }

        let mut count = 0;
        for entry in std::fs::read_dir(game_save_dir)? {
            let entry = entry?;
            if !is_live_metadata(&entry.file_name().to_string_lossy()) {
                count += 1;
            }
        }

        if count > 0 { Ok(Some(count)) } else { Ok(None) }
    }

    /// Adopt existing saves from the game's save directory into a profile's vault.
    /// Returns the number of files adopted.
    pub fn adopt(
        &self,
        game_id: &GameId,
        profile_name: &str,
        game_save_dir: &Path,
    ) -> Result<usize> {
        Self::ensure_branch(game_id, profile_name)?;
        self.capture(game_id, profile_name, game_save_dir)
    }

    // ── DB-level save tracking ───────────────────────────────────

    /// Assign a save file or directory to a profile.
    pub async fn assign(&self, profile_id: i64, path: &Path, label: Option<&str>) -> Result<()> {
        self.db.assign_save(profile_id, path, label).await
    }

    /// Remove a save assignment.
    pub async fn unassign(&self, path: &Path) -> Result<()> {
        self.db.unassign_save(path).await
    }

    /// List all saves assigned to a profile.
    pub async fn list(&self, profile_id: i64) -> Result<Vec<SaveEntry>> {
        self.db.list_saves(profile_id).await
    }

    /// Scan a save directory and return paths not yet assigned to any profile.
    pub async fn list_unassigned(&self, game_save_dir: &Path) -> Result<Vec<PathBuf>> {
        if !game_save_dir.exists() {
            return Ok(Vec::new());
        }

        let mut unassigned = Vec::new();

        for entry in std::fs::read_dir(game_save_dir)?.flatten() {
            let path = entry.path();
            if !self.db.is_save_assigned(&path).await? {
                unassigned.push(path);
            }
        }

        Ok(unassigned)
    }
}
