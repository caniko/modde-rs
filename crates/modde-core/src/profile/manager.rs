//! SQLite-backed profile manager operations.

use std::path::{Path, PathBuf};

use super::{ActivateResult, ActiveProfileInfo, ForkOptions, Profile, validate_profile_name};
use crate::db::{ModdeDb, ProfileSummary};
use crate::error::{CoreError, Result};
use crate::library::SaveContext;
use crate::resolver::GameId;
use crate::save::{SaveFingerprint, SaveManager};

pub struct ProfileManager {
    db: ModdeDb,
}

impl ProfileManager {
    /// Open the profile manager using the configured database backend.
    pub async fn open() -> Result<Self> {
        let db = ModdeDb::open().await?;
        Ok(Self { db })
    }

    /// Create a profile manager with a custom database (for testing).
    pub fn with_db(db: ModdeDb) -> Self {
        Self { db }
    }

    /// Access the underlying database.
    pub fn db(&self) -> &ModdeDb {
        &self.db
    }

    /// List profile summaries, optionally filtered by game.
    pub async fn list(&self) -> Result<Vec<ProfileSummary>> {
        self.db.list_profiles(None).await
    }

    /// List profiles for a specific game.
    pub async fn list_for_game(&self, game_id: &GameId) -> Result<Vec<ProfileSummary>> {
        self.db.list_profiles(Some(game_id)).await
    }

    /// Load a profile by name. If `game_id` is None, the name must be unambiguous.
    pub async fn load(&self, name: &str, game_id: Option<&GameId>) -> Result<Profile> {
        match game_id {
            Some(gid) => self.db.load_profile(name, gid).await,
            None => self.db.load_profile_by_name(name).await,
        }
    }

    /// Create a new profile, returning its database ID.
    pub async fn create(&self, profile: &Profile) -> Result<i64> {
        validate_profile_name(&profile.name)?;
        self.db.create_profile(profile).await
    }

    /// Update an existing profile.
    pub async fn update(&self, profile: &Profile) -> Result<()> {
        self.db.update_profile(profile).await
    }

    /// Create a profile if it doesn't exist, or update it if it does.
    pub async fn create_or_update(&self, profile: &Profile) -> Result<i64> {
        validate_profile_name(&profile.name)?;
        match self.db.create_profile(profile).await {
            Ok(id) => Ok(id),
            Err(CoreError::Database(_)) => {
                self.db.update_profile(profile).await?;
                // Return the existing ID
                let loaded = self
                    .db
                    .load_profile(&profile.name, &profile.game_id)
                    .await?;
                Ok(loaded.id.unwrap_or(0))
            }
            Err(e) => Err(e),
        }
    }

    /// Delete a profile. If `game_id` is None, the name must be unambiguous.
    pub async fn delete(&self, name: &str, game_id: Option<&GameId>) -> Result<()> {
        if let Some(gid) = game_id {
            self.db.delete_profile(name, gid).await
        } else {
            // Resolve the game_id first
            let profile = self.db.load_profile_by_name(name).await?;
            self.db.delete_profile(name, &profile.game_id).await
        }
    }

    /// Import existing TOML profile files into the database.
    pub async fn import_toml(&self, profiles_dir: &Path) -> Result<usize> {
        self.db.import_toml_profiles(profiles_dir).await
    }

    /// Staging directory for a profile (still on-disk).
    #[must_use]
    pub fn staging_dir(name: &str) -> PathBuf {
        crate::paths::profiles_dir().join(name).join("staging")
    }

    /// Default overrides directory for a profile.
    #[must_use]
    pub fn default_overrides(name: &str) -> PathBuf {
        crate::paths::profiles_dir().join(name).join("overrides")
    }

    // ── Save-aware profile management ────────────────────────────

    /// Activate a profile, swapping saves automatically.
    ///
    /// `save_dir` is the game's save directory (resolved by the caller via
    /// `GamePlugin::save_directory()`). If `None`, save swapping is skipped.
    ///
    /// `fingerprint` is the current profile's mod fingerprint. If provided,
    /// it is embedded in the save vault commit so future restores can warn
    /// about mod mismatches.
    ///
    /// If existing saves are detected with no active profile, returns
    /// `ActivateResult::AdoptionRequired` so the caller can prompt the user.
    pub async fn activate(
        &self,
        name: &str,
        game_id: &GameId,
        save_dir: Option<&Path>,
    ) -> Result<ActivateResult> {
        self.activate_with_fingerprint(name, game_id, save_dir, None)
            .await
    }

    /// Activate with an optional mod fingerprint embedded in the save capture.
    pub async fn activate_with_fingerprint(
        &self,
        name: &str,
        game_id: &GameId,
        save_dir: Option<&Path>,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<ActivateResult> {
        self.activate_scoped(name, &SaveContext::legacy(game_id, save_dir), fingerprint)
            .await
    }

    pub async fn activate_scoped(
        &self,
        name: &str,
        context: &SaveContext,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<ActivateResult> {
        let game_id = &context.game_id;
        let scope = &context.scope;
        let profile = self.db.load_profile(name, game_id).await?;
        let profile_id = profile
            .id
            .ok_or_else(|| CoreError::Other("profile has no database ID".into()))?;

        if let Some(dir) = context.directory.as_deref() {
            let sm = SaveManager::new(&self.db);

            // Check for unadopted saves
            if let Some(count) = sm.detect_unadopted(scope, dir).await? {
                return Ok(ActivateResult::AdoptionRequired { save_count: count });
            }

            // Get current active profile (if any) to capture its saves
            let current = self.db.get_active_profile(scope).await?;
            let current_name = current.map(|(_, name)| name);

            sm.activate_with_fingerprint(scope, name, current_name.as_deref(), dir, fingerprint)?;
        }

        self.db.set_active_profile(scope, profile_id).await?;

        Ok(ActivateResult::Activated)
    }

    /// Try a profile experimentally, pushing the current profile onto the stack.
    ///
    /// `save_dir` is the game's save directory. If `None`, save swapping is skipped.
    /// `fingerprint` is the current profile's mod fingerprint.
    pub async fn try_profile(
        &self,
        name: &str,
        game_id: &GameId,
        save_dir: Option<&Path>,
    ) -> Result<()> {
        self.try_profile_with_fingerprint(name, game_id, save_dir, None)
            .await
    }

    /// Try a profile experimentally with a mod fingerprint.
    pub async fn try_profile_with_fingerprint(
        &self,
        name: &str,
        game_id: &GameId,
        save_dir: Option<&Path>,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<()> {
        self.try_profile_scoped(name, &SaveContext::legacy(game_id, save_dir), fingerprint)
            .await
    }

    pub async fn try_profile_scoped(
        &self,
        name: &str,
        context: &SaveContext,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<()> {
        let game_id = &context.game_id;
        let scope = &context.scope;
        let (current_id, current_name) = self
            .db
            .get_active_profile(scope)
            .await?
            .ok_or_else(|| CoreError::NoActiveProfile(game_id.to_string()))?;

        let new_profile = self.db.load_profile(name, game_id).await?;
        let new_id = new_profile
            .id
            .ok_or_else(|| CoreError::Other("profile has no database ID".into()))?;

        // Push current profile onto experiment stack (before switching)
        self.db.push_experiment(scope, current_id).await?;

        if let Some(dir) = context.directory.as_deref() {
            let sm = SaveManager::new(&self.db);
            sm.activate_with_fingerprint(scope, name, Some(&current_name), dir, fingerprint)?;
        }

        self.db.set_active_profile(scope, new_id).await?;

        Ok(())
    }

    /// Roll back to the previous profile on the experiment stack.
    /// Returns the name of the profile we rolled back to.
    ///
    /// `save_dir` is the game's save directory. If `None`, save swapping is skipped.
    /// `fingerprint` is the current profile's mod fingerprint.
    pub async fn rollback(&self, game_id: &GameId, save_dir: Option<&Path>) -> Result<String> {
        self.rollback_with_fingerprint(game_id, save_dir, None)
            .await
    }

    /// Roll back with a mod fingerprint.
    pub async fn rollback_with_fingerprint(
        &self,
        game_id: &GameId,
        save_dir: Option<&Path>,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<String> {
        self.rollback_scoped(&SaveContext::legacy(game_id, save_dir), fingerprint)
            .await
    }

    pub async fn rollback_scoped(
        &self,
        context: &SaveContext,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<String> {
        let game_id = &context.game_id;
        let scope = &context.scope;
        let prev_id = self
            .db
            .peek_experiment(scope)
            .await?
            .ok_or_else(|| CoreError::NotInExperiment(game_id.to_string()))?;

        let (_current_id, current_name) = self
            .db
            .get_active_profile(scope)
            .await?
            .ok_or_else(|| CoreError::NoActiveProfile(game_id.to_string()))?;

        let prev_profile = self.db.load_profile_by_id(prev_id).await?;
        if prev_profile.game_id != *game_id {
            return Err(CoreError::Other(
                "experiment profile belongs to a different game".into(),
            ));
        }

        if let Some(dir) = context.directory.as_deref() {
            let sm = SaveManager::new(&self.db);
            sm.activate_with_fingerprint(
                scope,
                &prev_profile.name,
                Some(&current_name),
                dir,
                fingerprint,
            )?;
        }

        self.db.set_active_profile(scope, prev_id).await?;
        self.db.pop_experiment(scope).await?;

        Ok(prev_profile.name.clone())
    }

    /// Accept the current experiment, clearing the experiment stack.
    pub async fn commit(&self, game_id: &GameId) -> Result<()> {
        let depth = self.db.experiment_depth(game_id).await?;
        if depth == 0 {
            return Err(CoreError::NotInExperiment(game_id.to_string()));
        }
        self.db.clear_experiment_stack(game_id).await?;
        Ok(())
    }

    /// Get the currently active profile and experiment depth for a game.
    pub async fn active(&self, game_id: &GameId) -> Result<Option<ActiveProfileInfo>> {
        let (profile_id, _name) = match self.db.get_active_profile(game_id).await? {
            Some(pair) => pair,
            None => return Ok(None),
        };

        let profile = self.db.load_profile_by_id(profile_id).await?;
        let experiment_depth = self.db.experiment_depth(game_id).await?;

        Ok(Some(ActiveProfileInfo {
            profile,
            experiment_depth,
        }))
    }

    /// Fork a profile: clone its mods, load order rules, and save branch.
    ///
    /// By default this is a **faithful copy** — both the profile-level
    /// `load_order_lock` and every per-mod pin ride along. Use
    /// [`Self::fork_with_options`] (or `modde profile fork --unlock`) for
    /// the "fork to diverge" workflow where the new profile starts
    /// unlocked so it can be freely reorganised.
    pub async fn fork(&self, source_name: &str, new_name: &str, game_id: &GameId) -> Result<i64> {
        self.fork_with_options(source_name, new_name, game_id, ForkOptions::default())
            .await
    }

    /// Fork a profile with explicit control over whether the new profile
    /// inherits locks. See [`ForkOptions`] for the flags.
    pub async fn fork_with_options(
        &self,
        source_name: &str,
        new_name: &str,
        game_id: &GameId,
        options: ForkOptions,
    ) -> Result<i64> {
        self.fork_scoped(
            source_name,
            new_name,
            &SaveContext::legacy(game_id, None),
            options,
        )
        .await
    }

    pub async fn fork_scoped(
        &self,
        source_name: &str,
        new_name: &str,
        context: &SaveContext,
        options: ForkOptions,
    ) -> Result<i64> {
        let game_id = &context.game_id;
        validate_profile_name(new_name)?;
        let source = self.db.load_profile(source_name, game_id).await?;

        // Clone then optionally strip. Done in two steps so the decision
        // logic is obvious — one place to look when auditing lock flow.
        let mut mods = source.mods.clone();
        let mut load_order_lock = source.load_order_lock.clone();
        if options.unlock {
            load_order_lock = None;
            for m in &mut mods {
                m.lock = None;
            }
        }

        let new_profile = Profile {
            id: None,
            name: new_name.to_string(),
            game_id: game_id.clone(),
            source: source.source.clone(),
            mods,
            overrides: Self::default_overrides(new_name),
            load_order_rules: source.load_order_rules.clone(),
            load_order_lock,
        };

        let new_id = self.db.create_profile(&new_profile).await?;

        // Fork the save branch
        SaveManager::fork_saves(&context.scope, source_name, new_name)?;

        Ok(new_id)
    }
}
