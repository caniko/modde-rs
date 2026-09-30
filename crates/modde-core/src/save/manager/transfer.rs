use std::path::Path;

use git2::IndexAddOption;
use tracing::info;

use super::SaveManager;
use super::helpers::{
    clear_active_save_dir, copy_dir_contents_filtered, is_live_metadata, park_active_saves,
    remove_live_metadata_from_vault, sanitize_branch_name, vault_signature,
};
use crate::error::{CoreError, Result};
use crate::resolver::GameId;
use crate::save::SaveFingerprint;

impl SaveManager<'_> {
    /// Recovery of a first activation whose original live save set was empty.
    /// Steam Cloud metadata and modde's parked profiles are preserved.
    pub fn clear_live_saves(game_save_dir: &Path) -> Result<()> {
        clear_active_save_dir(game_save_dir)
    }

    pub fn capture_with_fingerprint(
        &self,
        game_id: &GameId,
        profile_name: &str,
        game_save_dir: &Path,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<usize> {
        if !game_save_dir.exists() {
            return Ok(0);
        }

        let repo = Self::vault_repo(game_id)?;
        let vault_path = crate::paths::save_vault_dir(game_id);

        Self::checkout_branch(game_id, profile_name)?;

        remove_live_metadata_from_vault(&vault_path)?;
        // Capture is a snapshot, not an overlay: deleted live files must also
        // disappear from the vault, including when the live set becomes empty.
        for entry in std::fs::read_dir(&vault_path)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            if entry.file_type()?.is_dir() {
                std::fs::remove_dir_all(entry.path())?;
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }

        let count =
            copy_dir_contents_filtered(game_save_dir, &vault_path, |name| !is_live_metadata(name))?;

        // Stage and commit
        let mut index = repo
            .index()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get index: {e}")))?;

        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to stage files: {e}")))?;

        // Handle deletions — remove index entries for files no longer on disk
        let mut to_remove = Vec::new();
        for entry in index.iter() {
            let path = String::from_utf8_lossy(&entry.path).to_string();
            if !vault_path.join(&path).exists() {
                to_remove.push(path);
            }
        }
        for path in &to_remove {
            index.remove_path(Path::new(path)).map_err(|e| {
                CoreError::SaveVaultError(format!("failed to remove from index: {e}"))
            })?;
        }

        index
            .write()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to write index: {e}")))?;

        let tree_oid = index
            .write_tree()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to write tree: {e}")))?;
        let tree = repo
            .find_tree(tree_oid)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to find tree: {e}")))?;

        let head_commit = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get HEAD: {e}")))?;

        // Skip commit if tree is identical to HEAD (no actual changes)
        if tree_oid == head_commit.tree_id() {
            info!(
                game_id = %game_id,
                profile = profile_name,
                "saves unchanged, skipping commit"
            );
            return Ok(0);
        }

        let sig = vault_signature();

        let mut message = format!("capture saves for profile '{profile_name}'");
        if let Some(fp) = fingerprint {
            message.push_str("\n\n");
            message.push_str(&fp.to_trailers());
        }

        repo.commit(Some("HEAD"), &sig, &sig, &message, &tree, &[&head_commit])
            .map_err(|e| CoreError::SaveVaultError(format!("failed to commit: {e}")))?;

        info!(game_id = %game_id, profile = profile_name, count, "captured saves");
        Ok(count)
    }

    /// Capture saves without a fingerprint (backwards-compatible convenience method).
    pub fn capture(
        &self,
        game_id: &GameId,
        profile_name: &str,
        game_save_dir: &Path,
    ) -> Result<usize> {
        self.capture_with_fingerprint(game_id, profile_name, game_save_dir, None)
    }

    /// Deploy saves from the vault to the game's save directory.
    /// Returns the number of files deployed.
    pub fn deploy(
        &self,
        game_id: &GameId,
        profile_name: &str,
        game_save_dir: &Path,
    ) -> Result<usize> {
        Self::checkout_branch(game_id, profile_name)?;

        let vault_path = crate::paths::save_vault_dir(game_id);

        // Clear only active saves. Steam Cloud metadata and modde's parked
        // inactive saves must stay in place.
        clear_active_save_dir(game_save_dir)?;

        std::fs::create_dir_all(game_save_dir)?;

        // Copy from vault working tree to game dir (skip .git and live metadata
        // captured by older versions).
        let count = copy_dir_contents_filtered(&vault_path, game_save_dir, |name| {
            name != ".git" && !is_live_metadata(name)
        })?;

        info!(game_id = %game_id, profile = profile_name, count, "deployed saves");
        Ok(count)
    }

    // ── High-level operations ────────────────────────────────────

    /// Full activate flow: capture current profile's saves, checkout + deploy new.
    ///
    /// If `fingerprint` is provided, it is embedded in the capture commit for
    /// the *current* profile's saves (the ones being put away).
    pub fn activate(
        &self,
        game_id: &GameId,
        new_profile: &str,
        current_profile: Option<&str>,
        game_save_dir: &Path,
    ) -> Result<()> {
        self.activate_with_fingerprint(game_id, new_profile, current_profile, game_save_dir, None)
    }

    /// Full activate flow with an optional mod fingerprint.
    pub fn activate_with_fingerprint(
        &self,
        game_id: &GameId,
        new_profile: &str,
        current_profile: Option<&str>,
        game_save_dir: &Path,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<()> {
        if let Some(current) = current_profile {
            self.capture_with_fingerprint(game_id, current, game_save_dir, fingerprint)?;
        }

        self.activate_captured(game_id, new_profile, current_profile, game_save_dir)
    }

    /// Replace live saves after the caller has captured the outgoing profile.
    /// A write-ahead journal can be persisted between capture and this step.
    pub fn activate_captured(
        &self,
        game_id: &GameId,
        new_profile: &str,
        current_profile: Option<&str>,
        game_save_dir: &Path,
    ) -> Result<()> {
        if let Some(current) = current_profile {
            park_active_saves(game_save_dir, current)?;
        }
        Self::ensure_branch(game_id, new_profile)?;
        self.deploy(game_id, new_profile, game_save_dir)?;

        Ok(())
    }

    /// Fork saves from one profile's branch to a new profile's branch.
    pub fn fork_saves(game_id: &GameId, source_profile: &str, target_profile: &str) -> Result<()> {
        let repo = Self::vault_repo(game_id)?;
        let source_branch = sanitize_branch_name(source_profile);
        let target_branch = sanitize_branch_name(target_profile);

        Self::ensure_branch(game_id, source_profile)?;

        let branch = repo
            .find_branch(&source_branch, git2::BranchType::Local)
            .map_err(|e| CoreError::SaveVaultError(format!("source branch not found: {e}")))?;

        let commit = branch
            .get()
            .peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get source commit: {e}")))?;

        repo.branch(&target_branch, &commit, false)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to create fork branch: {e}")))?;

        info!(
            game_id = %game_id,
            source = source_profile,
            target = target_profile,
            "forked save branch"
        );
        Ok(())
    }
}
