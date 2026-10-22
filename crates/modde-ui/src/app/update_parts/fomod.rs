#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! fomod update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_fomod_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── FOMOD ────────────────────────────────────────────
            Message::StartFOMOD {
                mod_path,
                dest_path,
            } => {
                let config_path = mod_path.join("fomod").join("ModuleConfig.xml");
                let xml = match std::fs::read_to_string(&config_path) {
                    Ok(xml) => xml,
                    Err(e) => {
                        self.set_error(format!("Failed to read ModuleConfig.xml: {e}"));
                        return Task::none();
                    }
                };
                let config = match fomod_oxide::ModuleConfig::parse(&xml) {
                    Ok(c) => c,
                    Err(e) => {
                        self.set_error(format!("Failed to parse ModuleConfig.xml: {e}"));
                        return Task::none();
                    }
                };
                let installer = fomod_oxide::Installer::new(config);
                let mut state = FOMODWizardState::with_installer(installer);
                let defaults = state.default_selections();
                self.fomod_selections.clear();
                for (step_idx, group_idx, sel) in defaults {
                    state.select(step_idx, group_idx, sel.clone());
                    self.fomod_selections.insert((step_idx, group_idx), sel);
                }
                self.fomod_installer = Some(state);
                self.fomod_source_dir = Some(mod_path);
                self.fomod_dest_dir = Some(dest_path);
                self.fomod_wizard_pos = 0;
                self.fomod_can_undo = false;
                self.refresh_fomod_visible_steps();
                self.refresh_fomod_conflicts();
                self.active_view = View::FOMODWizard(FOMODWizardState::new());
                self.set_status("FOMOD wizard started".to_string());
            }
            Message::FOMODChoice {
                step,
                group,
                option,
                selected,
            } => {
                if let Some(ref mut installer) = self.fomod_installer {
                    installer.checkpoint();
                    self.fomod_can_undo = true;
                    let group_type = installer.group_type_at(step, group);
                    let entry = self.fomod_selections.entry((step, group)).or_default();
                    match group_type {
                        Some(
                            fomod_oxide::config::GroupType::SelectExactlyOne
                            | fomod_oxide::config::GroupType::SelectAtMostOne,
                        ) => {
                            if selected {
                                *entry = vec![option];
                            } else {
                                entry.retain(|&o| o != option);
                            }
                        }
                        Some(fomod_oxide::config::GroupType::SelectAll) => {}
                        _ => {
                            if selected {
                                if !entry.contains(&option) {
                                    entry.push(option);
                                }
                            } else {
                                entry.retain(|&o| o != option);
                            }
                        }
                    }
                    let current_sel = entry.clone();
                    installer.select(step, group, current_sel);
                    self.refresh_fomod_visible_steps();
                    self.refresh_fomod_conflicts();
                }
            }
            Message::FOMODNext => {
                if self.fomod_is_last_step() {
                    let result = (|| -> Result<(), String> {
                        let installer = self
                            .fomod_installer
                            .as_ref()
                            .ok_or("No active FOMOD installer")?;
                        let source = self
                            .fomod_source_dir
                            .as_ref()
                            .ok_or("No source directory")?;
                        let dest = self
                            .fomod_dest_dir
                            .as_ref()
                            .ok_or("No destination directory")?;
                        let plan = installer.resolve();
                        plan.execute(source, dest).map_err(|e| e.to_string())?;
                        Ok(())
                    })();
                    match &result {
                        Ok(()) => {
                            self.set_status(
                                "FOMOD installation completed successfully".to_string(),
                            );
                        }
                        Err(e) => self.set_error(format!("FOMOD installation failed: {e}")),
                    }
                    self.reset_fomod();
                    self.active_view = View::ModList;
                    return Task::done(Message::FOMODInstallComplete(result));
                }
                if let Some(ref mut installer) = self.fomod_installer {
                    installer.checkpoint();
                    self.fomod_can_undo = true;
                }
                self.fomod_wizard_pos += 1;
            }
            Message::FOMODBack => {
                if self.fomod_wizard_pos > 0 {
                    self.fomod_wizard_pos -= 1;
                }
            }
            Message::FOMODCancel => {
                self.reset_fomod();
                self.active_view = View::ModList;
                self.set_status("FOMOD installation cancelled".to_string());
            }
            Message::FOMODUndo => {
                let rolled_back = self
                    .fomod_installer
                    .as_mut()
                    .is_some_and(FOMODWizardState::rollback);
                if rolled_back {
                    if let Some(ref installer) = self.fomod_installer {
                        self.fomod_selections = installer.selections();
                        self.fomod_can_undo = installer.history_len() > 0;
                    }
                    self.refresh_fomod_visible_steps();
                    self.refresh_fomod_conflicts();
                    self.set_status("Undid last FOMOD selection".to_string());
                }
            }
            Message::FOMODInstallComplete(result) => match result {
                Ok(()) => self.set_status("FOMOD installation complete!".to_string()),
                Err(e) => self.set_error(format!("FOMOD installation failed: {e}")),
            },
            _ => unreachable!("message routed to wrong update handler"),
        }
        Task::none()
    }
}
