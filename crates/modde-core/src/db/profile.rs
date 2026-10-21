//! Profile rows, summaries, and profile state snapshots.
#![allow(clippy::wildcard_imports)]

use super::rows::*;
use super::*;

impl ModdeDb {
    pub async fn create_profile(&self, profile: &Profile) -> Result<i64> {
        let (source_type, source_data) = encode_source(&profile.source);
        let load_order_lock = encode_lock(profile.load_order_lock.as_ref());

        let mut tx = self.db.begin().await?;
        let id = tx
            .fetch_one(
                "INSERT INTO profiles (name, game_id, source_type, source_data, overrides, load_order_lock)
                 VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
                &vals![
                    profile.name.clone(),
                    &profile.game_id,
                    source_type,
                    source_data,
                    profile.overrides.to_string_lossy().to_string(),
                    load_order_lock,
                ],
                |r| r.i64(0),
            )
            .await?;

        Self::insert_mods(&mut tx, id, &profile.mods).await?;
        Self::insert_rules(&mut tx, id, &profile.load_order_rules).await?;
        Self::insert_profile_state_snapshot(&mut tx, id, profile).await?;
        tx.commit().await?;

        Ok(id)
    }

    /// Load a profile by name and `game_id`.
    pub async fn load_profile(&self, name: &str, game_id: &GameId) -> Result<Profile> {
        let row = self
            .db
            .fetch_optional(
                "SELECT id, source_type, source_data, overrides, load_order_lock FROM profiles
                 WHERE name = ? AND game_id = ?",
                &vals![name, game_id],
                |r| {
                    Ok((
                        r.i64(0)?,
                        r.string(1)?,
                        r.opt_string(2)?,
                        r.string(3)?,
                        r.opt_string(4)?,
                    ))
                },
            )
            .await?;

        let (id, source_type, source_data, overrides, load_order_lock) =
            row.ok_or_else(|| CoreError::ProfileNotFound(format!("{name} (game: {game_id})")))?;

        self.assemble_profile(
            id,
            name,
            game_id,
            &source_type,
            source_data.as_deref(),
            &overrides,
            load_order_lock.as_deref(),
        )
        .await
    }

    /// Load a profile by its database ID.
    pub async fn load_profile_by_id(&self, id: i64) -> Result<Profile> {
        let row = self
            .db
            .fetch_optional(
                "SELECT name, game_id, source_type, source_data, overrides, load_order_lock
                 FROM profiles WHERE id = ?",
                &vals![id],
                |r| {
                    Ok((
                        r.string(0)?,
                        r.string(1)?,
                        r.string(2)?,
                        r.opt_string(3)?,
                        r.string(4)?,
                        r.opt_string(5)?,
                    ))
                },
            )
            .await?;

        let (name, game_id, source_type, source_data, overrides, load_order_lock) =
            row.ok_or_else(|| CoreError::ProfileNotFound(format!("id={id}")))?;

        self.assemble_profile(
            id,
            &name,
            &GameId::from(game_id),
            &source_type,
            source_data.as_deref(),
            &overrides,
            load_order_lock.as_deref(),
        )
        .await
    }

    /// Load a profile by name only. Errors with `AmbiguousProfile` if multiple games match.
    pub async fn load_profile_by_name(&self, name: &str) -> Result<Profile> {
        let rows = self
            .db
            .fetch_all(
                "SELECT id, game_id, source_type, source_data, overrides, load_order_lock
                 FROM profiles WHERE name = ?",
                &vals![name],
                |r| {
                    Ok((
                        r.i64(0)?,
                        r.string(1)?,
                        r.string(2)?,
                        r.opt_string(3)?,
                        r.string(4)?,
                        r.opt_string(5)?,
                    ))
                },
            )
            .await?;

        match rows.len() {
            0 => Err(CoreError::ProfileNotFound(name.to_string())),
            1 => {
                let (id, game_id, source_type, source_data, overrides, load_order_lock) = &rows[0];
                self.assemble_profile(
                    *id,
                    name,
                    &GameId::from(game_id.clone()),
                    source_type,
                    source_data.as_deref(),
                    overrides,
                    load_order_lock.as_deref(),
                )
                .await
            }
            _ => {
                let games: SmallVec<[GameId; 4]> = rows
                    .iter()
                    .map(|(_, g, _, _, _, _)| GameId::from(g.clone()))
                    .collect();
                Err(CoreError::AmbiguousProfile {
                    name: name.to_string(),
                    games,
                })
            }
        }
    }

    /// Update an existing profile (identified by name + `game_id`).
    pub async fn update_profile(&self, profile: &Profile) -> Result<()> {
        let (source_type, source_data) = encode_source(&profile.source);
        let load_order_lock = encode_lock(profile.load_order_lock.as_ref());

        let profile_id = self
            .db
            .fetch_optional(
                "SELECT id FROM profiles WHERE name = ? AND game_id = ?",
                &vals![profile.name.clone(), &profile.game_id],
                |r| r.i64(0),
            )
            .await?
            .ok_or_else(|| {
                CoreError::ProfileNotFound(format!("{} (game: {})", profile.name, profile.game_id))
            })?;

        // Replacing mods and rules is delete-then-insert; a failed insert must
        // not leave the profile emptied.
        let mut tx = self.db.begin().await?;
        tx.execute(
            "UPDATE profiles SET source_type = ?, source_data = ?, overrides = ?,
                    load_order_lock = ?, updated_at = {NOW}
             WHERE id = ?",
            &vals![
                source_type,
                source_data,
                profile.overrides.to_string_lossy().to_string(),
                load_order_lock,
                profile_id,
            ],
        )
        .await?;
        tx.execute(
            "DELETE FROM profile_mods WHERE profile_id = ?",
            &vals![profile_id],
        )
        .await?;
        tx.execute(
            "DELETE FROM load_order_rules WHERE profile_id = ?",
            &vals![profile_id],
        )
        .await?;

        Self::insert_mods(&mut tx, profile_id, &profile.mods).await?;
        Self::insert_rules(&mut tx, profile_id, &profile.load_order_rules).await?;
        Self::insert_profile_state_snapshot(&mut tx, profile_id, profile).await?;
        tx.commit().await
    }

    /// Persist the profile mod state used by `modde doctor` for recent diffs.
    pub async fn record_profile_state_snapshot(
        &self,
        profile_id: i64,
        profile: &Profile,
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;
        Self::insert_profile_state_snapshot(&mut tx, profile_id, profile).await?;
        tx.commit().await
    }

    async fn insert_profile_state_snapshot(
        tx: &mut DbTx<'_>,
        profile_id: i64,
        profile: &Profile,
    ) -> Result<()> {
        let snapshot = DoctorProfileModSnapshot::from_profile(profile);
        let snapshot_json = serde_json::to_string(&snapshot).map_err(|e| {
            CoreError::Other(format!("failed to encode profile state snapshot: {e}").into())
        })?;
        tx.execute(
            "INSERT INTO profile_state_snapshots
                (profile_id, game_id, profile_name, snapshot_json)
             VALUES (?, ?, ?, ?)",
            &vals![
                profile_id,
                &profile.game_id,
                profile.name.clone(),
                snapshot_json,
            ],
        )
        .await?;
        Ok(())
    }

    /// Return recent profile state snapshots, newest first.
    pub async fn recent_profile_state_snapshots(
        &self,
        profile_id: i64,
        limit: usize,
    ) -> Result<Vec<ProfileSnapshotRow>> {
        self.db
            .fetch_all(
                "SELECT id, snapshot_json, created_at
                   FROM profile_state_snapshots
                  WHERE profile_id = ?
               ORDER BY created_at DESC, id DESC
                  LIMIT ?",
                &vals![profile_id, limit as i64],
                |r| {
                    let snapshot_json = r.string(1)?;
                    let snapshot =
                        serde_json::from_str::<Vec<DoctorProfileModSnapshot>>(&snapshot_json)
                            .map_err(|e| {
                                CoreError::Other(
                                    format!("failed to decode profile state snapshot: {e}").into(),
                                )
                            })?;
                    Ok(ProfileSnapshotRow {
                        id: r.i64(0)?,
                        snapshot,
                        created_at: r.string(2)?,
                    })
                },
            )
            .await
    }

    /// Delete a profile by name and `game_id`.
    pub async fn delete_profile(&self, name: &str, game_id: &GameId) -> Result<()> {
        let changes = self
            .db
            .execute(
                "DELETE FROM profiles WHERE name = ? AND game_id = ?",
                &vals![name, game_id],
            )
            .await?;
        if changes == 0 {
            return Err(CoreError::ProfileNotFound(format!(
                "{name} (game: {game_id})"
            )));
        }
        Ok(())
    }

    /// List profile summaries, optionally filtered by game.
    pub async fn list_profiles(&self, game_id: Option<&GameId>) -> Result<Vec<ProfileSummary>> {
        let mapper = |r: &dyn DbRow| {
            Ok(ProfileSummary {
                id: r.i64(0)?,
                name: r.string(1)?,
                game_id: GameId::from(r.string(2)?),
                source_type: r.string(3)?,
                mod_count: r.i64(4)? as usize,
            })
        };

        match game_id {
            Some(gid) => {
                self.db
                    .fetch_all(
                        "SELECT p.id, p.name, p.game_id, p.source_type,
                                (SELECT COUNT(*) FROM profile_mods WHERE profile_id = p.id) as mod_count
                         FROM profiles p WHERE p.game_id = ? ORDER BY p.name",
                        &vals![gid],
                        mapper,
                    )
                    .await
            }
            None => {
                self.db
                    .fetch_all(
                        "SELECT p.id, p.name, p.game_id, p.source_type,
                                (SELECT COUNT(*) FROM profile_mods WHERE profile_id = p.id) as mod_count
                         FROM profiles p ORDER BY p.game_id, p.name",
                        &[],
                        mapper,
                    )
                    .await
            }
        }
    }
}
