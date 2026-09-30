#![allow(clippy::wildcard_imports)]
use super::*;

// ── read-only ────────────────────────────────────────────────

#[test]
fn detect_is_read_only() {
    assert!(!command_mutates_state(&Commands::Detect));
}

#[test]
fn diagnostics_is_read_only() {
    assert!(!command_mutates_state(&Commands::Diagnostics {
        game: "skyrim-se".into(),
        profile: None,
    }));
}

#[test]
fn export_is_read_only() {
    assert!(!command_mutates_state(&Commands::Export {
        profile: None,
        game: None,
        columns: None,
        output: None,
    }));
}

#[test]
fn verify_is_read_only() {
    assert!(!command_mutates_state(&Commands::Verify {
        profile: None,
        game: None,
    }));
}

#[test]
fn collisions_is_read_only() {
    assert!(!command_mutates_state(&Commands::Collisions {
        profile: None,
        game: None,
        all: false,
        suggest_hides: false,
    }));
}

#[test]
fn gui_is_read_only() {
    // The GUI command launches the GUI itself; pushing to
    // ourselves at startup would be confusing and pointless.
    #[cfg(feature = "gui")]
    assert!(!command_mutates_state(&Commands::Gui));
}

#[test]
fn update_check_is_read_only() {
    assert!(!command_mutates_state(&update_check()));
}

#[test]
fn instance_list_is_read_only() {
    assert!(!command_mutates_state(&instance_list()));
}

#[test]
#[cfg(feature = "bethesda")]
fn loot_validate_is_read_only() {
    assert!(!command_mutates_state(&loot_validate()));
}

#[test]
fn tool_list_is_read_only() {
    assert!(!command_mutates_state(&tool_list()));
}

#[test]
fn tool_show_is_read_only() {
    assert!(!command_mutates_state(&tool_show()));
}

#[test]
fn tool_diagnose_is_read_only() {
    assert!(!command_mutates_state(&tool_diagnose()));
}

#[test]
fn tool_doctor_is_read_only() {
    assert!(!command_mutates_state(&tool_doctor()));
}

#[test]
fn tool_settings_is_read_only() {
    assert!(!command_mutates_state(&tool_settings()));
}

#[test]
fn tool_profiles_is_read_only() {
    assert!(!command_mutates_state(&tool_profiles()));
}

#[test]
fn tool_sources_is_read_only() {
    assert!(!command_mutates_state(&tool_sources()));
}

#[test]
fn patcher_list_is_read_only() {
    assert!(!command_mutates_state(&patcher_list()));
}

#[test]
fn patcher_validate_is_read_only() {
    assert!(!command_mutates_state(&patcher_validate()));
}

#[test]
fn mod_diagnose_is_read_only() {
    assert!(!command_mutates_state(&mod_diagnose()));
}

#[test]
fn wabbajack_search_is_read_only() {
    assert!(!command_mutates_state(&wabbajack_search()));
}

#[test]
fn wabbajack_assess_is_read_only() {
    assert!(!command_mutates_state(&wabbajack_assess()));
}

#[test]
fn skill_list_is_read_only() {
    assert!(!command_mutates_state(&skill_list()));
}

#[test]
fn game_list_is_read_only() {
    assert!(!command_mutates_state(&Commands::Game {
        action: GameAction::List,
    }));
}

#[test]
fn game_detect_is_read_only() {
    assert!(!command_mutates_state(&Commands::Game {
        action: GameAction::Detect {
            install_path: PathBuf::from("/games"),
        },
    }));
}

#[test]
fn game_show_is_read_only() {
    assert!(!command_mutates_state(&Commands::Game {
        action: GameAction::Show {
            id: "custom-game".into(),
        },
    }));
}

#[test]
fn game_export_is_read_only() {
    assert!(!command_mutates_state(&Commands::Game {
        action: GameAction::Export {
            id: "custom-game".into(),
            with_optiscaler: false,
            output: None,
        },
    }));
}
