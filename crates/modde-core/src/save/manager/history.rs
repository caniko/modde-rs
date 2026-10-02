use std::path::Path;

use tracing::info;

use super::SaveManager;
use super::helpers::{
    clear_active_save_dir, collect_tree_paths, copy_dir_contents_filtered, count_tree_entries,
    is_live_metadata, sanitize_branch_name,
};
use crate::error::{CoreError, Result};
use crate::resolver::GameId;
use crate::save::{FingerprintCheck, SaveFingerprint, SaveSnapshot};

impl SaveManager<'_> {
    pub fn history(
        game_id: &GameId,
        profile_name: &str,
        limit: usize,
    ) -> Result<Vec<SaveSnapshot>> {
        let repo = Self::vault_repo(game_id)?;
        let branch_name = sanitize_branch_name(profile_name);

        let branch = repo
            .find_branch(&branch_name, git2::BranchType::Local)
            .map_err(|e| {
                CoreError::SaveVaultError(format!("branch '{branch_name}' not found: {e}"))
            })?;

        let commit_oid = branch
            .get()
            .target()
            .ok_or_else(|| CoreError::SaveVaultError("branch has no target".into()))?;

        let mut revwalk = repo
            .revwalk()
            .map_err(|e| CoreError::SaveVaultError(format!("revwalk failed: {e}")))?;
        revwalk
            .push(commit_oid)
            .map_err(|e| CoreError::SaveVaultError(format!("revwalk push failed: {e}")))?;

        let mut snapshots = Vec::new();
        for oid in revwalk.take(limit) {
            let oid = oid.map_err(|e| CoreError::SaveVaultError(format!("revwalk iter: {e}")))?;
            let commit = repo
                .find_commit(oid)
                .map_err(|e| CoreError::SaveVaultError(format!("find commit: {e}")))?;

            let message = commit.message().unwrap_or("").to_string();
            let time = commit.time();
            let secs = time.seconds();

            // Count files in tree
            let tree = commit
                .tree()
                .map_err(|e| CoreError::SaveVaultError(format!("commit tree: {e}")))?;
            let file_count = count_tree_entries(&repo, &tree);

            let fingerprint = SaveFingerprint::from_commit_message(&message);

            let mut snap = SaveSnapshot {
                id: oid.to_string(),
                message,
                timestamp: secs,
                file_count,
                fingerprint,
                profile_name: None,
                character_name: None,
                save_label: None,
                category: None,
            };
            snap.parse_metadata_from_message();
            snapshots.push(snap);
        }

        Ok(snapshots)
    }

    /// Check whether restoring a snapshot is compatible with the current mod fingerprint.
    ///
    /// Returns `FingerprintCheck` without performing the restore — use this
    /// to warn the user before calling `restore`.
    pub fn check_restore_compatibility(
        game_id: &GameId,
        _profile_name: &str,
        commit_id: &str,
        current_fingerprint: &SaveFingerprint,
    ) -> Result<FingerprintCheck> {
        let repo = Self::vault_repo(game_id)?;
        let obj = repo.revparse_single(commit_id).map_err(|e| {
            CoreError::SaveVaultError(format!("could not find commit '{commit_id}': {e}"))
        })?;
        let commit = obj
            .peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("not a commit: {e}")))?;

        let message = commit.message().unwrap_or("").to_string();
        let snapshot_fp = SaveFingerprint::from_commit_message(&message);

        match snapshot_fp {
            Some(fp) => {
                let snapshot = SaveSnapshot {
                    id: commit_id.to_string(),
                    message,
                    timestamp: 0,
                    file_count: 0,
                    fingerprint: Some(fp),
                    profile_name: None,
                    character_name: None,
                    save_label: None,
                    category: None,
                };
                Ok(snapshot.check_compatibility(current_fingerprint))
            }
            None => Ok(FingerprintCheck::NoFingerprint),
        }
    }

    /// Restore saves from a specific commit to the game save directory.
    pub fn restore(
        game_id: &GameId,
        profile_name: &str,
        commit_id: &str,
        game_save_dir: &Path,
    ) -> Result<usize> {
        let repo = Self::vault_repo(game_id)?;
        let vault_path = crate::paths::save_vault_dir(game_id);

        // Resolve commit
        let obj = repo.revparse_single(commit_id).map_err(|e| {
            CoreError::SaveVaultError(format!("could not find commit '{commit_id}': {e}"))
        })?;
        let commit = obj
            .peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("not a commit: {e}")))?;

        // Checkout that commit's tree into the vault working directory
        let branch_name = sanitize_branch_name(profile_name);
        Self::checkout_branch(game_id, profile_name)?;

        repo.checkout_tree(
            commit.as_object(),
            Some(
                git2::build::CheckoutBuilder::new()
                    .force()
                    .remove_untracked(true)
                    .remove_ignored(true),
            ),
        )
        .map_err(|e| CoreError::SaveVaultError(format!("checkout failed: {e}")))?;

        // Reset the branch to point at this commit
        let refname = format!("refs/heads/{branch_name}");
        repo.reference(
            &refname,
            commit.id(),
            true,
            &format!("restore to {commit_id}"),
        )
        .map_err(|e| CoreError::SaveVaultError(format!("failed to reset branch: {e}")))?;
        repo.set_head(&refname)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to set HEAD: {e}")))?;

        // Deploy from vault to game
        clear_active_save_dir(game_save_dir)?;
        std::fs::create_dir_all(game_save_dir)?;

        let count = copy_dir_contents_filtered(&vault_path, game_save_dir, |name| {
            name != ".git" && !is_live_metadata(name)
        })?;

        info!(
            game_id = %game_id,
            profile = profile_name,
            commit = commit_id,
            count,
            "restored saves from snapshot"
        );
        Ok(count)
    }

    /// List file paths in a specific snapshot's git tree.
    pub fn snapshot_file_list(game_id: &GameId, commit_id: &str) -> Result<Vec<String>> {
        let repo = Self::vault_repo(game_id)?;
        let obj = repo.revparse_single(commit_id).map_err(|e| {
            CoreError::SaveVaultError(format!("could not find commit '{commit_id}': {e}"))
        })?;
        let commit = obj
            .peel_to_commit()
            .map_err(|e| CoreError::SaveVaultError(format!("not a commit: {e}")))?;
        let tree = commit
            .tree()
            .map_err(|e| CoreError::SaveVaultError(format!("commit tree: {e}")))?;
        Ok(collect_tree_paths(&repo, &tree, ""))
    }
}
