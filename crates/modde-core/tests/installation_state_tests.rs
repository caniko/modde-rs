//! Installation scopes must use shared game profiles but independent live state.
use std::sync::OnceLock;

use modde_core::library::{PendingSession, SaveContext, SessionPhase};
use modde_core::profile::{Profile, ProfileManager, ProfileSource};
use modde_core::save::SaveManager;
use modde_core::{GameId, ModdeDb};

fn isolate() {
    static DATA: OnceLock<tempfile::TempDir> = OnceLock::new();
    DATA.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        modde_core::paths::set_data_dir(dir.path().into());
        dir
    });
}

fn profile(name: &str) -> Profile {
    Profile {
        id: None,
        name: name.into(),
        game_id: GameId::from("example"),
        source: ProfileSource::Manual,
        mods: Vec::new(),
        overrides: ProfileManager::default_overrides(name),
        load_order_rules: Default::default(),
        load_order_lock: None,
    }
}

#[tokio::test]
async fn history_restore_fork_and_experiments_stay_in_the_selected_installation() {
    isolate();
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let base = pm.create(&profile("base")).await.unwrap();
    let a_dir = tempfile::tempdir().unwrap();
    let b_dir = tempfile::tempdir().unwrap();
    let a = SaveContext {
        game_id: GameId::from("example"),
        scope: GameId::from("install-test-a"),
        directory: Some(a_dir.path().into()),
    };
    let b = SaveContext {
        game_id: a.game_id.clone(),
        scope: GameId::from("install-test-b"),
        directory: Some(b_dir.path().into()),
    };
    let sm = SaveManager::new(pm.db());
    std::fs::write(a_dir.path().join("slot.sav"), "A").unwrap();
    std::fs::write(b_dir.path().join("slot.sav"), "B").unwrap();
    sm.adopt(&a.scope, "base", a_dir.path()).unwrap();
    sm.adopt(&b.scope, "base", b_dir.path()).unwrap();
    pm.db().set_active_profile(&a.scope, base).await.unwrap();
    pm.db().set_active_profile(&b.scope, base).await.unwrap();
    let old_a = SaveManager::history(&a.scope, "base", 1).unwrap()[0]
        .id
        .clone();
    let b_history = SaveManager::history(&b.scope, "base", 1).unwrap()[0]
        .id
        .clone();

    pm.fork_scoped("base", "experiment-a", &a, Default::default())
        .await
        .unwrap();
    pm.fork_scoped("base", "experiment-b", &b, Default::default())
        .await
        .unwrap();
    std::fs::write(a_dir.path().join("slot.sav"), "A-later").unwrap();
    pm.try_profile_scoped("experiment-a", &a, None)
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(a_dir.path().join("slot.sav")).unwrap(),
        "A"
    );
    pm.try_profile_scoped("experiment-b", &b, None)
        .await
        .unwrap();
    assert_eq!(
        pm.active(&a.scope).await.unwrap().unwrap().experiment_depth,
        1
    );
    pm.rollback_scoped(&a, None).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(a_dir.path().join("slot.sav")).unwrap(),
        "A-later"
    );
    assert_eq!(
        pm.active(&b.scope).await.unwrap().unwrap().profile.name,
        "experiment-b"
    );
    assert_eq!(
        pm.active(&b.scope).await.unwrap().unwrap().experiment_depth,
        1
    );
    SaveManager::restore(&a.scope, "base", &old_a, a_dir.path()).unwrap();
    assert_eq!(
        std::fs::read_to_string(a_dir.path().join("slot.sav")).unwrap(),
        "A"
    );
    assert_eq!(
        std::fs::read_to_string(b_dir.path().join("slot.sav")).unwrap(),
        "B"
    );
    assert_eq!(
        SaveManager::history(&b.scope, "base", 1).unwrap()[0].id,
        b_history
    );
    assert!(pm.active(&a.game_id).await.unwrap().is_none());
}

#[tokio::test]
async fn interrupted_switch_recovers_without_capturing_partial_saves_and_is_retryable() {
    isolate();
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let old = pm.create(&profile("old")).await.unwrap();
    let new = pm.create(&profile("new")).await.unwrap();
    let live = tempfile::tempdir().unwrap();
    let scope = GameId::from("install-recovery-test");
    let sm = SaveManager::new(pm.db());
    std::fs::write(live.path().join("slot.sav"), "complete-old").unwrap();
    sm.adopt(&scope, "old", live.path()).unwrap();
    let captured = SaveManager::history(&scope, "old", 1).unwrap()[0]
        .id
        .clone();
    pm.db().set_active_profile(&scope, new).await.unwrap();
    std::fs::write(live.path().join("slot.sav"), "partial-new").unwrap();
    let mut session = PendingSession {
        installation: "test".into(),
        name: "Example".into(),
        game_id: Some("example".into()),
        scope: scope.to_string(),
        profile: Some("new".into()),
        save_directory: Some(live.path().into()),
        capture: true,
        phase: SessionPhase::SwitchingSaves,
        previous_profile: Some((old, "old".into())),
        switched_profile: true,
        deployment_started: false,
        data_directory: Some(modde_core::paths::modde_data_dir()),
        install_path: None,
        prefix: None,
        save_transition: None,
        observation: None,
        diagnostics: None,
        launch_request: None,
    };
    session.restore_preparation(&pm).await.unwrap();
    session.restore_preparation(&pm).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(live.path().join("slot.sav")).unwrap(),
        "complete-old"
    );
    assert_eq!(
        pm.db().get_active_profile(&scope).await.unwrap().unwrap().0,
        old
    );
    assert_eq!(
        SaveManager::history(&scope, "old", 1).unwrap()[0].id,
        captured
    );
    session.phase = SessionPhase::Launching;
    assert!(session.restore_preparation(&pm).await.is_err());

    // Preparation without a switch must leave current saves untouched.
    session.phase = SessionPhase::Ready;
    session.switched_profile = false;
    std::fs::write(live.path().join("slot.sav"), "untouched").unwrap();
    session.restore_preparation(&pm).await.unwrap();
    assert_eq!(
        std::fs::read_to_string(live.path().join("slot.sav")).unwrap(),
        "untouched"
    );

    // First activation has no outgoing branch. Recovery removes a partially
    // deployed save set while retaining metadata; a missing directory is safe.
    session.switched_profile = true;
    session.previous_profile = None;
    std::fs::write(live.path().join("steam_autocloud.vdf"), "cloud metadata").unwrap();
    session.restore_preparation(&pm).await.unwrap();
    assert!(!live.path().join("slot.sav").exists());
    assert!(live.path().join("steam_autocloud.vdf").is_file());
    assert!(pm.db().get_active_profile(&scope).await.unwrap().is_none());
    assert!(
        sm.detect_unadopted(&scope, live.path())
            .await
            .unwrap()
            .is_none()
    );
    session.save_directory = Some(live.path().join("not-created"));
    session.restore_preparation(&pm).await.unwrap();
}

#[tokio::test]
async fn journalled_switch_parks_outgoing_saves_without_recapturing_them() {
    isolate();
    let db = ModdeDb::open_memory().await.unwrap();
    let sm = SaveManager::new(&db);
    let live = tempfile::tempdir().unwrap();
    let scope = GameId::from("install-parked-test");
    std::fs::write(live.path().join("slot.sav"), "original").unwrap();
    sm.adopt(&scope, "old", live.path()).unwrap();
    let commit = SaveManager::history(&scope, "old", 1).unwrap()[0]
        .id
        .clone();
    sm.activate_captured(&scope, "new", Some("old"), live.path())
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(live.path().join(".modde/profiles/old/slot.sav")).unwrap(),
        "original"
    );
    assert_eq!(
        SaveManager::history(&scope, "old", 1).unwrap()[0].id,
        commit
    );
}

#[tokio::test]
async fn captures_record_deletions_including_an_empty_live_save_set() {
    isolate();
    let db = ModdeDb::open_memory().await.unwrap();
    let sm = SaveManager::new(&db);
    let live = tempfile::tempdir().unwrap();
    let scope = GameId::from("install-empty-capture-test");
    std::fs::write(live.path().join("deleted.sav"), "old").unwrap();
    sm.adopt(&scope, "base", live.path()).unwrap();
    std::fs::remove_file(live.path().join("deleted.sav")).unwrap();
    sm.capture(&scope, "base", live.path()).unwrap();
    sm.deploy(&scope, "base", live.path()).unwrap();
    assert!(!live.path().join("deleted.sav").exists());
    assert_eq!(
        SaveManager::history(&scope, "base", 1).unwrap()[0].file_count,
        0
    );
}
