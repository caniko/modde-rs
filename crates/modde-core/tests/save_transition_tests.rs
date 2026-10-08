//! Crash boundaries must restore immutable saves and the whole experiment stack.
use modde_core::library::{PendingSession, SaveContext, SessionPhase};
use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::save::SaveManager;
use modde_core::{GameId, ModdeDb};

#[tokio::test]
async fn interrupted_replacement_restores_pinned_saves_and_state_until_committed() {
    let root = tempfile::tempdir().unwrap();
    modde_core::paths::set_data_dir(root.path().join("data"));
    modde_core::paths::set_config_dir(root.path().join("config"));
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let context = SaveContext {
        game_id: GameId::from("example"),
        scope: GameId::from("install-transaction"),
        directory: Some(root.path().join("live")),
    };
    let live = context.directory.as_ref().unwrap();
    std::fs::create_dir_all(live).unwrap();
    let mut ids = Vec::new();
    for name in ["original", "candidate"] {
        ids.push(
            pm.create(&Profile {
                id: None,
                name: name.into(),
                game_id: context.game_id.clone(),
                source: ProfileSource::Manual,
                mods: Vec::new(),
                overrides: root.path().join(name),
                load_order_rules: Default::default(),
                load_order_lock: None,
            })
            .await
            .unwrap(),
        );
    }
    let sm = SaveManager::new(pm.db());
    std::fs::write(live.join("slot.sav"), "old snapshot").unwrap();
    sm.adopt(&context.scope, "original", live).unwrap();
    let old = SaveManager::history(&context.scope, "original", 1).unwrap()[0]
        .id
        .clone();
    pm.db()
        .set_active_profile(&context.scope, ids[0])
        .await
        .unwrap();
    pm.db()
        .push_experiment(&context.scope, ids[1])
        .await
        .unwrap();
    pm.db()
        .push_experiment(&context.scope, ids[0])
        .await
        .unwrap();
    std::fs::write(live.join("slot.sav"), "latest live save").unwrap();

    let mut session = PendingSession::for_save_operation(&pm, &context)
        .await
        .unwrap();
    session.save().unwrap();
    session.prepare_save_transition(&pm, None).await.unwrap();
    // A restore has already reset the original branch, and a later copy/DB
    // update is interrupted. Recovery cannot use the branch's moving HEAD.
    SaveManager::restore(&context.scope, "original", &old, live).unwrap();
    std::fs::write(live.join("slot.sav"), "partial replacement").unwrap();
    // Untracked leftovers in the vault must not leak into snapshot recovery.
    std::fs::write(
        modde_core::paths::save_vault_dir(&context.scope).join("partial.tmp"),
        "partial capture",
    )
    .unwrap();
    pm.db()
        .set_active_profile(&context.scope, ids[1])
        .await
        .unwrap();
    pm.db()
        .clear_experiment_stack(&context.scope)
        .await
        .unwrap();
    let session = PendingSession::load().unwrap().unwrap();
    session.restore_preparation(&pm).await.unwrap();
    session.restore_preparation(&pm).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(live.join("slot.sav")).unwrap(),
        "latest live save"
    );
    assert!(!live.join("partial.tmp").exists());
    assert_eq!(
        pm.db()
            .get_active_profile(&context.scope)
            .await
            .unwrap()
            .unwrap()
            .0,
        ids[0]
    );
    assert_eq!(
        pm.db().experiment_profiles(&context.scope).await.unwrap(),
        vec![ids[1], ids[0]]
    );
    assert!(
        PendingSession::for_save_operation(&pm, &context)
            .await
            .is_err()
    );

    // A recovery IO failure keeps its marker and can be retried.
    std::fs::remove_dir_all(live).unwrap();
    std::fs::write(live, "blocked destination").unwrap();
    assert!(session.restore_preparation(&pm).await.is_err());
    assert!(PendingSession::load().unwrap().is_some());
    std::fs::remove_file(live).unwrap();
    session.restore_preparation(&pm).await.unwrap();
    PendingSession::clear().unwrap();

    let mut session = PendingSession::for_save_operation(&pm, &context)
        .await
        .unwrap();
    session.save().unwrap();
    session
        .restore_snapshot(&pm, "original", &old, None)
        .await
        .unwrap();
    session.advance(SessionPhase::SaveCommitted).unwrap();
    // Simulate a crash between the durable commit marker and journal removal.
    PendingSession::load()
        .unwrap()
        .unwrap()
        .restore_preparation(&pm)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(live.join("slot.sav")).unwrap(),
        "old snapshot"
    );
    PendingSession::clear().unwrap();
}
