//! Bisect sessions and TOML profile import helpers.
#![allow(clippy::wildcard_imports)]

use super::rows::*;
use super::*;

impl ModdeDb {
    pub async fn create_bisect_session(&self, session: &NewBisectSession) -> Result<()> {
        self.db
            .execute(
                "INSERT INTO bisect_sessions
                    (session_id, game_id, source_profile_id, source_profile_name, oracle_json,
                     status, suspect_mod_ids_json, known_good_mod_ids_json,
                     known_bad_mod_ids_json, save_safety, keep_profiles)
                 VALUES (?, ?, ?, ?, ?, ?, ?, '[]', '[]', ?, ?)",
                &vals![
                    session.session_id.clone(),
                    &session.game_id,
                    session.source_profile_id,
                    session.source_profile_name.clone(),
                    encode_json(&session.oracle, "bisect oracle")?,
                    BisectStatus::Active.as_str(),
                    encode_json(&session.suspect_mod_ids, "bisect suspect mod ids")?,
                    match session.save_safety {
                        BisectSaveSafety::Refuse => "refuse",
                        BisectSaveSafety::Force => "force",
                    },
                    session.keep_profiles,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn load_bisect_session(&self, session_id: &str) -> Result<BisectSession> {
        self.db
            .fetch_optional(
                "SELECT session_id, game_id, source_profile_id, source_profile_name,
                        oracle_json, status, suspect_mod_ids_json,
                        known_good_mod_ids_json, known_bad_mod_ids_json,
                        current_step_id, current_candidate_profile, save_safety,
                        keep_profiles, created_at, updated_at
                   FROM bisect_sessions WHERE session_id = ?",
                &vals![session_id],
                bisect_session_from_row,
            )
            .await?
            .ok_or_else(|| {
                CoreError::Other(format!("bisect session not found: {session_id}").into())
            })
    }

    pub async fn update_bisect_session_state(
        &self,
        session_id: &str,
        status: BisectStatus,
        suspect_mod_ids: &[String],
        known_good_mod_ids: &[String],
        known_bad_mod_ids: &[String],
        current_step_id: Option<i64>,
        current_candidate_profile: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE bisect_sessions SET
                    status = ?,
                    suspect_mod_ids_json = ?,
                    known_good_mod_ids_json = ?,
                    known_bad_mod_ids_json = ?,
                    current_step_id = ?,
                    current_candidate_profile = ?,
                    updated_at = {NOW}
                 WHERE session_id = ?",
                &vals![
                    status.as_str(),
                    encode_json(suspect_mod_ids, "bisect suspect mod ids")?,
                    encode_json(known_good_mod_ids, "bisect known good mod ids")?,
                    encode_json(known_bad_mod_ids, "bisect known bad mod ids")?,
                    current_step_id,
                    current_candidate_profile.map(str::to_string),
                    session_id,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn create_bisect_step(&self, step: &NewBisectStep) -> Result<i64> {
        let id = self
            .db
            .fetch_one(
                "INSERT INTO bisect_steps
                    (session_id, step_index, candidate_profile, candidate_mod_ids_json,
                     enabled_mod_ids_json, disabled_mod_ids_json)
                 VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
                &vals![
                    step.session_id.clone(),
                    step.step_index as i64,
                    step.candidate_profile.clone(),
                    encode_json(&step.candidate_mod_ids, "bisect candidate mod ids")?,
                    encode_json(&step.enabled_mod_ids, "bisect enabled mod ids")?,
                    encode_json(&step.disabled_mod_ids, "bisect disabled mod ids")?,
                ],
                |r| r.i64(0),
            )
            .await?;
        Ok(id)
    }

    pub async fn load_bisect_step(&self, id: i64) -> Result<BisectStep> {
        self.db
            .fetch_optional(
                "SELECT id, session_id, step_index, candidate_profile,
                        candidate_mod_ids_json, enabled_mod_ids_json, disabled_mod_ids_json,
                        result, observed_signal, notes, launched_at
                   FROM bisect_steps WHERE id = ?",
                &vals![id],
                bisect_step_from_row,
            )
            .await?
            .ok_or_else(|| CoreError::Other(format!("bisect step not found: {id}").into()))
    }

    pub async fn list_bisect_steps(&self, session_id: &str) -> Result<Vec<BisectStep>> {
        self.db
            .fetch_all(
                "SELECT id, session_id, step_index, candidate_profile,
                        candidate_mod_ids_json, enabled_mod_ids_json, disabled_mod_ids_json,
                        result, observed_signal, notes, launched_at
                   FROM bisect_steps WHERE session_id = ? ORDER BY step_index",
                &vals![session_id],
                bisect_step_from_row,
            )
            .await
    }

    pub async fn complete_bisect_step(
        &self,
        id: i64,
        result: BisectResult,
        observed_signal: Option<&str>,
        notes: Option<&str>,
    ) -> Result<()> {
        self.db
            .execute(
                "UPDATE bisect_steps SET result = ?, observed_signal = ?, notes = ? WHERE id = ?",
                &vals![
                    result.as_str(),
                    observed_signal.map(str::to_string),
                    notes.map(str::to_string),
                    id,
                ],
            )
            .await?;
        Ok(())
    }

    pub async fn bisect_candidate_profiles(&self, session_id: &str) -> Result<Vec<String>> {
        self.db
            .fetch_all(
                "SELECT candidate_profile FROM bisect_steps WHERE session_id = ? ORDER BY step_index",
                &vals![session_id],
                |r| r.string(0),
            )
            .await
    }

    // ── TOML Import ───────────────────────────────────────────────

    /// Import existing TOML profile files into the database.
    /// Returns the number of profiles imported.
    pub async fn import_toml_profiles(&self, profiles_dir: &Path) -> Result<usize> {
        if !profiles_dir.exists() {
            return Ok(0);
        }

        let mut count = 0usize;

        for entry in std::fs::read_dir(profiles_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }

            let toml_path = entry.path().join("profile.toml");
            if !toml_path.exists() {
                continue;
            }

            let content = match std::fs::read_to_string(&toml_path) {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(path = %toml_path.display(), error = %e, "skipping unreadable profile");
                    continue;
                }
            };

            #[allow(deprecated)]
            let mut profile: Profile = match toml::from_str(&content) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(path = %toml_path.display(), error = %e, "skipping unparseable profile");
                    continue;
                }
            };
            if let Err(e) = crate::profile::validate_profile_name(&profile.name) {
                tracing::warn!(path = %toml_path.display(), error = %e, "skipping profile with invalid name");
                continue;
            }

            if profile.load_order_lock.is_none() {
                profile.load_order_lock = Some(LoadOrderLock::now(LockReason::TomlImport {
                    source_path: toml_path.display().to_string(),
                }));
            }

            let exists = self
                .db
                .fetch_one(
                    "SELECT COUNT(*) FROM profiles WHERE name = ? AND game_id = ?",
                    &vals![profile.name.clone(), &profile.game_id],
                    |r| r.i64(0),
                )
                .await?
                > 0;

            if exists {
                tracing::debug!(name = %profile.name, game = %profile.game_id, "profile already in DB, skipping");
                continue;
            }

            self.create_profile(&profile).await?;
            tracing::info!(name = %profile.name, game = %profile.game_id, "imported TOML profile");
            count += 1;
        }

        Ok(count)
    }

    // ── Internal helpers ──────────────────────────────────────────

    pub(super) async fn insert_mods(&self, profile_id: i64, mods: &[EnabledMod]) -> Result<()> {
        for (idx, m) in mods.iter().enumerate() {
            let lock_reason = encode_lock_reason(m.lock.as_ref());
            let nexus_mod_id = m.nexus_mod_id.map(NexusModId::to_i64).transpose()?;
            let nexus_file_id = m.nexus_file_id.map(NexusFileId::to_i64).transpose()?;
            let tags = encode_tags(&m.tags)?;
            let install_method = m
                .install_method
                .as_ref()
                .map(encode_install_method)
                .transpose()?;
            let install_status = m.install_status.map(InstallStatus::as_str);

            self.db
                .execute(
                    "INSERT INTO profile_mods (profile_id, mod_id, display_name, enabled, version, fomod_config, sort_index,
                            nexus_mod_id, nexus_file_id, nexus_game_domain, installed_timestamp,
                            category_id, notes, tags, lock_reason,
                            install_method, source_archive_hash, install_status)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    &vals![
                        profile_id,
                        m.mod_id.clone(),
                        m.display_name.clone(),
                        m.enabled,
                        m.version.clone(),
                        m.fomod_config.clone(),
                        idx as i64,
                        nexus_mod_id,
                        nexus_file_id,
                        m.nexus_game_domain.clone(),
                        m.installed_timestamp,
                        m.category_id,
                        m.notes.clone(),
                        tags,
                        lock_reason,
                        install_method,
                        m.source_archive_hash.clone(),
                        install_status,
                    ],
                )
                .await?;
        }
        Ok(())
    }

    pub(super) async fn insert_rules(
        &self,
        profile_id: i64,
        rules: &[LoadOrderRule],
    ) -> Result<()> {
        for rule in rules {
            let (rule_type, mod_a, mod_b) = match rule {
                LoadOrderRule::LoadAfter { mod_id, after } => {
                    ("load_after", mod_id.as_str(), after.as_str())
                }
                LoadOrderRule::LoadBefore { mod_id, before } => {
                    ("load_before", mod_id.as_str(), before.as_str())
                }
                LoadOrderRule::Incompatible { mod_a, mod_b } => {
                    ("incompatible", mod_a.as_str(), mod_b.as_str())
                }
            };
            self.db
                .execute(
                    "INSERT INTO load_order_rules (profile_id, rule_type, mod_a, mod_b)
                     VALUES (?, ?, ?, ?)",
                    &vals![profile_id, rule_type, mod_a, mod_b],
                )
                .await?;
        }
        Ok(())
    }

    async fn load_mods(&self, profile_id: i64) -> Result<Vec<EnabledMod>> {
        self.db
            .fetch_all(
                "SELECT mod_id, display_name, enabled, version, fomod_config,
                        nexus_mod_id, nexus_file_id, nexus_game_domain, installed_timestamp,
                        category_id, notes, tags, lock_reason,
                        install_method, source_archive_hash, install_status
                 FROM profile_mods WHERE profile_id = ? ORDER BY sort_index",
                &vals![profile_id],
                map_enabled_mod,
            )
            .await
    }

    async fn load_rules(&self, profile_id: i64) -> Result<SmallVec<[LoadOrderRule; 4]>> {
        let raw = self
            .db
            .fetch_all(
                "SELECT rule_type, mod_a, mod_b FROM load_order_rules WHERE profile_id = ?",
                &vals![profile_id],
                |r| Ok((r.string(0)?, r.string(1)?, r.string(2)?)),
            )
            .await?;

        let mut result = SmallVec::with_capacity(raw.len());
        for (rule_type, mod_a, mod_b) in raw {
            let rule = match rule_type.as_str() {
                "load_after" => LoadOrderRule::LoadAfter {
                    mod_id: ModId::from(mod_a),
                    after: ModId::from(mod_b),
                },
                "load_before" => LoadOrderRule::LoadBefore {
                    mod_id: ModId::from(mod_a),
                    before: ModId::from(mod_b),
                },
                "incompatible" => LoadOrderRule::Incompatible {
                    mod_a: ModId::from(mod_a),
                    mod_b: ModId::from(mod_b),
                },
                other => {
                    tracing::warn!(rule_type = other, "unknown load order rule type, skipping");
                    continue;
                }
            };
            result.push(rule);
        }

        Ok(result)
    }

    pub(super) async fn assemble_profile(
        &self,
        id: i64,
        name: &str,
        game_id: &GameId,
        source_type: &str,
        source_data: Option<&str>,
        overrides: &str,
        load_order_lock_raw: Option<&str>,
    ) -> Result<Profile> {
        let source = decode_source(source_type, source_data)?;
        let mods = self.load_mods(id).await?;
        let load_order_rules = self.load_rules(id).await?;
        let load_order_lock = decode_lock(load_order_lock_raw)?;

        Ok(Profile {
            id: Some(id),
            name: name.to_string(),
            game_id: game_id.clone(),
            source,
            mods,
            overrides: PathBuf::from(overrides),
            load_order_rules,
            load_order_lock,
        })
    }
}
