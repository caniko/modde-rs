#![allow(clippy::wildcard_imports)]
//! Tests for [`command_mutates_state`].
//!
//! Goal: lock in which commands push a refresh signal to running
//! GUIs. A wrong classification has user-visible consequences:
//!
//! * False negative (mutating cmd marked read-only) → GUI misses
//!   an update; user sees stale state until they click around.
//! * False positive (read-only cmd marked mutating) → harmless
//!   socket round-trip per active GUI, but adds noise.
//!
//! When a new top-level [`Commands`] variant is added, the
//! exhaustive `match` in `command_mutates_state` will not compile
//! until you classify it — at which point a test in this module
//! should pin the answer.
use super::*;
use std::path::PathBuf;

fn list_profiles() -> Commands {
    Commands::Profile {
        action: ProfileAction::List { game: None },
    }
}

fn create_profile() -> Commands {
    Commands::Profile {
        action: ProfileAction::Create {
            name: "p".into(),
            game: "skyrim-se".into(),
        },
    }
}

fn install_mod() -> Commands {
    Commands::Install {
        source: InstallSource::Mod {
            url: "https://www.nexusmods.com/skyrimspecialedition/mods/1".into(),
            profile: None,
            fomod_config: None,
        },
    }
}

fn update_check() -> Commands {
    Commands::Update {
        action: UpdateAction::Check {
            profile: None,
            game: None,
            period: "1w".into(),
            mods: false,
        },
    }
}

fn update_apply() -> Commands {
    Commands::Update {
        action: UpdateAction::Apply {
            profile: None,
            game: None,
            period: "1w".into(),
            dry_run: false,
            confirm_locked: false,
            accept_breaking: false,
            yes: false,
        },
    }
}

fn instance_list() -> Commands {
    Commands::Instance {
        action: InstanceAction::List,
    }
}

fn instance_create() -> Commands {
    Commands::Instance {
        action: InstanceAction::Create {
            name: "x".into(),
            data_dir: PathBuf::from("/tmp/x"),
        },
    }
}

#[cfg(feature = "bethesda")]
fn loot_validate() -> Commands {
    Commands::Loot {
        action: LootAction::Validate {
            game: "skyrim-se".into(),
        },
    }
}

#[cfg(feature = "bethesda")]
fn loot_sort() -> Commands {
    Commands::Loot {
        action: LootAction::Sort {
            game: "skyrim-se".into(),
            data_dir: None,
        },
    }
}

fn tool_list() -> Commands {
    Commands::Tool {
        action: ToolAction::List {
            game: "skyrim-se".into(),
        },
    }
}

fn tool_show() -> Commands {
    Commands::Tool {
        action: ToolAction::Show {
            tool_id: "optiscaler".into(),
            game: "stellar-blade".into(),
            json: false,
        },
    }
}

fn tool_diagnose() -> Commands {
    Commands::Tool {
        action: ToolAction::Diagnose {
            tool_id: "optiscaler".into(),
            game: "stellar-blade".into(),
            json: false,
        },
    }
}

fn tool_doctor() -> Commands {
    Commands::Tool {
        action: ToolAction::Doctor {
            tool_id: Some("optiscaler".into()),
            game: "stellar-blade".into(),
            json: false,
            fix: false,
        },
    }
}

fn tool_settings() -> Commands {
    Commands::Tool {
        action: ToolAction::Settings {
            tool_id: "optiscaler".into(),
            game: "stellar-blade".into(),
            json: false,
        },
    }
}

fn tool_profiles() -> Commands {
    Commands::Tool {
        action: ToolAction::Profiles {
            tool_id: "optiscaler".into(),
            game: "stellar-blade".into(),
            json: false,
        },
    }
}

fn tool_sources() -> Commands {
    Commands::Tool {
        action: ToolAction::Sources {
            tool_id: "optiscaler".into(),
            game: "stellar-blade".into(),
            json: false,
        },
    }
}

fn patcher_list() -> Commands {
    Commands::Patcher {
        action: PatcherAction::List {
            profile: None,
            game: None,
        },
    }
}

fn patcher_validate() -> Commands {
    Commands::Patcher {
        action: PatcherAction::Validate {
            profile: None,
            game: None,
        },
    }
}

fn tool_apply() -> Commands {
    Commands::Tool {
        action: ToolAction::Apply {
            tool_id: "mangohud".into(),
            game: "skyrim-se".into(),
            dry_run: false,
        },
    }
}

fn tool_setup() -> Commands {
    Commands::Tool {
        action: ToolAction::Setup {
            tool_id: "optiscaler".into(),
            game: "stellar-blade".into(),
            profile: None,
            source: "auto".into(),
            release_tag: None,
            release_asset: None,
            local_source_dir: None,
            hardware_tuning: "auto".into(),
            upgrade: false,
            apply: false,
            dry_run: false,
            yes: false,
        },
    }
}

fn mod_remove() -> Commands {
    Commands::Mod {
        action: ModAction::Remove {
            mod_id: "x".into(),
            profile: None,
            dry_run: false,
            force_contaminate: false,
        },
    }
}

fn mod_diagnose() -> Commands {
    Commands::Mod {
        action: ModAction::Diagnose { mod_id: "x".into() },
    }
}

fn wabbajack_search() -> Commands {
    Commands::Wabbajack {
        action: WabbajackAction::Search {
            query: None,
            game: None,
            source: "both".into(),
            json: false,
        },
    }
}

fn wabbajack_import() -> Commands {
    Commands::Wabbajack {
        action: WabbajackAction::ImportArchive {
            manifest: PathBuf::from("m.json"),
            archives: vec![],
        },
    }
}

fn wabbajack_acquire_missing() -> Commands {
    Commands::Wabbajack {
        action: WabbajackAction::AcquireMissing {
            manifest: PathBuf::from("m.json"),
            download_dir: None,
            data_dir: None,
            browser_profile: None,
            include_nexus: false,
            browser_controller: false,
            timeout: 1,
            json: false,
        },
    }
}

fn wabbajack_assess() -> Commands {
    Commands::Wabbajack {
        action: WabbajackAction::Assess {
            manifest: PathBuf::from("list.wabbajack"),
            profile: None,
            game_dir: None,
            json: false,
        },
    }
}

fn skill_list() -> Commands {
    Commands::Skill {
        action: SkillAction::List,
    }
}

fn skill_install() -> Commands {
    Commands::Skill {
        action: SkillAction::Install {
            name: "all".into(),
            force: false,
        },
    }
}

fn game_add() -> Commands {
    Commands::Game {
        action: GameAction::Add {
            id: "custom-game".into(),
            display_name: "Custom Game".into(),
            executable_dir: PathBuf::from("bin"),
            steam_app_id: None,
            install_dir_name: None,
            mod_dir: None,
            nexus_domain: None,
            proxy_dlls: vec![],
            force: false,
        },
    }
}

fn game_remove() -> Commands {
    Commands::Game {
        action: GameAction::Remove {
            id: "custom-game".into(),
            yes: false,
        },
    }
}

mod mutating;
mod read_only;
