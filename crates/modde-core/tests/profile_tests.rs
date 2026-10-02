use smallvec::smallvec;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::ModdeDb;
use modde_core::error::CoreError;
use modde_core::installer::{InstallMethod, InstallStatus};
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{LoadOrderRule, ModId};
use pretty_assertions::assert_eq;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_manual_profile(name: &str, game_id: &str, mods: Vec<EnabledMod>) -> Profile {
    Profile {
        id: None,
        name: name.to_string(),
        game_id: GameId::from(game_id),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    }
}

fn simple_mod(id: &str, enabled: bool) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled,
        version: None,
        fomod_config: None,
        ..Default::default()
    }
}

/// Assert two profiles have identical content by comparing every field.
/// The types intentionally do not derive `PartialEq`, so we compare field-by-field
/// via their TOML serialization which is deterministic.
fn assert_profiles_eq(a: &Profile, b: &Profile) {
    let a_toml = toml::to_string_pretty(a).expect("serialize a");
    let b_toml = toml::to_string_pretty(b).expect("serialize b");
    assert_eq!(a_toml, b_toml);
}

#[test]
fn legacy_enabled_mod_metadata_deserializes_to_typed_fields() {
    let method_raw = toml::to_string(&InstallMethod::BareExtract).unwrap();
    let legacy = format!(
        "mod_id = \"legacy\"\n\
         enabled = true\n\
         install_status = \"pending_user_input\"\n\
         install_method = {method_raw:?}\n\
         tags = '[\"quest\",\"ui\"]'\n"
    );

    let enabled: EnabledMod = toml::from_str(&legacy).unwrap();
    assert_eq!(
        enabled.install_status,
        Some(InstallStatus::PendingUserInput)
    );
    assert_eq!(enabled.install_method, Some(InstallMethod::BareExtract));
    assert_eq!(enabled.tags, vec!["quest".to_string(), "ui".to_string()]);

    let missing_status: EnabledMod =
        toml::from_str("mod_id = \"legacy-missing\"\nenabled = true\n").unwrap();
    assert_eq!(missing_status.install_status, None);
    assert_eq!(missing_status.tags, [] as [std::string::String; 0]);
}

// ===========================================================================
// Profile serialization tests
// ===========================================================================

#[tokio::test]
async fn test_profile_save_and_load_roundtrip() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile(
        "my-profile",
        "skyrim-se",
        vec![
            simple_mod("skse", true),
            simple_mod("ussep", true),
            simple_mod("broken-mod", false),
        ],
    );

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("my-profile", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);
}

#[tokio::test]
async fn test_profile_create_and_load_by_name() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile("nested", "fallout4", vec![]);
    pm.create(&profile).await.unwrap();

    let loaded = pm.load("nested", None).await.unwrap();
    assert_eq!(loaded.name, "nested");
}

#[tokio::test]
async fn test_profile_load_nonexistent() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let err = pm.load("does-not-exist", None).await.unwrap_err();
    assert!(
        matches!(err, CoreError::ProfileNotFound(_)),
        "expected ProfileNotFound error, got: {err:?}"
    );
}

#[tokio::test]
async fn test_profile_load_nonexistent_by_game() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let err = pm
        .load("does-not-exist", Some(&GameId::from("skyrim-se")))
        .await
        .unwrap_err();
    assert!(
        matches!(err, CoreError::ProfileNotFound(_)),
        "expected ProfileNotFound error, got: {err:?}"
    );
}

#[tokio::test]
async fn test_profile_with_nexus_collection_source() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = Profile {
        id: None,
        name: "nexus-collection".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::NexusCollection {
            slug: "living-skyrim".to_string(),
            version: "4.2.0".to_string(),
        },
        mods: vec![simple_mod("skse", true)],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("nexus-collection", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);

    // Also verify the variant fields explicitly
    match &loaded.source {
        ProfileSource::NexusCollection { slug, version } => {
            assert_eq!(slug, "living-skyrim");
            assert_eq!(version, "4.2.0");
        }
        other => panic!("expected NexusCollection, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_profile_with_wabbajack_source() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = Profile {
        id: None,
        name: "wj-list".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Wabbajack {
            manifest_hash: "abc123def456".to_string(),
        },
        mods: vec![simple_mod("engine-fixes", true)],
        overrides: PathBuf::from("/data/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("wj-list", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);

    match &loaded.source {
        ProfileSource::Wabbajack { manifest_hash } => {
            assert_eq!(manifest_hash, "abc123def456");
        }
        other => panic!("expected Wabbajack, got: {other:?}"),
    }
}

#[tokio::test]
async fn test_profile_with_load_order_rules() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let rules = smallvec![
        LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_b"),
            after: ModId::from("mod_a"),
        },
        LoadOrderRule::LoadBefore {
            mod_id: ModId::from("mod_c"),
            before: ModId::from("mod_d"),
        },
        LoadOrderRule::Incompatible {
            mod_a: ModId::from("mod_x"),
            mod_b: ModId::from("mod_y"),
        },
    ];

    let profile = Profile {
        id: None,
        name: "with-rules".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("mod_a", true),
            simple_mod("mod_b", true),
            simple_mod("mod_c", true),
            simple_mod("mod_d", true),
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: rules,
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("with-rules", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);
    assert_eq!(loaded.load_order_rules.len(), 3);
}

#[tokio::test]
async fn test_profile_with_many_mods() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mods: Vec<EnabledMod> = (0..500)
        .map(|i| EnabledMod {
            mod_id: format!("mod_{i:04}"),
            enabled: i % 3 != 0, // disable every third
            version: if i % 5 == 0 {
                Some(format!("{}.{}.0", i / 100, i % 100))
            } else {
                None
            },
            fomod_config: None,
            ..Default::default()
        })
        .collect();

    let profile = make_manual_profile("big-list", "fallout4", mods);

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("big-list", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);
    assert_eq!(loaded.mods.len(), 500);
}

#[tokio::test]
async fn test_profile_mod_with_version() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile(
        "versioned",
        "skyrim-se",
        vec![
            EnabledMod {
                mod_id: "skse".to_string(),
                enabled: true,
                version: Some("2.2.6".to_string()),
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "ussep".to_string(),
                enabled: true,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
        ],
    );

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("versioned", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);
    assert_eq!(loaded.mods[0].version.as_deref(), Some("2.2.6"));
    assert_eq!(loaded.mods[1].version, None);
}

#[tokio::test]
async fn test_profile_empty_mods_list() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile("empty", "skyrim-se", vec![]);

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("empty", None).await.unwrap();

    assert_profiles_eq(&profile, &loaded);
    assert!(loaded.mods.is_empty());
}

// ===========================================================================
// ProfileManager tests
// ===========================================================================

#[tokio::test]
async fn test_pm_list_empty_db() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profiles = pm.list().await.unwrap();
    assert_eq!(profiles, [] as [modde_core::ProfileSummary; 0]);
}

#[tokio::test]
async fn test_pm_create_and_load() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile(
        "test-profile",
        "skyrim-se",
        vec![simple_mod("skse", true), simple_mod("ussep", true)],
    );

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("test-profile", None).await.unwrap();

    assert_eq!(loaded.name, "test-profile");
    assert_eq!(loaded.game_id, "skyrim-se");
    assert_eq!(loaded.mods.len(), 2);
    assert_eq!(loaded.mods[0].mod_id, "skse");
    assert_eq!(loaded.mods[1].mod_id, "ussep");
}

#[tokio::test]
async fn test_pm_create_duplicate() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile("dup", "skyrim-se", vec![]);
    pm.create(&profile).await.unwrap();

    let err = pm.create(&profile).await.unwrap_err();
    // SQLite backend returns a constraint violation for duplicate profiles
    assert!(
        matches!(err, CoreError::ProfileAlreadyExists(ref name) if name == "dup")
            || format!("{err:?}").contains("UNIQUE constraint"),
        "expected ProfileAlreadyExists or UNIQUE constraint error, got: {err:?}"
    );
}

#[tokio::test]
async fn test_pm_delete_existing() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_manual_profile("to-delete", "skyrim-se", vec![]);
    pm.create(&profile).await.unwrap();

    // Confirm it exists
    assert_eq!(pm.list().await.unwrap().len(), 1);

    pm.delete("to-delete", None).await.unwrap();

    // Confirm it is gone
    assert_eq!(
        pm.list().await.unwrap(),
        [] as [modde_core::ProfileSummary; 0]
    );

    let err = pm.load("to-delete", None).await.unwrap_err();
    assert!(
        matches!(err, CoreError::ProfileNotFound(_)),
        "expected ProfileNotFound after delete, got: {err:?}"
    );
}

#[tokio::test]
async fn test_pm_delete_nonexistent() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let err = pm.delete("ghost", None).await.unwrap_err();
    assert!(
        matches!(err, CoreError::ProfileNotFound(ref name) if name == "ghost"),
        "expected ProfileNotFound, got: {err:?}"
    );
}

#[tokio::test]
async fn test_pm_list_multiple() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let names = ["alpha", "beta", "gamma", "delta"];
    for name in &names {
        let profile = make_manual_profile(name, "skyrim-se", vec![]);
        pm.create(&profile).await.unwrap();
    }

    let mut listed: Vec<String> = pm
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    listed.sort();
    let mut expected = names.map(String::from).to_vec();
    expected.sort();

    assert_eq!(listed, expected);
}

#[tokio::test]
async fn test_pm_list_only_created_profiles() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    // Create a single profile
    let profile = make_manual_profile("real-profile", "skyrim-se", vec![]);
    pm.create(&profile).await.unwrap();

    let listed = pm.list().await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "real-profile");
}
