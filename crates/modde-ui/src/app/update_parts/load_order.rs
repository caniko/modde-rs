#![allow(clippy::needless_return)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::wildcard_imports)]
//! load_order update handlers.

use super::*;

impl Modde {
    pub(super) fn handle_load_order_update(&mut self, message: Message) -> Task<Message> {
        match message {
            // ── Load order ───────────────────────────────────────
            Message::ReorderMod { mod_id, direction } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                let profile_name = profile_name.clone();
                return Task::perform(
                    reorder_mod(self.db.clone(), profile_name, mod_id.clone(), direction),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Reorder {
                            mod_id: mod_id.clone(),
                            direction,
                        },
                        result,
                    },
                );
            }

            Message::LockMod { mod_id } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                let profile_name = profile_name.clone();
                self.set_status(format!("Pinning '{mod_id}'..."));
                return Task::perform(
                    set_mod_lock(self.db.clone(), profile_name, mod_id.clone(), true),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Lock {
                            mod_id: mod_id.clone(),
                        },
                        result,
                    },
                );
            }
            Message::UnlockMod { mod_id } => {
                let Some(ref profile_name) = self.active_profile else {
                    return Task::none();
                };
                self.context_generation = self.context_generation.wrapping_add(1);
                let generation = self.context_generation;
                let profile_name = profile_name.clone();
                self.set_status(format!("Unpinning '{mod_id}'..."));
                return Task::perform(
                    set_mod_lock(self.db.clone(), profile_name, mod_id.clone(), false),
                    move |result| Message::ProfileWriteDone {
                        generation,
                        kind: ProfileWriteKind::Unlock {
                            mod_id: mod_id.clone(),
                        },
                        result,
                    },
                );
            }
            _ => unreachable!("message routed to wrong update handler"),
        }
    }
}
