#![allow(clippy::wildcard_imports)]
use super::*;

// ── mutating ─────────────────────────────────────────────────

#[test]
fn profile_list_is_mutating() {
    // Pinned as `mutating` in `command_mutates_state` because
    // the dispatcher routes both List and create/delete through
    // the same `commands::profile::handle` and we don't peek
    // inside the action enum. Document that here; if we later
    // refine the classification, flip this assertion.
    assert!(command_mutates_state(&list_profiles()));
}

#[test]
fn profile_create_is_mutating() {
    assert!(command_mutates_state(&create_profile()));
}

#[test]
fn install_is_mutating() {
    assert!(command_mutates_state(&install_mod()));
}

#[test]
fn update_apply_is_mutating() {
    assert!(command_mutates_state(&update_apply()));
}

#[test]
fn instance_create_is_mutating() {
    assert!(command_mutates_state(&instance_create()));
}

#[test]
#[cfg(feature = "bethesda")]
fn loot_sort_is_mutating() {
    assert!(command_mutates_state(&loot_sort()));
}

#[test]
fn tool_apply_is_mutating() {
    assert!(command_mutates_state(&tool_apply()));
}

#[test]
fn tool_setup_is_mutating() {
    assert!(command_mutates_state(&tool_setup()));
}

#[test]
fn patcher_add_command_is_mutating() {
    assert!(command_mutates_state(&Commands::Patcher {
        action: PatcherAction::AddCommand {
            name: "stage".into(),
            profile: None,
            game: None,
            executable: PathBuf::from("/bin/true"),
            working_dir: None,
            args: Vec::new(),
            environment: Vec::new(),
            output_mod: "generated".into(),
            order: 1,
            timeout_seconds: modde_core::patcher::DEFAULT_PATCHER_TIMEOUT_SECONDS,
        },
    }));
}

#[test]
fn mod_remove_is_mutating() {
    assert!(command_mutates_state(&mod_remove()));
}

#[test]
fn wabbajack_import_archive_is_mutating() {
    assert!(command_mutates_state(&wabbajack_import()));
}

#[test]
fn wabbajack_acquire_missing_is_mutating() {
    assert!(command_mutates_state(&wabbajack_acquire_missing()));
}

#[test]
fn skill_install_is_mutating() {
    assert!(command_mutates_state(&skill_install()));
}

#[test]
fn game_add_is_mutating() {
    assert!(command_mutates_state(&game_add()));
}

#[test]
fn game_remove_is_mutating() {
    assert!(command_mutates_state(&game_remove()));
}

#[test]
fn deploy_is_mutating() {
    assert!(command_mutates_state(&Commands::Deploy {
        profile: None,
        game: None,
    }));
}

#[test]
fn hot_deploy_dry_run_is_read_only() {
    assert!(!command_mutates_state(&Commands::HotDeploy {
        profile: None,
        game: Some("cyberpunk2077".into()),
        mod_id: "cosmetic".into(),
        enable: true,
        disable: false,
        dry_run: true,
        force: false,
    }));
}

#[test]
fn hot_deploy_apply_is_mutating() {
    assert!(command_mutates_state(&Commands::HotDeploy {
        profile: None,
        game: Some("cyberpunk2077".into()),
        mod_id: "cosmetic".into(),
        enable: true,
        disable: false,
        dry_run: false,
        force: false,
    }));
}

#[test]
fn rollback_is_mutating() {
    assert!(command_mutates_state(&Commands::Rollback {
        profile: None,
        game: None,
    }));
}

#[test]
fn import_is_mutating() {
    assert!(command_mutates_state(&Commands::Import));
}

#[test]
fn stock_is_mutating() {
    assert!(command_mutates_state(&Commands::Stock {
        action: StockAction::Snapshot {
            game_id: "skyrim-se".into(),
        },
    }));
}
