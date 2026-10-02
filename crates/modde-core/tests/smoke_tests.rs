//! Quick smoke tests that verify the basic API surface works.

use smallvec::smallvec;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::ModdeDb;
use modde_core::error::CoreError;
use modde_core::hash::{hash_file_sha256, hash_file_xxhash, verify_sha256, verify_xxhash};
use modde_core::manifest::collection::{
    CollectionAuthor, CollectionGame, CollectionManifest, CollectionVersion,
};
use modde_core::manifest::wabbajack::WabbajackManifest;
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::resolver::{ConflictMap, LoadOrderRule, ModId, ResolvedLoadOrder, resolve};
use modde_core::stock::StockGameManager;
use modde_core::vfs::SymlinkFarm;
use tempfile::TempDir;

// ── Helpers ──────────────────────────────────────────────────────────

fn simple_mod(id: &str, enabled: bool) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled,
        version: Some("1.0".to_string()),
        fomod_config: None,
        ..Default::default()
    }
}

fn make_profile(
    name: &str,
    mods: Vec<&str>,
    rules: smallvec::SmallVec<[LoadOrderRule; 4]>,
) -> Profile {
    Profile {
        id: None,
        name: name.to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: mods.into_iter().map(|id| simple_mod(id, true)).collect(),
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: rules,
        load_order_lock: None,
    }
}

// ── Profile serialize/deserialize ────────────────────────────────────

#[tokio::test]
async fn smoke_profile_roundtrip() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = Profile {
        id: None,
        name: "smoke".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Wabbajack {
            manifest_hash: "deadbeef".to_string(),
        },
        mods: vec![simple_mod("mod_a", true), simple_mod("mod_b", false)],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_b"),
            after: ModId::from("mod_a"),
        }],
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("smoke", None).await.unwrap();

    assert_eq!(loaded.name, "smoke");
    assert_eq!(loaded.game_id, "skyrim-se");
    assert_eq!(loaded.mods.len(), 2);
    assert_eq!(loaded.mods[0].mod_id, "mod_a");
    assert!(loaded.mods[0].enabled);
    assert!(!loaded.mods[1].enabled);
    assert_eq!(loaded.load_order_rules.len(), 1);
}

#[tokio::test]
async fn smoke_profile_nexus_collection_source_roundtrip() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = Profile {
        id: None,
        name: "nexus_col".to_string(),
        game_id: GameId::from("fallout4"),
        source: ProfileSource::NexusCollection {
            slug: "my-collection".to_string(),
            version: "3.2.1".to_string(),
        },
        mods: vec![],
        overrides: PathBuf::from("/tmp"),
        load_order_rules: smallvec![],
        load_order_lock: None,
    };

    pm.create(&profile).await.unwrap();
    let loaded = pm.load("nexus_col", None).await.unwrap();
    assert_eq!(loaded.name, "nexus_col");
    match &loaded.source {
        ProfileSource::NexusCollection { slug, version } => {
            assert_eq!(slug, "my-collection");
            assert_eq!(version, "3.2.1");
        }
        _ => panic!("expected NexusCollection source"),
    }
}

// ── ProfileManager ───────────────────────────────────────────────────

#[tokio::test]
async fn smoke_profile_manager_list_empty() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());
    let list = mgr.list().await.unwrap();
    assert_eq!(list, [] as [modde_core::ProfileSummary; 0]);
}

#[tokio::test]
async fn smoke_profile_manager_create_and_load() {
    let mgr = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_profile("test_profile", vec!["mod_a"], smallvec![]);
    mgr.create(&profile).await.unwrap();

    let loaded = mgr.load("test_profile", None).await.unwrap();
    assert_eq!(loaded.name, "test_profile");
    assert_eq!(loaded.mods.len(), 1);

    let list: Vec<String> = mgr
        .list()
        .await
        .unwrap()
        .into_iter()
        .map(|s| s.name)
        .collect();
    assert_eq!(list, vec!["test_profile"]);
}

// ── Hashing ──────────────────────────────────────────────────────────

#[tokio::test]
async fn smoke_hash_file_and_verify_xxhash() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"smoke test content").unwrap();

    let hash = hash_file_xxhash(f.path()).await.unwrap();
    assert!(hash != 0);
    verify_xxhash(f.path(), hash).await.unwrap();
}

#[tokio::test]
async fn smoke_hash_file_and_verify_sha256() {
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"smoke test sha256").unwrap();

    let hash = hash_file_sha256(f.path()).await.unwrap();
    assert_ne!(hash, "");
    verify_sha256(f.path(), &hash).await.unwrap();
}

// ── ConflictMap ──────────────────────────────────────────────────────

#[test]
fn smoke_conflict_map_register_and_query() {
    let mut cm = ConflictMap::default();

    // Single provider: no conflict
    cm.register("file_a.esp".to_string(), ModId::from("mod_1"));
    assert_eq!(
        cm.conflicts(),
        [] as [(&str, &std::collections::HashSet<modde_core::ModId>); 0]
    );

    // Two providers for same file: conflict
    cm.register("file_a.esp".to_string(), ModId::from("mod_2"));
    let conflicts = cm.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert!(conflicts[0].1.contains("mod_1"));
    assert!(conflicts[0].1.contains("mod_2"));

    // Different file, single provider: still only one conflict total
    cm.register("file_b.nif".to_string(), ModId::from("mod_3"));
    assert_eq!(cm.conflicts().len(), 1);
}

#[test]
fn smoke_conflict_map_empty() {
    let cm = ConflictMap::default();
    assert_eq!(
        cm.conflicts(),
        [] as [(&str, &std::collections::HashSet<modde_core::ModId>); 0]
    );
    assert!(cm.files.is_empty());
}

// ── resolve ──────────────────────────────────────────────────────────

#[test]
fn smoke_resolve_simple() {
    let profile = make_profile("simple", vec!["mod_a", "mod_b", "mod_c"], smallvec![]);
    let resolved = resolve(&profile).unwrap();
    assert_eq!(resolved.order.len(), 3);
}

#[test]
fn smoke_resolve_with_ordering() {
    let profile = make_profile(
        "ordered",
        vec!["mod_a", "mod_b"],
        smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_b"),
            after: ModId::from("mod_a"),
        }],
    );
    let resolved = resolve(&profile).unwrap();
    let pos_a = resolved.order.iter().position(|m| m == "mod_a").unwrap();
    let pos_b = resolved.order.iter().position(|m| m == "mod_b").unwrap();
    assert!(pos_a < pos_b);
}

// ── SymlinkFarm::build ──────────────────────────────────────────────

#[test]
fn smoke_symlink_farm_build() {
    let resolved = ResolvedLoadOrder {
        order: vec![ModId::from("mod_a"), ModId::from("mod_b")],
    };

    let mut mod_files: HashMap<ModId, Vec<(String, PathBuf)>> = HashMap::new();
    mod_files.insert(
        ModId::from("mod_a"),
        vec![(
            "textures/sky.dds".into(),
            PathBuf::from("/store/mod_a/sky.dds"),
        )],
    );
    mod_files.insert(
        ModId::from("mod_b"),
        vec![(
            "textures/sky.dds".into(),
            PathBuf::from("/store/mod_b/sky.dds"),
        )],
    );

    let farm = SymlinkFarm::build("smoke_profile", &resolved, &mod_files, None, None).unwrap();
    // mod_b wins because it's later in the order
    assert_eq!(farm.links.len(), 1);
    assert_eq!(
        farm.links.get("textures/sky.dds").unwrap(),
        &PathBuf::from("/store/mod_b/sky.dds")
    );
}

// ── WabbajackManifest ────────────────────────────────────────────────

#[test]
fn smoke_wabbajack_manifest_minimal_json() {
    let json = r#"{
        "Name": "TestList",
        "Author": "tester",
        "Description": "A test modlist",
        "Game": "SkyrimSpecialEdition",
        "Version": "1.0.0"
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.name, "TestList");
    assert_eq!(manifest.author, "tester");
    assert_eq!(manifest.description, "A test modlist");
    assert_eq!(manifest.game, "SkyrimSpecialEdition");
    assert_eq!(manifest.version, "1.0.0");
    assert!(manifest.archives.is_empty());
    assert!(manifest.directives.is_empty());
}

#[test]
fn smoke_wabbajack_manifest_with_archives() {
    let json = r#"{
        "Name": "Full",
        "Author": "a",
        "Description": "d",
        "Game": "SkyrimSE",
        "Version": "2.0",
        "Archives": [
            {
                "Hash": 12345,
                "Name": "archive.7z",
                "Size": 99999
            }
        ]
    }"#;

    let manifest: WabbajackManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.archives.len(), 1);
    assert_eq!(manifest.archives[0].hash, 12345);
    assert_eq!(manifest.archives[0].name, "archive.7z");
    assert_eq!(manifest.archives[0].size, 99999);
}

// ── CollectionManifest ───────────────────────────────────────────────

#[test]
fn smoke_collection_manifest_minimal_json() {
    let json = r#"{
        "slug": "my-collection",
        "name": "My Collection",
        "author": { "name": "TestUser" },
        "game": { "id": 1704, "domain_name": "skyrimspecialedition", "name": "Skyrim SE" },
        "mods": [],
        "version": { "version": "1.0.0" }
    }"#;

    let manifest: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.slug, "my-collection");
    assert_eq!(manifest.name, "My Collection");
    assert_eq!(manifest.author.name, "TestUser");
    assert_eq!(manifest.game.id, 1704);
    assert_eq!(manifest.game.domain_name, "skyrimspecialedition");
    assert!(manifest.mods.is_empty());
    assert_eq!(manifest.version.version, "1.0.0");
}

#[test]
fn smoke_collection_manifest_with_mods() {
    let json = r#"{
        "slug": "test",
        "name": "Test",
        "author": { "name": "A", "member_id": 42 },
        "game": { "id": 1, "domain_name": "skyrim", "name": "Skyrim" },
        "mods": [
            {
                "mod_id": 100,
                "file_id": 200,
                "name": "Cool Mod",
                "version": "1.2.3",
                "optional": true,
                "install_order": 5
            }
        ],
        "version": { "version": "2.0", "created_at": "2025-01-01" },
        "endorsements": 999
    }"#;

    let manifest: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.mods.len(), 1);
    assert_eq!(manifest.mods[0].mod_id.get(), 100);
    assert_eq!(manifest.mods[0].file_id.get(), 200);
    assert!(manifest.mods[0].optional);
    assert_eq!(manifest.mods[0].install_order, 5);
    assert_eq!(manifest.endorsements, 999);
    assert_eq!(manifest.author.member_id, Some(42));
}

// ── CoreError variants ───────────────────────────────────────────────

#[test]
fn smoke_all_error_variants_display() {
    let errors: Vec<CoreError> = vec![
        CoreError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "gone")),
        CoreError::Json(serde_json::from_str::<serde_json::Value>("not json").unwrap_err()),
        CoreError::HashMismatch {
            path: PathBuf::from("/test"),
            expected: "aaa".to_string(),
            actual: "bbb".to_string(),
        },
        CoreError::DependencyCycle("mod_x".to_string()),
        CoreError::FileConflict {
            path: "shared.esp".to_string(),
            mods: Box::new(smallvec::smallvec!["a".into(), "b".into()]),
        },
        CoreError::ProfileNotFound("missing".to_string()),
        CoreError::ProfileAlreadyExists("dup".to_string()),
        CoreError::GameNotDetected("unknown".to_string()),
        CoreError::UnsupportedFs("ntfs thing".into()),
        CoreError::Other("something went wrong".into()),
    ];

    for err in &errors {
        let msg = format!("{err}");
        assert!(!msg.is_empty(), "error display should not be empty");
    }

    // Verify specific messages
    assert!(format!("{}", errors[0]).contains("IO error"));
    assert!(format!("{}", errors[2]).contains("hash mismatch"));
    assert!(format!("{}", errors[3]).contains("dependency cycle"));
    assert!(format!("{}", errors[4]).contains("conflict"));
    assert!(format!("{}", errors[5]).contains("not found"));
    assert!(format!("{}", errors[6]).contains("already exists"));
    assert!(format!("{}", errors[7]).contains("not detected"));
    assert!(format!("{}", errors[8]).contains("unsupported"));
}

#[test]
fn smoke_core_error_from_io_error() {
    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let core_err: CoreError = io_err.into();
    assert!(format!("{core_err}").contains("denied"));
}

// ── StockGameManager ─────────────────────────────────────────────────

#[test]
fn smoke_stock_game_manager_new() {
    let tmp = TempDir::new().unwrap();
    let _mgr = StockGameManager::new(tmp.path().to_path_buf());
    // Construction should not panic or fail
}

#[test]
fn smoke_stock_game_manager_detect_nonexistent() {
    let tmp = TempDir::new().unwrap();
    let mgr = StockGameManager::new(tmp.path().to_path_buf());
    // detect_steam_install looks at the Steam common path, which won't exist in test
    let result = mgr.detect_steam_install("NonExistentGame12345");
    assert!(result.is_none());
}

// ── CollectionManifest serialization roundtrip ───────────────────────

#[test]
fn smoke_collection_manifest_serialize_roundtrip() {
    let manifest = CollectionManifest {
        slug: "roundtrip".to_string(),
        name: "Roundtrip Test".to_string(),
        summary: Some("A summary".to_string()),
        description: None,
        author: CollectionAuthor {
            name: "Author".to_string(),
            member_id: Some(1),
        },
        game: CollectionGame {
            id: 42,
            domain_name: "testgame".to_string(),
            name: "Test Game".to_string(),
        },
        mods: vec![],
        version: CollectionVersion {
            version: "1.0".to_string(),
            created_at: None,
        },
        endorsements: 0,
        image_url: None,
    };

    let json = serde_json::to_string(&manifest).unwrap();
    let parsed: CollectionManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.slug, "roundtrip");
    assert_eq!(parsed.summary, Some("A summary".to_string()));
}

// ── WabbajackManifest download_directives / install_directives ──────

#[test]
fn smoke_wabbajack_download_directives_empty() {
    let manifest = WabbajackManifest {
        name: "empty".into(),
        author: "a".into(),
        description: "d".into(),
        game: "g".into(),
        version: "1".into(),
        archives: vec![],
        directives: vec![],
    };
    assert!(manifest.download_directives().is_empty());
    assert!(manifest.install_directives().is_empty());
}
