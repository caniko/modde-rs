use git2::{Repository, RepositoryInitOptions};
use tracing::info;

use super::SaveManager;
use super::helpers::{sanitize_branch_name, vault_signature};
use crate::error::{CoreError, Result};
use crate::resolver::GameId;

impl SaveManager<'_> {
    /// Resolve revision syntax before a capture or restore can move HEAD.
    pub fn resolve_snapshot(game_id: &GameId, revision: &str) -> Result<String> {
        let repo = Self::vault_repo(game_id)?;
        let commit = repo
            .revparse_single(revision)
            .and_then(|object| object.peel_to_commit())
            .map_err(|error| {
                CoreError::SaveVaultError(format!("invalid save snapshot '{revision}': {error}"))
            })?;
        Ok(commit.id().to_string())
    }

    pub fn profile_snapshot(game_id: &GameId, profile_name: &str) -> Result<String> {
        Self::resolve_snapshot(
            game_id,
            &format!("refs/heads/{}", sanitize_branch_name(profile_name)),
        )
    }

    pub fn init_vault(game_id: &GameId) -> Result<Repository> {
        let vault_path = crate::paths::save_vault_dir(game_id);
        if vault_path.join(".git").exists() {
            return Repository::open(&vault_path)
                .map_err(|e| CoreError::SaveVaultError(format!("failed to open vault: {e}")));
        }

        std::fs::create_dir_all(&vault_path)?;
        let mut opts = RepositoryInitOptions::new();
        opts.external_template(false).initial_head("main");
        let repo = Repository::init_opts(&vault_path, &opts)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to init vault: {e}")))?;

        // Create an initial empty commit on `main` so we have a root
        {
            let sig = vault_signature();
            let mut index = repo
                .index()
                .map_err(|e| CoreError::SaveVaultError(format!("failed to get index: {e}")))?;
            let tree_oid = index
                .write_tree()
                .map_err(|e| CoreError::SaveVaultError(format!("failed to write tree: {e}")))?;
            let tree = repo
                .find_tree(tree_oid)
                .map_err(|e| CoreError::SaveVaultError(format!("failed to find tree: {e}")))?;
            repo.commit(Some("HEAD"), &sig, &sig, "init save vault", &tree, &[])
                .map_err(|e| {
                    CoreError::SaveVaultError(format!("failed to create initial commit: {e}"))
                })?;
        }

        info!(game_id = %game_id, path = %vault_path.display(), "initialized save vault");
        Ok(repo)
    }

    /// Open an existing vault repo, or initialize it if it doesn't exist.
    pub fn vault_repo(game_id: &GameId) -> Result<Repository> {
        let vault_path = crate::paths::save_vault_dir(game_id);
        if vault_path.join(".git").exists() {
            Repository::open(&vault_path)
                .map_err(|e| CoreError::SaveVaultError(format!("failed to open vault: {e}")))
        } else {
            Self::init_vault(game_id)
        }
    }

    // ── Branch operations ────────────────────────────────────────

    /// Ensure a branch exists for a profile. Creates a branch if needed.
    pub fn ensure_branch(game_id: &GameId, profile_name: &str) -> Result<()> {
        let repo = Self::vault_repo(game_id)?;
        let branch_name = sanitize_branch_name(profile_name);

        if repo
            .find_branch(&branch_name, git2::BranchType::Local)
            .is_ok()
        {
            return Ok(());
        }

        let head_commit = repo
            .head()
            .and_then(|h| h.peel_to_commit())
            .map_err(|e| CoreError::SaveVaultError(format!("failed to get HEAD: {e}")))?;

        repo.branch(&branch_name, &head_commit, false)
            .map_err(|e| {
                CoreError::SaveVaultError(format!("failed to create branch '{branch_name}': {e}"))
            })?;

        info!(game_id = %game_id, branch = %branch_name, "created save branch");
        Ok(())
    }

    /// Checkout a profile's branch, updating the working directory.
    pub fn checkout_branch(game_id: &GameId, profile_name: &str) -> Result<()> {
        let repo = Self::vault_repo(game_id)?;
        let branch_name = sanitize_branch_name(profile_name);

        Self::ensure_branch(game_id, profile_name)?;

        let branch = repo
            .find_branch(&branch_name, git2::BranchType::Local)
            .map_err(|e| {
                CoreError::SaveVaultError(format!("branch '{branch_name}' not found: {e}"))
            })?;

        let refname = branch
            .get()
            .name()
            .map_err(|e| CoreError::SaveVaultError(format!("invalid branch ref name: {e}")))?
            .to_string();

        let obj = repo
            .revparse_single(&refname)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to resolve branch: {e}")))?;

        repo.checkout_tree(
            &obj,
            Some(
                git2::build::CheckoutBuilder::new()
                    .force()
                    .remove_untracked(true)
                    .remove_ignored(true),
            ),
        )
        .map_err(|e| CoreError::SaveVaultError(format!("checkout failed: {e}")))?;

        repo.set_head(&refname)
            .map_err(|e| CoreError::SaveVaultError(format!("failed to set HEAD: {e}")))?;

        info!(game_id = %game_id, branch = %branch_name, "checked out save branch");
        Ok(())
    }
}
