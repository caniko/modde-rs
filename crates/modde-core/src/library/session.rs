use super::*;

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    /// Deployment/tools may be partial, but live saves are untouched.
    Preparing,
    /// Previous saves were captured; live replacement may be partial.
    SwitchingSaves,
    /// Save replacement and active-slot update completed; no process started.
    Ready,
    /// Write-ahead launch intent. Also the conservative state for old journals.
    #[default]
    Launching,
    Requested,
    /// No preparation has run; a store wrapper must claim this request.
    AwaitingStore,
    Running,
    Exited,
    Capturing,
    /// Saves committed; remaining performance/bisect work may be retried.
    Captured,
    /// A save-only operation committed; recovery only removes its marker.
    SaveCommitted,
}

impl SessionPhase {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Preparing => "Preparing deployment",
            Self::SwitchingSaves => "Switching saves",
            Self::Ready => "Ready to launch",
            Self::Launching => "Starting supervisor",
            Self::Requested => "Unobserved store session",
            Self::AwaitingStore => "Waiting for store wrapper",
            Self::Running => "Game session running",
            Self::Exited => "Game exited",
            Self::Capturing => "Capturing saves",
            Self::Captured => "Completing analysis",
            Self::SaveCommitted => "Save operation completed",
        }
    }

    pub fn is_preparation(self) -> bool {
        matches!(self, Self::Preparing | Self::SwitchingSaves | Self::Ready | Self::SaveCommitted | Self::AwaitingStore)
    }
}

impl PendingSession {
    /// Shared across data directories: a store handoff keeps blocking mutations
    /// after the launching CLI (and its advisory resource locks) exits.
    pub fn path() -> PathBuf { crate::paths::modde_config_dir().join("sessions/pending-session.json") }

    pub fn load() -> Result<Option<Self>> {
        if let Some(session) = Self::load_at(&Self::path())? { return Ok(Some(session)); }
        let mut legacy = Self::load_at(&crate::paths::modde_data_dir().join("pending-session.json"))?;
        if let Some(session) = &mut legacy {
            session.data_directory = Some(normalized_path(&crate::paths::modde_data_dir()));
        }
        Ok(legacy)
    }

    /// User-facing state and mutation guards include durable post-capture work.
    /// Save transitions use load() because a completion owns its own mutation
    /// lease while restoring a bisect source through a new save journal.
    pub fn load_blocking() -> Result<Option<Self>> {
        if let Some(session) = Self::load()? { return Ok(Some(session)); }
        Self::load_completion()
    }

    pub fn completion_path() -> PathBuf { crate::paths::modde_config_dir().join("sessions/pending-completion.json") }

    pub fn load_completion() -> Result<Option<Self>> {
        let session = Self::load_at(&Self::completion_path())?;
        if session.as_ref().is_some_and(|session| session.phase != SessionPhase::Captured) {
            bail!("invalid completion receipt: saves have not committed");
        }
        Ok(session)
    }

    /// Write before clearing the launch journal, so a crash cannot lose the
    /// continuation between save capture and performance/bisect completion.
    pub fn save_completion(&self) -> Result<()> {
        self.require_owner()?;
        if self.phase != SessionPhase::Captured { bail!("capture must finish before completing analysis"); }
        atomic_json(&Self::completion_path(), self)
    }

    pub fn clear_completion() -> Result<()> {
        if let Some(session) = Self::load_completion()? { session.require_owner()?; }
        let path = Self::completion_path();
        match std::fs::remove_file(&path) {
            Ok(()) => {
                #[cfg(unix)]
                if let Some(parent) = path.parent() { File::open(parent)?.sync_all()?; }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("clearing completed analysis"),
        }
        Ok(())
    }

    fn load_at(path: &Path) -> Result<Option<Self>> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes).context("invalid pending session; recovery required")?)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn require_owner(&self) -> Result<()> {
        if let Some(directory) = &self.data_directory
            && normalized_path(directory) != normalized_path(&crate::paths::modde_data_dir()) {
            bail!("session belongs to {}; use modde --data-dir {} library finish (or recover for interrupted preparation)", directory.display(), directory.display());
        }
        Ok(())
    }

    pub fn save(&self) -> Result<()> {
        self.require_owner()?;
        let mut session = self.clone();
        session.data_directory = Some(normalized_path(&crate::paths::modde_data_dir()));
        atomic_json(&Self::path(), &session)
    }

    pub fn advance(&mut self, phase: SessionPhase) -> Result<()> {
        self.phase = phase;
        self.save()
    }

    /// Repeatable save recovery. The previous vault was captured before the
    /// SwitchingSaves marker; never capture potentially partial live data here.
    /// The caller holds the mutation lease and clears the journal on success.
    pub async fn restore_preparation(&self, pm: &crate::profile::ProfileManager) -> Result<()> {
        self.require_owner()?;
        if !self.phase.is_preparation() { bail!("a game may have started; finish the session after it exits"); }
        if self.phase == SessionPhase::SaveCommitted { return Ok(()); }
        if !self.switched_profile { return Ok(()); }
        let scope = crate::GameId::from(self.scope.as_str());
        if let Some((id, name)) = &self.previous_profile {
            let previous = pm.db().load_profile_by_id(*id).await?;
            if previous.name != *name || self.game_id.as_deref() != Some(previous.game_id.as_str()) {
                bail!("previous profile no longer matches the preparation journal");
            }
        }
        if let Some(transition) = &self.save_transition {
            for id in &transition.experiments {
                let profile = pm.db().load_profile_by_id(*id).await?;
                if self.game_id.as_deref() != Some(profile.game_id.as_str()) {
                    bail!("experiment profile no longer matches the preparation journal");
                }
            }
        }
        if let Some(dir) = &self.save_directory {
            if let Some((_, previous)) = &self.previous_profile {
                if let Some(snapshot) = self.save_transition.as_ref().and_then(|transition| transition.previous_snapshot.as_deref()) {
                    crate::save::SaveManager::restore(&scope, previous, snapshot, dir)?;
                } else {
                    crate::save::SaveManager::new(pm.db()).deploy(&scope, previous, dir)?;
                }
            } else {
                crate::save::SaveManager::clear_live_saves(dir)?;
            }
        }
        if let Some((id, _)) = self.previous_profile.as_ref() { pm.db().set_active_profile(&scope, *id).await?; }
        else { pm.db().clear_active_profile(&scope).await?; }
        if let Some(transition) = &self.save_transition {
            pm.db().replace_experiment_profiles(&scope, &transition.experiments).await?;
        }
        Ok(())
    }

    pub fn clear() -> Result<()> {
        if let Some(session) = Self::load()? { session.require_owner()?; }
        // Remove the old local marker first; a crash must leave the shared
        // marker in place rather than reveal a stale journal on next load.
        for path in [crate::paths::modde_data_dir().join("pending-session.json"), Self::path()] {
            match std::fs::remove_file(&path) {
                Ok(()) => {
                    #[cfg(unix)]
                    if let Some(parent) = path.parent() { File::open(parent)?.sync_all()?; }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("clearing completed session"),
            }
        }
        Ok(())
    }
}
