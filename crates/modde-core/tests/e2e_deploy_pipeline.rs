//! End-to-end test: full deploy pipeline
//!
//! Profile creation -> load order resolution -> VFS build ->
//! materialize -> deploy -> verify content

use smallvec::smallvec;
use std::collections::HashMap;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::ModdeDb;
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{ConflictMap, LoadOrderRule, ModId, resolve};
use modde_core::vfs::SymlinkFarm;
use tempfile::TempDir;

fn simple_mod(id: &str, enabled: bool) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled,
        version: Some("1.0".to_string()),
        fomod_config: None,
        ..Default::default()
    }
}

#[tokio::test]
async fn test_full_deploy_pipeline() {
    let tmp = TempDir::new().unwrap();

    // Step 1: Create a profile with mods and load order rules
    let profile = Profile {
        id: None,
        name: "e2e_test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("base_textures", true),
            simple_mod("texture_overhaul", true),
            simple_mod("disabled_mod", false),
            simple_mod("patch_mod", true),
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![
            // texture_overhaul must load after base_textures
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("texture_overhaul"),
                after: ModId::from("base_textures"),
            },
            // patch_mod must load after texture_overhaul
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("patch_mod"),
                after: ModId::from("texture_overhaul"),
            },
        ],
        load_order_lock: None,
    };

    // Step 2: Save and reload the profile via DB
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    pm.create(&profile).await.unwrap();
    let loaded_profile = pm.load("e2e_test", None).await.unwrap();
    assert_eq!(loaded_profile.name, "e2e_test");
    assert_eq!(loaded_profile.mods.len(), 4);

    // Step 3: Resolve load order
    let resolved = resolve(&loaded_profile).unwrap();
    // disabled_mod should be excluded
    assert_eq!(resolved.order.len(), 3);
    assert!(!resolved.order.contains(&ModId::from("disabled_mod")));

    // Verify ordering constraints
    let pos_base = resolved
        .order
        .iter()
        .position(|m| m == "base_textures")
        .unwrap();
    let pos_overhaul = resolved
        .order
        .iter()
        .position(|m| m == "texture_overhaul")
        .unwrap();
    let pos_patch = resolved
        .order
        .iter()
        .position(|m| m == "patch_mod")
        .unwrap();
    assert!(
        pos_base < pos_overhaul,
        "base_textures must come before texture_overhaul"
    );
    assert!(
        pos_overhaul < pos_patch,
        "texture_overhaul must come before patch_mod"
    );

    // Step 4: Create mod files in a mock store
    let store = tmp.path().join("store");

    // base_textures provides sky.dds and ground.dds
    let base_dir = store.join("base_textures");
    std::fs::create_dir_all(&base_dir).unwrap();
    std::fs::write(base_dir.join("sky.dds"), "base_sky").unwrap();
    std::fs::write(base_dir.join("ground.dds"), "base_ground").unwrap();

    // texture_overhaul overrides sky.dds
    let overhaul_dir = store.join("texture_overhaul");
    std::fs::create_dir_all(&overhaul_dir).unwrap();
    std::fs::write(overhaul_dir.join("sky.dds"), "overhaul_sky").unwrap();

    // patch_mod adds patch.esp and overrides ground.dds
    let patch_dir = store.join("patch_mod");
    std::fs::create_dir_all(&patch_dir).unwrap();
    std::fs::write(patch_dir.join("patch.esp"), "patch_plugin").unwrap();
    std::fs::write(patch_dir.join("ground.dds"), "patch_ground").unwrap();

    // Build mod_files map
    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        ModId::from("base_textures"),
        vec![
            ("textures/sky.dds".into(), base_dir.join("sky.dds")),
            ("textures/ground.dds".into(), base_dir.join("ground.dds")),
        ],
    );
    mod_files.insert(
        ModId::from("texture_overhaul"),
        vec![("textures/sky.dds".into(), overhaul_dir.join("sky.dds"))],
    );
    mod_files.insert(
        ModId::from("patch_mod"),
        vec![
            ("patch.esp".into(), patch_dir.join("patch.esp")),
            ("textures/ground.dds".into(), patch_dir.join("ground.dds")),
        ],
    );

    // Step 5: Build the symlink farm
    // Use a custom staging dir within our temp
    let farm = SymlinkFarm::from_links(tmp.path().join("staging"), {
        let mut links = HashMap::new();
        for mod_id in &resolved.order {
            if let Some(files) = mod_files.get(mod_id) {
                for (rel_path, source) in files {
                    links.insert(rel_path.clone(), source.clone());
                }
            }
        }
        links
    });

    // Verify override resolution: 3 unique files
    assert_eq!(farm.links.len(), 3);
    // sky.dds should come from texture_overhaul (loaded after base_textures)
    assert!(
        farm.links["textures/sky.dds"]
            .to_string_lossy()
            .contains("texture_overhaul")
    );
    // ground.dds should come from patch_mod (loaded last)
    assert!(
        farm.links["textures/ground.dds"]
            .to_string_lossy()
            .contains("patch_mod")
    );
    // patch.esp only from patch_mod
    assert!(
        farm.links["patch.esp"]
            .to_string_lossy()
            .contains("patch_mod")
    );

    // Step 6: Materialize the symlink farm
    let farm = farm.materialize().await.unwrap();

    // Verify staging directory structure
    assert!(
        tmp.path()
            .join("staging/textures/sky.dds")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        tmp.path()
            .join("staging/textures/ground.dds")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(
        tmp.path()
            .join("staging/patch.esp")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );

    // Verify content through symlinks
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("staging/textures/sky.dds")).unwrap(),
        "overhaul_sky"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("staging/textures/ground.dds")).unwrap(),
        "patch_ground"
    );
    assert_eq!(
        std::fs::read_to_string(tmp.path().join("staging/patch.esp")).unwrap(),
        "patch_plugin"
    );

    // Step 7: Deploy to a game directory
    let game_dir = tmp.path().join("game/Data");
    farm.deploy_to(&game_dir).await.unwrap();

    // Verify deployment - symlinks in game dir point to staging
    assert!(
        game_dir
            .join("textures/sky.dds")
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    // Content accessible through the double-symlink chain
    assert_eq!(
        std::fs::read_to_string(game_dir.join("textures/sky.dds")).unwrap(),
        "overhaul_sky"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("textures/ground.dds")).unwrap(),
        "patch_ground"
    );
    assert_eq!(
        std::fs::read_to_string(game_dir.join("patch.esp")).unwrap(),
        "patch_plugin"
    );
}

#[tokio::test]
async fn test_profile_roundtrip_all_sources() {
    let profiles = vec![
        Profile {
            id: None,
            name: "manual".to_string(),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Manual,
            mods: vec![simple_mod("mod_a", true)],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: smallvec![],
            load_order_lock: None,
        },
        Profile {
            id: None,
            name: "nexus".to_string(),
            game_id: GameId::from("fallout4"),
            source: ProfileSource::NexusCollection {
                slug: "cool-collection".to_string(),
                version: "2.1".to_string(),
            },
            mods: vec![
                simple_mod("nexus_mod", true),
                simple_mod("optional_mod", false),
            ],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: smallvec![LoadOrderRule::Incompatible {
                mod_a: ModId::from("nexus_mod"),
                mod_b: ModId::from("conflicting_mod"),
            },],
            load_order_lock: None,
        },
        Profile {
            id: None,
            name: "wabbajack".to_string(),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Wabbajack {
                manifest_hash: "abc123def456".to_string(),
            },
            mods: vec![],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: smallvec![],
            load_order_lock: None,
        },
    ];

    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    for profile in &profiles {
        pm.create(profile).await.unwrap();
        let loaded = pm.load(&profile.name, None).await.unwrap();
        assert_eq!(loaded.name, profile.name);
        assert_eq!(loaded.game_id, profile.game_id);
        assert_eq!(loaded.mods.len(), profile.mods.len());
    }
}

#[tokio::test]
async fn test_resolve_and_detect_incompatible() {
    let profile = Profile {
        id: None,
        name: "conflict".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("enbseries", true), simple_mod("reshade", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::Incompatible {
            mod_a: ModId::from("enbseries"),
            mod_b: ModId::from("reshade"),
        },],
        load_order_lock: None,
    };

    let result = resolve(&profile);
    assert!(result.is_err(), "should detect incompatible mods");
}

#[tokio::test]
async fn test_conflict_map_populated_during_build() {
    let tmp = TempDir::new().unwrap();
    let store = tmp.path().join("store");

    // Two mods providing the same file
    let mod_a_dir = store.join("mod_a");
    std::fs::create_dir_all(&mod_a_dir).unwrap();
    std::fs::write(mod_a_dir.join("shared.esp"), "mod_a_version").unwrap();

    let mod_b_dir = store.join("mod_b");
    std::fs::create_dir_all(&mod_b_dir).unwrap();
    std::fs::write(mod_b_dir.join("shared.esp"), "mod_b_version").unwrap();

    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        ModId::from("mod_a"),
        vec![("shared.esp".into(), mod_a_dir.join("shared.esp"))],
    );
    mod_files.insert(
        ModId::from("mod_b"),
        vec![("shared.esp".into(), mod_b_dir.join("shared.esp"))],
    );

    // Build conflict map manually (as the resolver would)
    let mut conflict_map = ConflictMap::default();
    for (mod_id, files) in &mod_files {
        for (rel_path, _) in files {
            conflict_map.register(rel_path.clone(), mod_id.clone());
        }
    }

    let conflicts = conflict_map.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].0, "shared.esp");
    assert!(conflicts[0].1.contains("mod_a"));
    assert!(conflicts[0].1.contains("mod_b"));
}

// ── Smoke test: profile manager lifecycle ───────────────────────────

#[tokio::test]
async fn test_profile_manager_full_lifecycle() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    // Initially empty
    assert_eq!(
        mgr.list().await.unwrap(),
        [] as [modde_core::ProfileSummary; 0]
    );

    // Create profiles
    let p1 = Profile {
        id: None,
        name: "alpha".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("mod_a", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };
    let p2 = Profile {
        id: None,
        name: "beta".to_string(),
        game_id: GameId::from("fallout4"),
        source: ProfileSource::Manual,
        mods: vec![],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    mgr.create(&p1).await.unwrap();
    mgr.create(&p2).await.unwrap();

    // List should have both
    let mut names: Vec<String> = mgr
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    names.sort();
    assert_eq!(names, vec!["alpha", "beta"]);

    // Load and verify
    let loaded = mgr.load("alpha", None).await.unwrap();
    assert_eq!(loaded.game_id, "skyrim-se");
    assert_eq!(loaded.mods.len(), 1);

    // Duplicate create should fail
    assert!(mgr.create(&p1).await.is_err());

    // Delete
    mgr.delete("alpha", None).await.unwrap();
    let names: Vec<String> = mgr
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(names, vec!["beta"]);

    // Load deleted should fail
    assert!(mgr.load("alpha", None).await.is_err());

    // Delete nonexistent should fail
    assert!(mgr.delete("alpha", None).await.is_err());
}
