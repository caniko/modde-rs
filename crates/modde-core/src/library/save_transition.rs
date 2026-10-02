//! One write-ahead save transition for launch preparation and save-only actions.
//! Callers retain the mutation lease until completion or durable interruption.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};

use super::{PendingSession, SaveContext, SessionPhase, normalized_path};
use crate::GameId;
use crate::profile::{Profile, ProfileManager};
use crate::save::{SaveFingerprint, SaveManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveTransition {
    pub previous_snapshot: Option<String>,
    pub experiments: Vec<i64>,
}

#[derive(Debug, Clone, Copy)]
pub enum ExperimentChange {
    Keep,
    PushPrevious,
    PopPrevious,
}

impl PendingSession {
    pub async fn for_save_operation(pm: &ProfileManager, context: &SaveContext) -> Result<Self> {
        ensure!(
            Self::load()?.is_none(),
            "an unfinished operation requires `modde library recover` or `modde library finish`"
        );
        Ok(Self {
            installation: context.scope.to_string(),
            name: context.game_id.to_string(),
            game_id: Some(context.game_id.to_string()),
            scope: context.scope.to_string(),
            profile: None,
            save_directory: context.directory.as_deref().map(normalized_path),
            capture: false,
            phase: SessionPhase::Preparing,
            previous_profile: pm.db().get_active_profile(&context.scope).await?,
            switched_profile: false,
            deployment_started: false,
            data_directory: Some(normalized_path(&crate::paths::modde_data_dir())),
            install_path: None,
            prefix: None,
            save_transition: None,
            observation: None,
            diagnostics: None,
            launch_request: None,
        })
    }

    /// Capture before the write-ahead marker. No live save or database state is
    /// replaced until the immutable snapshot and stack have been persisted.
    pub async fn prepare_save_transition(
        &mut self,
        pm: &ProfileManager,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<()> {
        self.require_owner()?;
        ensure!(
            self.phase == SessionPhase::Preparing && !self.switched_profile,
            "save transition already started; recover it before retrying"
        );
        let scope = GameId::from(self.scope.as_str());
        ensure!(
            pm.db().get_active_profile(&scope).await? == self.previous_profile,
            "active profile changed during preparation"
        );
        if let Some((id, _)) = &self.previous_profile {
            let previous = pm.db().load_profile_by_id(*id).await?;
            ensure!(
                self.game_id.as_deref() == Some(previous.game_id.as_str()),
                "active profile belongs to another game"
            );
        }
        let experiments = pm.db().experiment_profiles(&scope).await?;
        let sm = SaveManager::new(pm.db());
        let previous_snapshot = if let Some(dir) = &self.save_directory {
            if let Some((_, previous)) = &self.previous_profile {
                ensure!(
                    dir.is_dir(),
                    "active profile's save directory is missing; restore it before switching"
                );
                sm.capture_with_fingerprint(&scope, previous, dir, fingerprint)?;
                Some(SaveManager::profile_snapshot(&scope, previous)?)
            } else {
                ensure!(
                    sm.detect_unadopted(&scope, dir).await?.is_none(),
                    "existing saves require adoption before replacement"
                );
                None
            }
        } else {
            None
        };
        self.save_transition = Some(SaveTransition {
            previous_snapshot,
            experiments,
        });
        self.switched_profile = true;
        self.advance(SessionPhase::SwitchingSaves)
    }

    pub async fn switch_profile(
        &mut self,
        pm: &ProfileManager,
        profile: &Profile,
        fingerprint: Option<&SaveFingerprint>,
        change: ExperimentChange,
    ) -> Result<()> {
        ensure!(
            self.game_id.as_deref() == Some(profile.game_id.as_str()),
            "profile belongs to another game"
        );
        let profile_id = profile.id.context("profile has no database ID")?;
        let scope = GameId::from(self.scope.as_str());
        let mut experiments = pm.db().experiment_profiles(&scope).await?;
        match change {
            ExperimentChange::Keep => {}
            ExperimentChange::PushPrevious => experiments.push(
                self.previous_profile
                    .as_ref()
                    .context("no active profile")?
                    .0,
            ),
            ExperimentChange::PopPrevious => {
                ensure!(
                    experiments.pop() == Some(profile_id),
                    "experiment destination changed during preparation"
                );
            }
        }
        self.profile = Some(profile.name.clone());
        self.prepare_save_transition(pm, fingerprint).await?;
        if let Some(dir) = &self.save_directory {
            SaveManager::new(pm.db()).activate_captured(
                &scope,
                &profile.name,
                self.previous_profile
                    .as_ref()
                    .map(|(_, name)| name.as_str()),
                dir,
            )?;
        }
        pm.db().set_active_profile(&scope, profile_id).await?;
        if !matches!(change, ExperimentChange::Keep) {
            pm.db()
                .replace_experiment_profiles(&scope, &experiments)
                .await?;
        }
        Ok(())
    }

    pub async fn restore_snapshot(
        &mut self,
        pm: &ProfileManager,
        profile: &str,
        revision: &str,
        fingerprint: Option<&SaveFingerprint>,
    ) -> Result<usize> {
        if self
            .previous_profile
            .as_ref()
            .is_none_or(|(_, active)| active != profile)
        {
            bail!("activate this profile for the selected installation before restoring its saves");
        }
        let scope = GameId::from(self.scope.as_str());
        let snapshot = SaveManager::resolve_snapshot(&scope, revision)?;
        let dir = self
            .save_directory
            .clone()
            .context("save directory is not configured")?;
        // Restoring a lost directory is valid: capture its empty pre-restore
        // state so an interrupted restore can still roll back repeatably.
        std::fs::create_dir_all(&dir)?;
        self.profile = Some(profile.to_string());
        self.prepare_save_transition(pm, fingerprint).await?;
        SaveManager::restore(&scope, profile, &snapshot, &dir).map_err(Into::into)
    }

    pub fn commit_save_operation(&mut self) -> Result<()> {
        ensure!(
            self.phase == SessionPhase::SwitchingSaves,
            "save operation has not applied its transition"
        );
        self.advance(SessionPhase::SaveCommitted)?;
        Self::clear()
    }
}
