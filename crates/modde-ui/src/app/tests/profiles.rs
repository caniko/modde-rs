#![allow(clippy::wildcard_imports)]
use super::*;

/// Build an `EnabledMod` for the refusal-test fixtures. `lock` controls
/// the per-mod pin: `None` for a normal entry, `Some(reason)` to pin.
pub(crate) fn seed_mod(id: &str, lock: Option<LockReason>) -> modde_core::profile::EnabledMod {
    modde_core::profile::EnabledMod {
        mod_id: id.to_string(),
        display_name: Some(id.to_string()),
        enabled: true,
        lock,
        ..Default::default()
    }
}
/// Write a profile directly to the isolated DB. Profile names must be
/// unique across all tests in this module (they share the `OnceLock`
/// tempdir). The fake `game_id = "test-game"` is deliberate — it makes
/// `modde_games::resolve_game_plugin` return `None`, which in turn
/// makes `reload_profile`'s fingerprint block short-circuit, avoiding
/// a walk of the non-existent staging dir.
pub(crate) fn seed_profile(
    name: &str,
    mods: Vec<modde_core::profile::EnabledMod>,
    lock: Option<LoadOrderLock>,
) {
    reset_isolated_db();
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    let profile = modde_core::profile::Profile {
        id: None,
        name: name.to_string(),
        game_id: modde_core::GameId::from("test-game"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: SmallVec::new(),
        load_order_lock: lock,
    };
    crate::app::block_on(pm.create(&profile)).expect("seed profile");
}

pub(crate) fn profile_for_game(
    name: &str,
    game_id: &str,
    mods: Vec<modde_core::profile::EnabledMod>,
) -> modde_core::profile::Profile {
    modde_core::profile::Profile {
        id: None,
        name: name.to_string(),
        game_id: modde_core::GameId::from(game_id),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from(format!("/tmp/{name}/overrides")),
        load_order_rules: SmallVec::new(),
        load_order_lock: None,
    }
}

/// Build a `Modde` with `active_profile` set to the seeded profile
/// and `loaded_profile` populated via `reload_profile` (reading from
/// the isolated DB).
pub(crate) fn loaded_test_app(name: &str) -> Modde {
    let mut app = test_app();
    app.active_profile = Some(name.to_string());
    app.reload_profile_blocking();
    app
}

/// Read the profile back from the isolated DB. Assertions should use
/// this (not `app.loaded_profile`) to verify *persisted* state.
pub(crate) fn reload_seeded(name: &str) -> modde_core::profile::Profile {
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    crate::app::block_on(pm.load(name, Some(&GameId::from("test-game"))))
        .expect("load seeded profile")
}

/// Shorthand: return the `mod_id`s of a profile in current order.
pub(crate) fn mod_ids(profile: &modde_core::profile::Profile) -> Vec<&str> {
    profile.mods.iter().map(|m| m.mod_id.as_str()).collect()
}

pub(crate) fn complete_create_profile_write(app: &mut Modde, name: &str, game_id: &str) {
    let generation = app.context_generation;
    let profile = modde_core::Profile {
        id: None,
        name: name.to_string(),
        game_id: modde_core::GameId::from(game_id),
        source: modde_core::ProfileSource::Manual,
        mods: Vec::new(),
        overrides: PathBuf::from("overrides"),
        load_order_rules: SmallVec::new(),
        load_order_lock: None,
    };
    let result = crate::app::block_on(super::profile_ops::create_profile(app.db.clone(), profile));
    let _ = app.update(Message::ProfileWriteDone {
        generation,
        kind: ProfileWriteKind::Create {
            name: name.to_string(),
            game_id: game_id.to_string(),
        },
        result,
    });
}

pub(crate) fn complete_reorder_write(
    app: &mut Modde,
    profile_name: &str,
    mod_id: &str,
    direction: ReorderDirection,
) {
    let generation = app.context_generation;
    let result = crate::app::block_on(super::profile_ops::reorder_mod(
        app.db.clone(),
        profile_name.to_string(),
        mod_id.to_string(),
        direction,
    ));
    let _ = app.update(Message::ProfileWriteDone {
        generation,
        kind: ProfileWriteKind::Reorder {
            mod_id: mod_id.to_string(),
            direction,
        },
        result,
    });
}

pub(crate) fn complete_lock_write(app: &mut Modde, profile_name: &str, mod_id: &str, locked: bool) {
    let generation = app.context_generation;
    let result = crate::app::block_on(super::profile_ops::set_mod_lock(
        app.db.clone(),
        profile_name.to_string(),
        mod_id.to_string(),
        locked,
    ));
    let kind = if locked {
        ProfileWriteKind::Lock {
            mod_id: mod_id.to_string(),
        }
    } else {
        ProfileWriteKind::Unlock {
            mod_id: mod_id.to_string(),
        }
    };
    let _ = app.update(Message::ProfileWriteDone {
        generation,
        kind,
        result,
    });
}

pub(crate) fn complete_experiment_write(
    app: &mut Modde,
    kind: ExperimentWriteKind,
    profile_name: Option<&str>,
    game_id: &str,
) {
    let generation = app.context_generation;
    let result = crate::app::block_on(super::profile_ops::run_experiment_write(
        app.db.clone(),
        kind.clone(),
        profile_name.map(str::to_string),
        GameId::from(game_id),
        app.settings.clone(),
        app.experiment_depth,
    ));
    let _ = app.update(Message::ExperimentWriteDone {
        generation,
        kind,
        result,
    });
}

#[test]
fn select_game_filters_profiles_to_game_and_loads_active_profile() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    let _skyrim_id = crate::app::block_on(pm.create(&profile_for_game(
        "skyrim-profile",
        "skyrim-se",
        vec![seed_mod("skyrim-mod", None)],
    )))
    .expect("seed skyrim profile");
    let inactive_id = crate::app::block_on(pm.create(&profile_for_game(
        "cp-inactive",
        "cyberpunk2077",
        vec![seed_mod("inactive-mod", None)],
    )))
    .expect("seed inactive cyberpunk profile");
    let active_id = crate::app::block_on(pm.create(&profile_for_game(
        "cp-active",
        "cyberpunk2077",
        vec![seed_mod("active-mod", None)],
    )))
    .expect("seed active cyberpunk profile");
    crate::app::block_on(
        pm.db()
            .set_active_profile(&GameId::from("cyberpunk2077"), active_id),
    )
    .expect("set active cyberpunk profile");
    drop(pm);

    let mut app = test_app();
    app.settings.set_game_path(
        &GameId::from("cyberpunk2077"),
        game_dir.path().to_path_buf(),
    );
    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));
    app.finish_pending_switch_blocking();

    let profile_names: Vec<_> = app.profiles.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(profile_names, vec!["cp-active", "cp-inactive"]);
    assert_eq!(app.active_profile.as_deref(), Some("cp-active"));
    assert_eq!(
        app.loaded_profile.as_ref().map(|p| mod_ids(p)),
        Some(vec!["active-mod"])
    );
    assert_ne!(inactive_id, active_id);
}

#[test]
fn select_game_falls_back_to_first_profile_for_game() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    crate::app::block_on(pm.create(&profile_for_game(
        "zeta",
        "cyberpunk2077",
        vec![seed_mod("zeta-mod", None)],
    )))
    .expect("seed zeta profile");
    crate::app::block_on(pm.create(&profile_for_game(
        "alpha",
        "cyberpunk2077",
        vec![seed_mod("alpha-mod", None)],
    )))
    .expect("seed alpha profile");
    drop(pm);

    let mut app = test_app();
    app.settings.set_game_path(
        &GameId::from("cyberpunk2077"),
        game_dir.path().to_path_buf(),
    );
    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));
    app.finish_pending_switch_blocking();

    assert_eq!(app.active_profile.as_deref(), Some("alpha"));
    assert_eq!(
        app.loaded_profile.as_ref().map(|p| mod_ids(p)),
        Some(vec!["alpha-mod"])
    );
}

#[test]
fn select_game_with_no_profiles_clears_profile_context() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let mut app = test_app();
    app.active_profile = Some("old".to_string());
    app.loaded_profile = Some(profile_for_game("old", "skyrim-se", vec![]));
    app.settings.set_game_path(
        &GameId::from("cyberpunk2077"),
        game_dir.path().to_path_buf(),
    );

    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));
    app.finish_pending_switch_blocking();

    assert!(app.profiles.is_empty());
    assert!(app.active_profile.is_none());
    assert!(app.loaded_profile.is_none());
}

#[test]
fn profile_context_loaded_discards_stale_generation() {
    let _guard = db_lock();
    let mut app = test_app();

    // Simulate two kickoffs having advanced the generation to 2; the in-flight
    // load tagged generation 1 is now stale and must be dropped, while the
    // generation-2 load applies. This is the linchpin against a slow load for
    // game A clobbering state after the user switched to game B.
    app.context_generation = 2;

    let stale = ProfileContextSnapshot {
        profiles: Vec::new(),
        active_profile: Some("STALE".to_string()),
        profile_outcome: ProfileLoadOutcome::Cleared,
        data_tab_conflicts: Vec::new(),
        missing_store_mod_count: 0,
        tools: None,
        rerun_diagnostics: false,
    };
    let _ = app.update(Message::ProfileContextLoaded {
        generation: 1,
        result: Ok(stale),
    });
    assert!(
        app.active_profile.is_none(),
        "stale generation-1 load must not clobber state"
    );

    let fresh = ProfileContextSnapshot {
        profiles: Vec::new(),
        active_profile: Some("FRESH".to_string()),
        profile_outcome: ProfileLoadOutcome::Cleared,
        data_tab_conflicts: Vec::new(),
        missing_store_mod_count: 0,
        tools: None,
        rerun_diagnostics: false,
    };
    let _ = app.update(Message::ProfileContextLoaded {
        generation: 2,
        result: Ok(fresh),
    });
    assert_eq!(
        app.active_profile.as_deref(),
        Some("FRESH"),
        "current generation-2 load must apply"
    );
}

#[test]
fn profile_context_dispatch_invalidates_pending_data_tab_load() {
    let _guard = db_lock();
    let mut app = test_app();

    // Pretend a standalone Data-tab refresh is in flight at generation 5, and
    // its (old game's) conflicts are currently shown.
    app.data_tab_generation = 5;
    app.data_tab_conflicts = vec![("keep.esp".to_string(), vec!["keep".to_string()])];

    // Kicking off a profile-context reload must invalidate that pending Data-tab
    // load (the composite reload recomputes conflicts authoritatively). The
    // returned Task is discarded — we only assert the synchronous generation bump.
    let _ = app.reload_profile();
    assert_eq!(
        app.data_tab_generation, 6,
        "a profile-context dispatch must invalidate the pending data-tab load"
    );

    // The now-stale Data-tab result (generation 5) must be dropped, so the
    // current conflicts are retained rather than clobbered by the old game's.
    let _ = app.update(Message::DataTabConflictsLoaded {
        generation: 5,
        result: Ok(DataTabConflicts {
            conflicts: vec![("stale.esp".to_string(), vec!["stale".to_string()])],
            missing_store_mod_count: 7,
        }),
    });
    assert_eq!(
        app.data_tab_conflicts,
        vec![("keep.esp".to_string(), vec!["keep".to_string()])],
        "stale data-tab conflicts must not clobber state after a context dispatch"
    );
    assert_eq!(app.data_tab_state.missing_store_mod_count, 0);
}

#[test]
fn select_game_clears_stale_selection_state() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    crate::app::block_on(pm.create(&profile_for_game(
        "cp-profile",
        "cyberpunk2077",
        vec![seed_mod("active-mod", None)],
    )))
    .expect("seed cyberpunk profile");
    drop(pm);

    let mut app = test_app();
    app.selected_mod_index = Some(4);
    app.selected_mod_details = Some(crate::views::mod_details::ModDetailsState::loading(
        1.into(),
        "skyrimspecialedition".to_string(),
        "Old mod".to_string(),
        "1.0".to_string(),
    ));
    app.selected_save_details = Some(crate::views::save_details::SaveDetailsState {
        commit_id: "abcdef".to_string(),
        short_id: "abcdef".to_string(),
        timestamp: 0,
        profile_name: Some("old".to_string()),
        character_name: None,
        save_label: None,
        category: None,
        file_count: 0,
        file_paths: None,
        fingerprint: None,
        compatibility: None,
    });
    app.save_snapshots = vec![SaveSnapshot {
        id: "abcdef".to_string(),
        timestamp: 0,
        message: "snapshot".to_string(),
        profile_name: Some("old".to_string()),
        character_name: None,
        save_label: None,
        category: None,
        file_count: 0,
        fingerprint: None,
    }];
    app.settings.set_game_path(
        &GameId::from("cyberpunk2077"),
        game_dir.path().to_path_buf(),
    );

    let _ = app.update(Message::SelectGame("cyberpunk2077".to_string()));

    assert!(app.selected_mod_index.is_none());
    assert!(app.selected_mod_details.is_none());
    assert!(app.selected_save_details.is_none());
    assert!(app.save_snapshots.is_empty());
}

#[test]
fn game_path_dialog_selection_stores_path_and_switches_context() {
    let _guard = db_lock();
    reset_isolated_db();
    let game_dir = tempfile::tempdir().expect("game dir");
    let pm = crate::app::block_on(ProfileManager::open()).expect("open isolated DB");
    crate::app::block_on(pm.create(&profile_for_game(
        "custom-profile",
        "custom-game",
        vec![seed_mod("custom-mod", None)],
    )))
    .expect("seed custom profile");
    drop(pm);

    let mut app = test_app();
    app.game_path_dialog_open = true;
    app.pending_game_path_game_id = Some("custom-game".to_string());
    app.previous_game_before_path_dialog = Some("skyrim-se".to_string());

    let _ = app.update(Message::GamePathDialogPathSelected {
        game_id: "custom-game".to_string(),
        path: game_dir.path().to_path_buf(),
    });
    app.finish_pending_switch_blocking();

    assert!(!app.game_path_dialog_open);
    assert_eq!(app.selected_game.as_deref(), Some("custom-game"));
    assert_eq!(
        app.settings.game_path(&GameId::from("custom-game")),
        Some(&game_dir.path().to_path_buf())
    );
    assert_eq!(app.active_profile.as_deref(), Some("custom-profile"));
    assert_eq!(
        app.loaded_profile.as_ref().map(|p| mod_ids(p)),
        Some(vec!["custom-mod"])
    );
}

#[test]
fn cancel_game_path_dialog_restores_previous_game() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("custom-game".to_string());
    app.settings.selected_game = Some("custom-game".to_string());
    app.game_path_dialog_open = true;
    app.pending_game_path_game_id = Some("custom-game".to_string());
    app.previous_game_before_path_dialog = Some("skyrim-se".to_string());

    let _ = app.update(Message::CancelGamePathDialog);

    assert!(!app.game_path_dialog_open);
    assert_eq!(app.selected_game.as_deref(), Some("skyrim-se"));
    assert_eq!(app.settings.selected_game.as_deref(), Some("skyrim-se"));
}
