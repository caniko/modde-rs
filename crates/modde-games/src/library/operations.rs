//! Installation-scoped, journalled entry points for CLI and GUI save actions.

use anyhow::{Context, Result, ensure};
use modde_core::library::{ExperimentChange, PendingSession};
use modde_core::profile::{ActivateResult, ProfileManager};
use modde_core::save::{SaveFingerprint, SaveManager};

use super::context::InstallationContext;

impl InstallationContext {
    async fn save_session(&self, pm: &ProfileManager) -> Result<PendingSession> {
        self.require_save_management()?;
        let mut session = PendingSession::for_save_operation(pm, &self.saves).await?;
        session.installation.clone_from(&self.game.id);
        session.name.clone_from(&self.game.name);
        session.install_path.clone_from(&self.game.install_path);
        session.prefix.clone_from(&self.prefix);
        Ok(session)
    }

    /// The caller holds the shared mutation lease through this operation.
    pub async fn activate_profile(&self, pm: &ProfileManager, name: &str, fingerprint: Option<&SaveFingerprint>) -> Result<ActivateResult> {
        if let Some(dir) = &self.saves.directory
            && let Some(save_count) = SaveManager::new(pm.db()).detect_unadopted(&self.saves.scope, dir).await? {
            return Ok(ActivateResult::AdoptionRequired { save_count });
        }
        self.change_profile(pm, name, fingerprint, ExperimentChange::Keep).await?;
        Ok(ActivateResult::Activated)
    }

    pub async fn try_profile(&self, pm: &ProfileManager, name: &str, fingerprint: Option<&SaveFingerprint>) -> Result<()> {
        ensure!(pm.db().get_active_profile(&self.saves.scope).await?.is_some(), "no active profile for this installation");
        self.change_profile(pm, name, fingerprint, ExperimentChange::PushPrevious).await
    }

    pub async fn rollback_profile(&self, pm: &ProfileManager, fingerprint: Option<&SaveFingerprint>) -> Result<String> {
        let id = pm.db().peek_experiment(&self.saves.scope).await?.context("this installation is not in an experiment")?;
        let profile = pm.db().load_profile_by_id(id).await?;
        ensure!(profile.game_id == self.saves.game_id, "experiment belongs to another game");
        self.change_profile(pm, &profile.name, fingerprint, ExperimentChange::PopPrevious).await?;
        Ok(profile.name)
    }

    async fn change_profile(&self, pm: &ProfileManager, name: &str, fingerprint: Option<&SaveFingerprint>, change: ExperimentChange) -> Result<()> {
        let profile = pm.load(name, Some(&self.saves.game_id)).await?;
        let mut session = self.save_session(pm).await?;
        session.save()?;
        session.switch_profile(pm, &profile, fingerprint, change).await
            .context("save transition interrupted; run `modde library recover` before retrying")?;
        if session.previous_profile.as_ref().is_none_or(|(id, _)| Some(*id) != profile.id) {
            modde_core::library::LibraryPreferences::update(|preferences| { preferences.needs_deploy.insert(self.game.id.clone()); })?;
        }
        session.commit_save_operation()
    }

    pub async fn restore_saves(&self, pm: &ProfileManager, profile: &str, revision: &str, fingerprint: Option<&SaveFingerprint>) -> Result<usize> {
        let target = pm.load(profile, Some(&self.saves.game_id)).await?;
        let snapshot = SaveManager::resolve_snapshot(&self.saves.scope, revision)?;
        let mut session = self.save_session(pm).await?;
        ensure!(session.previous_profile.as_ref().is_some_and(|(id, _)| Some(*id) == target.id),
            "activate this profile for the selected installation before restoring its saves");
        session.save()?;
        let count = session.restore_snapshot(pm, profile, &snapshot, fingerprint).await
            .context("save restore interrupted; run `modde library recover` before retrying")?;
        session.commit_save_operation()?;
        Ok(count)
    }
}
