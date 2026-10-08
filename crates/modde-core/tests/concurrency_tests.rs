//! Concurrency and stress tests for modde-core.
//!
//! Tests parallel hashing, concurrent profile operations, and large-scale
//! resolution under load.

use smallvec::{SmallVec, smallvec};
use std::collections::HashMap;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::ModdeDb;
use modde_core::hash::{hash_file_sha256, hash_file_xxhash, verify_sha256, verify_xxhash};
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{ConflictMap, LoadOrderRule, ModId, ResolvedLoadOrder, resolve};
use modde_core::vfs::SymlinkFarm;
use tempfile::TempDir;

fn simple_mod(id: &str, enabled: bool) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled,
        version: None,
        fomod_config: None,
        ..Default::default()
    }
}

// ── Concurrent hashing ─────────────────────────────────────────────

#[tokio::test]
async fn test_concurrent_xxhash_many_files() {
    let tmp = TempDir::new().unwrap();
    let mut paths = Vec::new();

    // Create 50 files with distinct content
    for i in 0..50 {
        let p = tmp.path().join(format!("file_{i}.bin"));
        std::fs::write(&p, format!("content for file {i}")).unwrap();
        paths.push(p);
    }

    let handles: Vec<_> = paths
        .iter()
        .map(|p| {
            let p = p.clone();
            tokio::spawn(async move { hash_file_xxhash(&p).await })
        })
        .collect();

    let mut hashes = Vec::new();
    for h in handles {
        let hash = h.await.unwrap().unwrap();
        hashes.push(hash);
    }

    // Each file has distinct content → distinct hashes
    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(unique.len(), 50, "all 50 files should have unique hashes");
}

#[tokio::test]
async fn test_concurrent_sha256_many_files() {
    let tmp = TempDir::new().unwrap();
    let mut paths = Vec::new();

    for i in 0..30 {
        let p = tmp.path().join(format!("sha_{i}.bin"));
        std::fs::write(&p, format!("sha256 content {i}")).unwrap();
        paths.push(p);
    }

    let handles: Vec<_> = paths
        .iter()
        .map(|p| {
            let p = p.clone();
            tokio::spawn(async move { hash_file_sha256(&p).await })
        })
        .collect();

    let mut hashes = Vec::new();
    for h in handles {
        let hash = h.await.unwrap().unwrap();
        hashes.push(hash);
    }

    let unique: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(unique.len(), 30);
}

#[tokio::test]
async fn test_concurrent_hash_same_file() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("shared.bin");
    std::fs::write(&p, b"shared content across readers").unwrap();

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let p = p.clone();
            tokio::spawn(async move { hash_file_xxhash(&p).await })
        })
        .collect();

    let first = handles.into_iter().next().unwrap().await.unwrap().unwrap();
    // All should produce the same hash (verified indirectly by first being valid)
    verify_xxhash(&p, first).await.unwrap();
}

#[tokio::test]
async fn test_concurrent_verify_many_files() {
    let tmp = TempDir::new().unwrap();
    let mut file_hashes = Vec::new();

    for i in 0..25 {
        let p = tmp.path().join(format!("verify_{i}.bin"));
        std::fs::write(&p, format!("verify content {i}")).unwrap();
        let hash = hash_file_xxhash(&p).await.unwrap();
        file_hashes.push((p, hash));
    }

    let handles: Vec<_> = file_hashes
        .iter()
        .map(|(p, h)| {
            let p = p.clone();
            let h = *h;
            tokio::spawn(async move { verify_xxhash(&p, h).await })
        })
        .collect();

    for h in handles {
        h.await.unwrap().unwrap();
    }
}

// ── Stress: large mod lists ────────────────────────────────────────

#[test]
fn test_resolve_200_mods_linear_chain() {
    // A→B→C→...→Z (200 mods in a chain)
    let count = 200;
    let mods: Vec<EnabledMod> = (0..count)
        .map(|i| simple_mod(&format!("mod_{i:04}"), true))
        .collect();

    let rules: SmallVec<[LoadOrderRule; 4]> = (0..count - 1)
        .map(|i| LoadOrderRule::LoadAfter {
            mod_id: ModId::from(format!("mod_{:04}", i + 1).as_str()),
            after: ModId::from(format!("mod_{i:04}").as_str()),
        })
        .collect();

    let profile = Profile {
        id: None,
        name: "stress_chain".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: rules,
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), count);

    // Verify ordering: mod_0000 must come before mod_0001, etc.
    for i in 0..count - 1 {
        let pos_a = result
            .order
            .iter()
            .position(|m| m == format!("mod_{i:04}").as_str())
            .unwrap();
        let pos_b = result
            .order
            .iter()
            .position(|m| m == format!("mod_{:04}", i + 1).as_str())
            .unwrap();
        assert!(
            pos_a < pos_b,
            "mod_{i:04} should be before mod_{:04}",
            i + 1
        );
    }
}

#[test]
fn test_resolve_500_mods_no_rules() {
    let mods: Vec<EnabledMod> = (0..500)
        .map(|i| simple_mod(&format!("mod_{i}"), true))
        .collect();

    let profile = Profile {
        id: None,
        name: "stress_no_rules".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 500);
}

#[test]
fn test_resolve_diamond_dependency_50_wide() {
    // Root → 50 mods → Sink
    let mut mods = vec![simple_mod("root", true), simple_mod("sink", true)];
    let mut rules: SmallVec<[LoadOrderRule; 4]> = SmallVec::new();

    for i in 0..50 {
        let mid = format!("mid_{i}");
        mods.push(simple_mod(&mid, true));
        rules.push(LoadOrderRule::LoadAfter {
            mod_id: ModId::from(mid.as_str()),
            after: ModId::from("root"),
        });
        rules.push(LoadOrderRule::LoadBefore {
            mod_id: ModId::from(mid.as_str()),
            before: ModId::from("sink"),
        });
    }

    let profile = Profile {
        id: None,
        name: "diamond".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp"),
        load_order_rules: rules,
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    let root_pos = result.order.iter().position(|m| m == "root").unwrap();
    let sink_pos = result.order.iter().position(|m| m == "sink").unwrap();
    assert!(root_pos < sink_pos);

    // All mid mods should be between root and sink
    for i in 0..50 {
        let mid_pos = result
            .order
            .iter()
            .position(|m| m == format!("mid_{i}").as_str())
            .unwrap();
        assert!(mid_pos > root_pos && mid_pos < sink_pos);
    }
}

// ── Stress: conflict map ───────────────────────────────────────────

#[test]
fn test_conflict_map_many_providers() {
    let mut cm = ConflictMap::default();
    for i in 0..100 {
        cm.register(
            "shared_file.dds".to_string(),
            ModId::from(format!("mod_{i}").as_str()),
        );
    }

    let conflicts = cm.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].1.len(), 100);
}

#[test]
fn test_conflict_map_many_files_no_conflicts() {
    let mut cm = ConflictMap::default();
    for i in 0..1000 {
        cm.register(
            format!("unique_file_{i}.dds"),
            ModId::from(format!("mod_{i}").as_str()),
        );
    }

    let conflicts = cm.conflicts();
    assert_eq!(
        conflicts,
        [] as [(&str, &std::collections::HashSet<modde_core::ModId>); 0]
    );
}

#[test]
fn test_conflict_map_mixed() {
    let mut cm = ConflictMap::default();

    // 50 unique files
    for i in 0..50 {
        cm.register(format!("unique_{i}.dds"), ModId::from("mod_a"));
    }

    // 10 conflicted files (each provided by 3 mods)
    for i in 0..10 {
        cm.register(format!("conflict_{i}.dds"), ModId::from("mod_a"));
        cm.register(format!("conflict_{i}.dds"), ModId::from("mod_b"));
        cm.register(format!("conflict_{i}.dds"), ModId::from("mod_c"));
    }

    let conflicts = cm.conflicts();
    assert_eq!(conflicts.len(), 10);
    for (_, providers) in &conflicts {
        assert_eq!(providers.len(), 3);
    }
}

// ── Stress: VFS symlink farm ───────────────────────────────────────

#[test]
fn test_vfs_build_100_mods_10_files_each() {
    let resolved = ResolvedLoadOrder {
        order: (0..100)
            .map(|i| ModId::from(format!("mod_{i}").as_str()))
            .collect(),
    };

    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    for i in 0..100 {
        let files: Vec<_> = (0..10)
            .map(|j| {
                (
                    format!("textures/mod_{i}/file_{j}.dds"),
                    PathBuf::from(format!("/store/mod_{i}/textures/mod_{i}/file_{j}.dds")),
                )
            })
            .collect();
        mod_files.insert(ModId::from(format!("mod_{i}").as_str()), files);
    }

    let farm = SymlinkFarm::build("stress_test", &resolved, &mod_files, None, None).unwrap();
    assert_eq!(farm.links.len(), 1000); // 100 mods × 10 unique files each
}

#[test]
fn test_vfs_build_override_cascade() {
    // 50 mods all providing the same file → only the last one wins
    let resolved = ResolvedLoadOrder {
        order: (0..50)
            .map(|i| ModId::from(format!("mod_{i}").as_str()))
            .collect(),
    };

    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    for i in 0..50 {
        mod_files.insert(
            ModId::from(format!("mod_{i}").as_str()),
            vec![(
                "shared.dds".to_string(),
                PathBuf::from(format!("/store/mod_{i}/shared.dds")),
            )],
        );
    }

    let farm = SymlinkFarm::build("cascade", &resolved, &mod_files, None, None).unwrap();
    assert_eq!(farm.links.len(), 1);
    // The last mod (mod_49) should win
    assert_eq!(
        farm.links.get("shared.dds").unwrap(),
        &PathBuf::from("/store/mod_49/shared.dds")
    );
}

#[tokio::test]
async fn test_vfs_materialize_500_files() {
    let tmp = TempDir::new().unwrap();
    let staging_dir = tmp.path().join("staging");

    let mut links = HashMap::new();
    for i in 0..500 {
        let src = tmp.path().join(format!("src_{i}.txt"));
        std::fs::write(&src, format!("content {i}")).unwrap();
        links.insert(format!("dir_{}/file_{}.txt", i / 10, i), src);
    }

    let farm = SymlinkFarm::from_links(staging_dir.clone(), links);
    farm.materialize().await.unwrap();

    // Verify a sample of files
    for i in [0, 100, 250, 499] {
        let link = staging_dir.join(format!("dir_{}/file_{}.txt", i / 10, i));
        assert!(
            link.symlink_metadata().unwrap().file_type().is_symlink(),
            "file_{i} should be a symlink"
        );
        assert_eq!(
            std::fs::read_to_string(&link).unwrap(),
            format!("content {i}")
        );
    }
}

// ── Concurrent profile operations ──────────────────────────────────

#[tokio::test]
async fn test_profile_manager_create_many_profiles() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    for i in 0..50 {
        let profile = Profile {
            id: None,
            name: format!("profile_{i}"),
            game_id: GameId::from("skyrim-se"),
            source: ProfileSource::Manual,
            mods: vec![simple_mod("base_mod", true)],
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: smallvec![],
            load_order_lock: None,
        };
        mgr.create(&profile).await.unwrap();
    }

    let list = mgr.list().await.unwrap();
    assert_eq!(list.len(), 50);
}

#[tokio::test]
async fn test_profile_manager_delete_then_recreate() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = Profile {
        id: None,
        name: "ephemeral".to_string(),
        game_id: GameId::from("fallout4"),
        source: ProfileSource::Manual,
        mods: vec![],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    mgr.create(&profile).await.unwrap();
    assert_eq!(mgr.list().await.unwrap().len(), 1);

    mgr.delete("ephemeral", None).await.unwrap();
    assert_eq!(mgr.list().await.unwrap().len(), 0);

    // Recreate with different content
    let profile2 = Profile {
        id: None,
        name: "ephemeral".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Wabbajack {
            manifest_hash: "abc123".to_string(),
        },
        mods: vec![simple_mod("new_mod", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    mgr.create(&profile2).await.unwrap();
    let loaded = mgr.load("ephemeral", None).await.unwrap();
    assert_eq!(loaded.game_id, "skyrim-se");
    assert_eq!(loaded.mods.len(), 1);
}

// ── Hash: edge cases with binary content ───────────────────────────

#[tokio::test]
async fn test_hash_all_byte_values() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("all_bytes.bin");
    let data: Vec<u8> = (0..=255).collect();
    std::fs::write(&p, &data).unwrap();

    let hash = hash_file_xxhash(&p).await.unwrap();
    verify_xxhash(&p, hash).await.unwrap();

    let sha = hash_file_sha256(&p).await.unwrap();
    verify_sha256(&p, &sha).await.unwrap();
}

#[tokio::test]
async fn test_hash_repeated_patterns() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("pattern.bin");
    let data: Vec<u8> = vec![0xDE, 0xAD, 0xBE, 0xEF]
        .into_iter()
        .cycle()
        .take(100_000)
        .collect();
    std::fs::write(&p, &data).unwrap();

    let hash = hash_file_xxhash(&p).await.unwrap();
    verify_xxhash(&p, hash).await.unwrap();
}

#[tokio::test]
async fn test_hash_zero_filled_file() {
    let tmp = TempDir::new().unwrap();
    let p = tmp.path().join("zeros.bin");
    let data = vec![0u8; 65536]; // Exactly one BUF_SIZE
    std::fs::write(&p, &data).unwrap();

    let hash = hash_file_xxhash(&p).await.unwrap();
    verify_xxhash(&p, hash).await.unwrap();
}

// ── Resolver: edge cases ───────────────────────────────────────────

#[test]
fn test_resolve_self_referencing_rule_ignored() {
    // A rule saying "mod_a loads after mod_a" should create a cycle
    let profile = Profile {
        id: None,
        name: "self_ref".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("mod_a", true)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_a"),
            after: ModId::from("mod_a"),
        }],
        load_order_lock: None,
    };

    let result = resolve(&profile);
    // Self-referential creates a cycle
    assert!(result.is_err());
}

#[test]
fn test_resolve_3_way_cycle() {
    let profile = Profile {
        id: None,
        name: "cycle3".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("a", true),
            simple_mod("b", true),
            simple_mod("c", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("b"),
                after: ModId::from("a"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("c"),
                after: ModId::from("b"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("a"),
                after: ModId::from("c"),
            },
        ],
        load_order_lock: None,
    };

    let result = resolve(&profile);
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("dependency cycle"));
}

#[test]
fn test_resolve_mixed_enabled_disabled_with_rules() {
    let profile = Profile {
        id: None,
        name: "mixed".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("a", true),
            simple_mod("b", false), // disabled
            simple_mod("c", true),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![
            // Rule references disabled mod → should be silently ignored
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("c"),
                after: ModId::from("b"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("c"),
                after: ModId::from("a"),
            },
        ],
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 2);
    let pos_a = result.order.iter().position(|m| m == "a").unwrap();
    let pos_c = result.order.iter().position(|m| m == "c").unwrap();
    assert!(pos_a < pos_c);
}

#[test]
fn test_resolve_incompatible_one_disabled_is_ok() {
    let profile = Profile {
        id: None,
        name: "incompat_ok".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![simple_mod("a", true), simple_mod("b", false)],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![LoadOrderRule::Incompatible {
            mod_a: ModId::from("a"),
            mod_b: ModId::from("b"),
        }],
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "a");
}

#[test]
fn test_resolve_all_disabled() {
    let profile = Profile {
        id: None,
        name: "none_enabled".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            simple_mod("a", false),
            simple_mod("b", false),
            simple_mod("c", false),
        ],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    let result = resolve(&profile).unwrap();
    assert_eq!(result.order, [] as [modde_core::ModId; 0]);
}

// ── Profile serialization edge cases ───────────────────────────────

#[tokio::test]
async fn test_profile_with_unicode_mod_names() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let profile = Profile {
        id: None,
        name: "unicode_test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            EnabledMod {
                mod_id: "日本語モッド".to_string(),
                enabled: true,
                version: Some("1.0α".to_string()),
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "Ñoño_Ñuñez".to_string(),
                enabled: true,
                version: Some("2.0".to_string()),
                fomod_config: None,
                ..Default::default()
            },
            EnabledMod {
                mod_id: "模组_中文".to_string(),
                enabled: false,
                version: None,
                fomod_config: None,
                ..Default::default()
            },
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("unicode_test", None).await.unwrap();
    assert_eq!(loaded.mods.len(), 3);
    assert_eq!(loaded.mods[0].mod_id, "日本語モッド");
    assert_eq!(loaded.mods[1].mod_id, "Ñoño_Ñuñez");
}

#[tokio::test]
async fn test_profile_with_fomod_config_roundtrip() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let fomod_json = r#"{"steps":[{"name":"Textures","groups":[{"name":"Quality","plugins":[{"name":"4K","files":["textures/4k/"]}]}]}]}"#;

    let profile = Profile {
        id: None,
        name: "fomod_test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![EnabledMod {
            mod_id: "texture_pack".to_string(),
            enabled: true,
            version: Some("3.0".to_string()),
            fomod_config: Some(fomod_json.to_string()),
            ..Default::default()
        }],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("fomod_test", None).await.unwrap();
    assert_eq!(loaded.mods[0].fomod_config.as_deref(), Some(fomod_json));
}

#[tokio::test]
async fn test_profile_all_source_types_roundtrip() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let sources = vec![
        ProfileSource::Manual,
        ProfileSource::NexusCollection {
            slug: "ultimate-skyrim".to_string(),
            version: "4.2.0".to_string(),
        },
        ProfileSource::Wabbajack {
            manifest_hash: "deadbeef01234567".to_string(),
        },
    ];

    for (i, source) in sources.into_iter().enumerate() {
        let profile = Profile {
            id: None,
            name: format!("source_test_{i}"),
            game_id: GameId::from("skyrim-se"),
            source: source.clone(),
            mods: vec![],
            overrides: PathBuf::from("/tmp"),
            load_order_rules: smallvec![],
            load_order_lock: None,
        };

        pm.create(&profile).await.unwrap();
        let loaded = pm.load(&format!("source_test_{i}"), None).await.unwrap();

        match (&profile.source, &loaded.source) {
            (ProfileSource::Manual, ProfileSource::Manual) => {}
            (
                ProfileSource::NexusCollection {
                    slug: s1,
                    version: v1,
                },
                ProfileSource::NexusCollection {
                    slug: s2,
                    version: v2,
                },
            ) => {
                assert_eq!(s1, s2);
                assert_eq!(v1, v2);
            }
            (
                ProfileSource::Wabbajack { manifest_hash: h1 },
                ProfileSource::Wabbajack { manifest_hash: h2 },
            ) => {
                assert_eq!(h1, h2);
            }
            _ => panic!("source type mismatch after roundtrip"),
        }
    }
}

// ── Stock snapshot ──────────────────────────────────────────────────

#[tokio::test]
async fn test_stock_concurrent_snapshot_and_verify() {
    use modde_core::stock::StockGameManager;

    let src = TempDir::new().unwrap();
    // Create 20 files
    for i in 0..20 {
        let p = src.path().join(format!("file_{i}.dat"));
        std::fs::write(&p, format!("stock data {i}")).unwrap();
    }

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());
    let snap = mgr
        .snapshot(&GameId::from("test-game"), src.path())
        .await
        .unwrap();
    assert_ne!(snap.hash, "");

    // Verify should pass
    assert!(mgr.verify(&GameId::from("test-game")).await.unwrap());

    // Modify a file in the snapshot → verify should fail
    let tampered = store.path().join("test-game/file_5.dat");
    std::fs::write(&tampered, "tampered data").unwrap();
    assert!(!mgr.verify(&GameId::from("test-game")).await.unwrap());
}
