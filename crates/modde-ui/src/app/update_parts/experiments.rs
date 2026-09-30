#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! experiments update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_experiments_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Experiments ──────────────────────────────────────
            Message::TryProfile => {
                if let (Some(profile), Some(profile_name)) =
                    (&self.loaded_profile, &self.active_profile)
                {
                    let game_id = profile.game_id.clone();
                    let name = profile_name.clone();
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let current_depth = self.experiment_depth;
                    self.status_message = "Starting experiment...".to_string();
                    return Task::perform(
                        run_experiment_write(
                            self.db.clone(),
                            ExperimentWriteKind::Try,
                            Some(name),
                            game_id,
                            self.settings.clone(),
                            current_depth,
                        ),
                        move |result| Message::ExperimentWriteDone {
                            generation,
                            kind: ExperimentWriteKind::Try,
                            result,
                        },
                    );
                }
            }
            Message::RollbackExperiment => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let current_depth = self.experiment_depth;
                    self.status_message = "Rolling back experiment...".to_string();
                    return Task::perform(
                        run_experiment_write(
                            self.db.clone(),
                            ExperimentWriteKind::Rollback,
                            None,
                            game_id,
                            self.settings.clone(),
                            current_depth,
                        ),
                        move |result| Message::ExperimentWriteDone {
                            generation,
                            kind: ExperimentWriteKind::Rollback,
                            result,
                        },
                    );
                }
            }
            Message::CommitExperiment => {
                if let Some(ref profile) = self.loaded_profile {
                    let game_id = profile.game_id.clone();
                    self.context_generation = self.context_generation.wrapping_add(1);
                    let generation = self.context_generation;
                    let current_depth = self.experiment_depth;
                    self.status_message = "Committing experiment...".to_string();
                    return Task::perform(
                        run_experiment_write(
                            self.db.clone(),
                            ExperimentWriteKind::Commit,
                            None,
                            game_id,
                            self.settings.clone(),
                            current_depth,
                        ),
                        move |result| Message::ExperimentWriteDone {
                            generation,
                            kind: ExperimentWriteKind::Commit,
                            result,
                        },
                    );
                }
            }
            Message::ExperimentWriteDone {
                generation,
                kind,
                result,
            } => {
                if generation != self.context_generation {
                    return Task::none();
                }
                match result {
                    Ok(outcome) => {
                        if let Some(previous_profile) = outcome.previous_profile {
                            self.active_profile = Some(previous_profile);
                        }
                        if matches!(kind, ExperimentWriteKind::Try) {
                            self.experiment_depth = self.experiment_depth.saturating_add(1);
                        } else if matches!(kind, ExperimentWriteKind::Commit) {
                            self.experiment_depth = 0;
                        }
                        self.status_message = outcome.status_message;
                        if outcome.reload {
                            return self.reload_profile();
                        }
                    }
                    Err(err) => {
                        self.status_message = match kind {
                            ExperimentWriteKind::Try => format!("Try failed: {err}"),
                            ExperimentWriteKind::Rollback => format!("Rollback failed: {err}"),
                            ExperimentWriteKind::Commit => format!("Commit failed: {err}"),
                        };
                    }
                }
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
