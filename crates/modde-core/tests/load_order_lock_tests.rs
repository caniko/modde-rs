//! Tests for the load order lock feature.
//!
//! Covers the *data model and persistence* surface that lives in
//! `modde-core` — the type round-trip, DB schema V7 migration, DB
//! round-trip for both profile-level and per-mod locks, `fork()`
//! clone semantics, helper unification, and the `import_toml_profiles`
//! preserve-before-overwrite contract.
//!
//! Tests for reorder-refusal enforcement, install-time lock acquisition,
//! scan retroactive ordering, and CLI/UI wiring live (or will live) in
//! their respective crates — see `/home/can/.claude/plans/greedy-shimmying-pine.md`
//! for the full verification matrix.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::OnceLock;

use modde_core::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, WabbajackManifest, cache_wabbajack_file, compute_manifest_hash,
};
use modde_core::profile::{
    EnabledMod, LoadOrderLock, LockReason, Profile, ProfileManager, ProfileSource,
};
use modde_core::scanner::{apply_wabbajack_lock, archive_mod_id, manifest_directive_order};
use modde_core::{GameId, ModdeDb};
use smallvec::smallvec;

// ---------------------------------------------------------------------------
// Test isolation
// ---------------------------------------------------------------------------
//
// `ProfileManager::fork` calls `SaveManager::fork_saves`, which writes to
// `modde_data_dir().join("saves/<game_id>/.git")`. Without isolation, this
// would pollute (and race against) the real `~/.local/share/modde` on the
// developer's machine. Redirect it to a per-test-binary tempdir via
// `paths::set_data_dir`, which is a `OnceLock` — calling it once from any
// test is enough because all tests in this file share the same process.
//
// The tempdir is held in a `OnceLock<tempfile::TempDir>` so it survives for
// the lifetime of the test binary; dropping it would delete the saves tree
// mid-test.

static ISOLATED_DATA_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();

/// Initialize the process-wide isolated data directory. `OnceLock` makes
/// the init closure run exactly once per process, so `set_data_dir`
/// (itself a `OnceLock` that panics on re-init) is invoked exactly once.
fn isolated_data_dir() {
    ISOLATED_DATA_DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("create isolated modde data dir for tests");
        modde_core::paths::set_data_dir(dir.path().to_path_buf());
        dir
    });
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

/// Parse the existing `wabbajack_manifest.json` fixture. It contains:
/// - One Nexus archive (hash `12345678901234`) referenced by two directives
///   (`FromArchive` and `PatchedFromArchive`) — so directive-order dedup is
///   exercised naturally.
/// - Two non-Nexus archives (GitHub + HTTP) that are NOT referenced by any
///   directive — so directive-order "only-referenced" semantics is exercised.
fn sample_manifest() -> WabbajackManifest {
    let fixture_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wabbajack_manifest.json");
    let json = std::fs::read_to_string(&fixture_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", fixture_path.display()));
    serde_json::from_str(&json).expect("parse wabbajack_manifest.json fixture")
}

fn make_profile(name: &str, game_id: &str, mods: Vec<EnabledMod>) -> Profile {
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

fn mod_entry(id: &str) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        display_name: Some(id.to_string()),
        enabled: true,
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// 1. Data model round-trip (types)
// ---------------------------------------------------------------------------

#[test]
fn lock_reason_serde_roundtrip_all_variants() {
    let cases = vec![
        LockReason::Wabbajack {
            manifest_hash: "deadbeef".to_string(),
        },
        LockReason::NexusCollection {
            slug: "my-collection".to_string(),
            version: "3.1.4".to_string(),
        },
        LockReason::TomlImport {
            source_path: "/tmp/profiles/ported/profile.toml".to_string(),
        },
        LockReason::Manual {
            note: Some("freeze for release build".to_string()),
        },
        LockReason::Manual { note: None },
    ];

    for reason in cases {
        let encoded = toml::to_string(&reason).expect("encode LockReason");
        let decoded: LockReason = toml::from_str(&encoded).expect("decode LockReason");
        assert_eq!(
            reason, decoded,
            "LockReason roundtrip mismatch for {reason:?}"
        );
    }
}

#[test]
fn load_order_lock_roundtrip() {
    let lock = LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "abc123".to_string(),
    });
    let encoded = toml::to_string(&lock).expect("encode LoadOrderLock");
    let decoded: LoadOrderLock = toml::from_str(&encoded).expect("decode LoadOrderLock");
    assert_eq!(lock, decoded);
    assert!(
        !decoded.locked_at.is_empty(),
        "locked_at should be populated"
    );
}

#[test]
fn profile_with_lock_toml_roundtrip() {
    // A serialized `Profile` written by one machine must round-trip through
    // TOML without losing the lock — this is the contract for TOML export
    // / import flows.
    let mut profile = make_profile("portable", "skyrim-se", vec![mod_entry("skse")]);
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "f00dface".to_string(),
    }));

    let encoded = toml::to_string(&profile).expect("serialize profile");
    let decoded: Profile = toml::from_str(&encoded).expect("deserialize profile");
    assert_eq!(profile.load_order_lock, decoded.load_order_lock);
}

#[test]
fn profile_without_lock_field_deserializes_as_none() {
    // Forward-compatibility check: a TOML profile written by a pre-V7 version
    // of modde has no `load_order_lock` key. Deserialization must default
    // it to `None` (via `#[serde(default)]`) rather than failing.
    let pre_v7_toml = r#"
name = "legacy"
game_id = "skyrim-se"
overrides = "/tmp/overrides"
source = "Manual"
mods = []
load_order_rules = []
"#;
    let profile: Profile = toml::from_str(pre_v7_toml).expect("decode pre-V7 profile");
    assert!(profile.load_order_lock.is_none());
}

// ---------------------------------------------------------------------------
// 2. Database round-trip for both profile-level and per-mod locks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn db_roundtrip_preserves_profile_level_lock() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mut profile = make_profile("wj-locked", "skyrim-se", vec![mod_entry("skse")]);
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "deadbeef".to_string(),
    }));

    pm.create(&profile).await.expect("create profile");
    let loaded = pm
        .load("wj-locked", Some(&GameId::from("skyrim-se")))
        .await
        .expect("load profile");

    assert_eq!(profile.load_order_lock, loaded.load_order_lock);
}

#[tokio::test]
async fn db_roundtrip_preserves_per_mod_lock() {
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mut pinned = mod_entry("SkyUI");
    pinned.lock = Some(LockReason::Manual {
        note: Some("pinned to top".to_string()),
    });
    let unpinned = mod_entry("USSEP");

    let profile = make_profile("mixed-pins", "skyrim-se", vec![pinned.clone(), unpinned]);
    pm.create(&profile).await.expect("create profile");

    let loaded = pm
        .load("mixed-pins", Some(&GameId::from("skyrim-se")))
        .await
        .expect("load profile");
    assert_eq!(loaded.mods.len(), 2);
    assert_eq!(loaded.mods[0].lock, pinned.lock);
    assert!(
        loaded.mods[1].lock.is_none(),
        "unpinned mod should load as None"
    );
}

#[tokio::test]
async fn db_update_preserves_lock_after_delete_reinsert() {
    // `update_profile` does a DELETE + INSERT for profile_mods — a known
    // shape that can silently drop columns if the INSERT statement is out
    // of sync with the struct. This test guards against regressions.
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mut pinned = mod_entry("SkyUI");
    pinned.lock = Some(LockReason::Manual { note: None });
    let mut profile = make_profile("pin-me", "skyrim-se", vec![pinned]);
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::NexusCollection {
        slug: "essentials".to_string(),
        version: "1.0".to_string(),
    }));

    pm.create(&profile).await.expect("create");
    // Mutate an unrelated field and call update.
    profile.overrides = PathBuf::from("/tmp/new-overrides");
    pm.update(&profile).await.expect("update");

    let loaded = pm
        .load("pin-me", Some(&GameId::from("skyrim-se")))
        .await
        .expect("reload");
    assert_eq!(loaded.load_order_lock, profile.load_order_lock);
    assert_eq!(loaded.mods[0].lock, profile.mods[0].lock);
}

#[tokio::test]
async fn db_roundtrip_none_lock_stays_none() {
    // Sanity: a profile with no lock and no per-mod pins must still load
    // back with None in both places — guards against sloppy Option
    // handling in decode_lock / decode_lock_reason.
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let profile = make_profile("plain", "skyrim-se", vec![mod_entry("skse")]);
    pm.create(&profile).await.expect("create");

    let loaded = pm
        .load("plain", Some(&GameId::from("skyrim-se")))
        .await
        .expect("load");
    assert!(loaded.load_order_lock.is_none());
    assert!(loaded.mods[0].lock.is_none());
}

// ---------------------------------------------------------------------------
// 3. Schema V7 migration is idempotent
// ---------------------------------------------------------------------------

#[tokio::test]
async fn schema_v7_migration_is_idempotent() {
    // Opening twice must not crash or double-add columns. `ModdeDb::open`
    // calls `migrate()` unconditionally, so round-tripping a connection is
    // the simplest way to exercise the idempotent guard.
    let db = ModdeDb::open_memory().await.expect("first open");
    // Create + load a profile to confirm the schema is usable.
    let pm = ProfileManager::with_db(db);
    let profile = make_profile("post-migration", "skyrim-se", vec![mod_entry("test")]);
    pm.create(&profile).await.expect("create after migration");
    let loaded = pm
        .load("post-migration", Some(&GameId::from("skyrim-se")))
        .await
        .unwrap();
    assert_eq!(loaded.mods.len(), 1);
}

// ---------------------------------------------------------------------------
// 4. Fork clones both profile-level and per-mod locks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fork_clones_both_profile_level_and_per_mod_locks() {
    // Merged into a single test because `ProfileManager::fork` writes to
    // the shared save vault git repo (`<data>/saves/<game_id>/.git`) and
    // parallel tests forking the same game_id race on git lockfiles. One
    // test covers both cases; splitting buys no clarity.
    isolated_data_dir();

    // Use a game_id unique to this test so the vault path doesn't collide
    // with other tests that may touch Skyrim saves.
    let game = GameId::from("skyrim-se-lock-fork-test");
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mut pinned = mod_entry("SkyUI");
    pinned.lock = Some(LockReason::Manual {
        note: Some("keep me".to_string()),
    });
    let mut profile = make_profile(
        "lock-src",
        game.as_str(),
        vec![pinned.clone(), mod_entry("USSEP")],
    );
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "src-hash".to_string(),
    }));
    pm.create(&profile).await.expect("create source");

    let _new_id = pm.fork("lock-src", "lock-fork", &game).await.expect("fork");

    let forked = pm.load("lock-fork", Some(&game)).await.expect("load fork");

    // Profile-level lock rides along (faithful-copy principle).
    assert_eq!(
        forked.load_order_lock, profile.load_order_lock,
        "fork must clone the profile-level lock verbatim"
    );
    // Per-mod locks ride along because they live on EnabledMod.
    assert_eq!(
        forked.mods[0].lock, pinned.lock,
        "fork must clone per-mod locks"
    );
    assert!(
        forked.mods[1].lock.is_none(),
        "unpinned mods must fork with no lock"
    );
}

#[tokio::test]
async fn fork_with_options_unlock_strips_both_lock_levels() {
    // `modde profile fork --unlock` (and `ForkOptions { unlock: true }`
    // via the library API) must strip BOTH the profile-level
    // `load_order_lock` AND every per-mod pin from the new profile,
    // without touching the source. This is the "fork to diverge"
    // workflow.
    use modde_core::profile::ForkOptions;
    isolated_data_dir();

    // Unique game_id to avoid save-vault collisions with other fork tests.
    let game = GameId::from("skyrim-se-fork-unlock-test");
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mut pinned_a = mod_entry("SkyUI");
    pinned_a.lock = Some(LockReason::Manual {
        note: Some("pinned".to_string()),
    });
    let mut pinned_b = mod_entry("USSEP");
    pinned_b.lock = Some(LockReason::Wabbajack {
        manifest_hash: "src".to_string(),
    });

    let mut source = make_profile(
        "wj-src",
        game.as_str(),
        vec![pinned_a, pinned_b, mod_entry("Free")],
    );
    source.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "src".to_string(),
    }));
    pm.create(&source).await.expect("create source");

    let _ = pm
        .fork_with_options("wj-src", "wj-diverged", &game, ForkOptions { unlock: true })
        .await
        .expect("fork --unlock");

    let forked = pm
        .load("wj-diverged", Some(&game))
        .await
        .expect("load fork");

    // Profile-level lock stripped.
    assert!(
        forked.load_order_lock.is_none(),
        "fork --unlock must strip profile-level lock; got {:?}",
        forked.load_order_lock
    );
    // Every per-mod pin stripped.
    for m in &forked.mods {
        assert!(
            m.lock.is_none(),
            "fork --unlock must strip per-mod pins; '{}' still has {:?}",
            m.mod_id,
            m.lock
        );
    }
    // Mods are still cloned (count and order preserved).
    assert_eq!(forked.mods.len(), 3);
    assert_eq!(forked.mods[0].mod_id, "SkyUI");
    assert_eq!(forked.mods[1].mod_id, "USSEP");
    assert_eq!(forked.mods[2].mod_id, "Free");

    // Source is untouched — critical invariant so users can "try a
    // diverged fork" without losing the locked original.
    let source_reloaded = pm.load("wj-src", Some(&game)).await.expect("reload source");
    assert!(
        source_reloaded.load_order_lock.is_some(),
        "fork --unlock must NOT touch the source profile's lock"
    );
    assert!(
        source_reloaded.mods[0].lock.is_some(),
        "fork --unlock must NOT touch source per-mod pins"
    );
}

#[tokio::test]
async fn fork_with_options_default_matches_legacy_fork() {
    // `ForkOptions::default()` (all false) must produce the same
    // result as the legacy `fork()` path — otherwise the wrapper is
    // breaking behavior for every existing caller.
    use modde_core::profile::ForkOptions;
    isolated_data_dir();

    let game = GameId::from("skyrim-se-fork-default-test");
    let pm = ProfileManager::with_db(ModdeDb::open_memory().await.unwrap());

    let mut pinned = mod_entry("SkyUI");
    pinned.lock = Some(LockReason::Manual { note: None });
    let mut source = make_profile("src-default", game.as_str(), vec![pinned.clone()]);
    source.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "abc".to_string(),
    }));
    pm.create(&source).await.expect("create source");

    let _ = pm
        .fork_with_options("src-default", "fork-default", &game, ForkOptions::default())
        .await
        .expect("fork default");

    let forked = pm
        .load("fork-default", Some(&game))
        .await
        .expect("load fork");
    assert_eq!(forked.load_order_lock, source.load_order_lock);
    assert_eq!(forked.mods[0].lock, pinned.lock);
}

// ---------------------------------------------------------------------------
// 5. TOML import preserve-before-overwrite
// ---------------------------------------------------------------------------

#[tokio::test]
async fn toml_import_stamps_tomlimport_when_no_existing_lock() {
    let tmp = tempfile::tempdir().expect("mktemp");
    let profile_dir = tmp.path().join("fresh-import");
    std::fs::create_dir_all(&profile_dir).unwrap();

    // Write a pre-V7-style TOML with no lock field.
    let toml = r#"
name = "fresh-import"
game_id = "skyrim-se"
overrides = "/tmp/overrides"
mods = []
load_order_rules = []

[source]
Manual = {}
"#;
    std::fs::write(profile_dir.join("profile.toml"), toml).unwrap();

    let db = ModdeDb::open_memory().await.unwrap();
    let imported = db.import_toml_profiles(tmp.path()).await.expect("import");
    assert_eq!(imported, 1);

    let profile = db
        .load_profile("fresh-import", &GameId::from("skyrim-se"))
        .await
        .expect("load imported");
    match profile.load_order_lock.as_ref().map(|l| &l.reason) {
        Some(LockReason::TomlImport { source_path }) => {
            assert!(
                source_path.contains("fresh-import"),
                "source_path should reference the imported file: {source_path}"
            );
        }
        other => panic!("expected TomlImport lock, got {other:?}"),
    }
}

#[tokio::test]
async fn toml_import_preserves_existing_wabbajack_lock() {
    // A TOML file that was exported from a Wabbajack-installed profile
    // already carries a `Wabbajack` lock. Importing it must preserve that
    // lock rather than overwriting with `TomlImport` — provenance wins.
    let tmp = tempfile::tempdir().expect("mktemp");
    let profile_dir = tmp.path().join("from-wj");
    std::fs::create_dir_all(&profile_dir).unwrap();

    // Build a profile with a Wabbajack lock, serialize it, write to disk.
    let mut source = make_profile("from-wj", "skyrim-se", vec![mod_entry("skse")]);
    source.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "original-wj-hash".to_string(),
    }));
    let toml = toml::to_string(&source).expect("serialize source");
    std::fs::write(profile_dir.join("profile.toml"), toml).unwrap();

    let db = ModdeDb::open_memory().await.unwrap();
    let imported = db.import_toml_profiles(tmp.path()).await.expect("import");
    assert_eq!(imported, 1);

    let loaded = db
        .load_profile("from-wj", &GameId::from("skyrim-se"))
        .await
        .expect("load");
    match loaded.load_order_lock.as_ref().map(|l| &l.reason) {
        Some(LockReason::Wabbajack { manifest_hash }) => {
            assert_eq!(manifest_hash, "original-wj-hash");
        }
        other => panic!("TOML import must preserve existing Wabbajack lock, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// 6. Manifest-hash helper determinism (compute_manifest_hash)
// ---------------------------------------------------------------------------

#[test]
fn compute_manifest_hash_is_deterministic() {
    let manifest = sample_manifest();
    let h1 = compute_manifest_hash(&manifest);
    let h2 = compute_manifest_hash(&manifest);
    assert_eq!(h1, h2, "same manifest must hash to same value");
    assert!(!h1.is_empty(), "hash should be non-empty");
}

#[test]
fn compute_manifest_hash_changes_with_version() {
    let mut a = sample_manifest();
    let b = sample_manifest();
    a.version = "2.0.0".to_string();
    assert_ne!(
        compute_manifest_hash(&a),
        compute_manifest_hash(&b),
        "different version must produce different hash"
    );
}

#[test]
fn compute_manifest_hash_changes_with_name() {
    let mut a = sample_manifest();
    let b = sample_manifest();
    a.name = "Different Modlist".to_string();
    assert_ne!(
        compute_manifest_hash(&a),
        compute_manifest_hash(&b),
        "different name must produce different hash"
    );
}

// ---------------------------------------------------------------------------
// 7. archive_mod_id helper — canonical unified derivation (Bug 2 fix)
// ---------------------------------------------------------------------------

#[test]
fn archive_mod_id_nexus_uses_nexus_prefix() {
    let nexus = ArchiveEntry {
        hash: 42,
        name: "foo.7z".to_string(),
        size: 100,
        state: Some(ArchiveState::NexusDownloader {
            game_name: "skyrimspecialedition".to_string(),
            mod_id: 1000.into(),
            file_id: 2000.into(),
        }),
    };
    assert_eq!(
        archive_mod_id(&nexus),
        "nexus_skyrimspecialedition_1000_2000"
    );
}

#[test]
fn archive_mod_id_non_nexus_uses_wj_prefix() {
    let http = ArchiveEntry {
        hash: 12345,
        name: "foo.zip".to_string(),
        size: 100,
        state: Some(ArchiveState::HttpDownloader {
            url: "https://example.com/foo.zip".to_string(),
            headers: Default::default(),
        }),
    };
    assert_eq!(archive_mod_id(&http), "wj_12345");
}

#[test]
fn archive_mod_id_stateless_archive_uses_wj_prefix() {
    // An archive with no `state` (rare but possible — stripped manifests)
    // should fall back to the hash-based id rather than panicking.
    let bare = ArchiveEntry {
        hash: 999,
        name: "mystery.bin".to_string(),
        size: 0,
        state: None,
    };
    assert_eq!(archive_mod_id(&bare), "wj_999");
}

// ---------------------------------------------------------------------------
// 8. manifest_directive_order — first-appearance semantics
// ---------------------------------------------------------------------------

#[test]
fn manifest_directive_order_dedupes_multiple_references() {
    // The sample manifest references the Nexus archive (hash=12345678901234)
    // in TWO directives (FromArchive + PatchedFromArchive). The returned
    // order must contain that mod_id exactly once.
    let manifest = sample_manifest();
    let order = manifest_directive_order(&manifest);

    assert_eq!(
        order.len(),
        1,
        "expected single dedup'd mod_id, got {order:?}"
    );
    assert_eq!(order[0], "nexus_skyrimspecialedition_42_100");
}

#[test]
fn manifest_directive_order_only_includes_referenced_archives() {
    // The github + http archives are in `manifest.archives` but have no
    // directives referencing them — they must NOT appear in the order.
    let manifest = sample_manifest();
    let order = manifest_directive_order(&manifest);
    let order_set: HashSet<&String> = order.iter().collect();
    assert!(!order_set.contains(&"wj_98765432109876".to_string()));
    assert!(!order_set.contains(&"wj_11111111111111".to_string()));
}

// ---------------------------------------------------------------------------
// 9. Install + scan mod_id unification (Bug 2 regression guard)
// ---------------------------------------------------------------------------

#[test]
fn nexus_archive_mod_id_matches_between_install_and_scan() {
    // Before the Bug 2 fix, the install flow produced `wj_{hash}` for all
    // archives while the scanner produced `nexus_{domain}_{mod}_{file}` for
    // Nexus-sourced ones. After unification, calling `archive_mod_id` on the
    // same ArchiveEntry from both contexts yields the same string.
    //
    // We assert the Nexus case specifically because it's the one that was
    // diverging — that's what made `modde install wabbajack` followed by
    // `modde scan --manifest <same>` create duplicates.
    let manifest = sample_manifest();
    let nexus = &manifest.archives[0];
    let id_from_install_perspective = archive_mod_id(nexus);
    let id_from_scanner_perspective = archive_mod_id(nexus);
    assert_eq!(id_from_install_perspective, id_from_scanner_perspective);
    assert!(id_from_install_perspective.starts_with("nexus_"));
}

// ---------------------------------------------------------------------------
// 10. apply_wabbajack_lock — retroactive reorder + lock
// ---------------------------------------------------------------------------
//
// The pure helper that powers `modde scan --manifest`. These guard the
// "preserve count, reorder matched, append unmatched, stamp lock"
// invariants — which are load-bearing for not losing user data during
// retroactive lock flows.

#[test]
fn apply_wabbajack_lock_preserves_mod_count() {
    // Regression guard: on 2026-04-10 we nearly shipped scan.rs where the
    // retroactive flow could drop mods because of a logic error. The
    // invariant is: matched + unmatched == original count, always.
    let manifest = sample_manifest();
    // Sample manifest contains 1 directive-referenced mod_id:
    //   nexus_skyrimspecialedition_42_100
    let matched_id = "nexus_skyrimspecialedition_42_100";
    let mut profile = make_profile(
        "retro",
        "skyrim-se",
        vec![
            mod_entry("unmatched_first"),
            mod_entry(matched_id),
            mod_entry("unmatched_middle"),
            mod_entry("unmatched_last"),
        ],
    );

    let report = apply_wabbajack_lock(&mut profile, &manifest);

    assert_eq!(profile.mods.len(), 4, "mod count must be preserved");
    assert_eq!(report.matched, 1);
    assert_eq!(report.unmatched, 3);
    assert!(!report.replaced_existing_lock);
}

#[test]
fn apply_wabbajack_lock_puts_matched_first_in_manifest_order() {
    let manifest = sample_manifest();
    let matched_id = "nexus_skyrimspecialedition_42_100";
    let mut profile = make_profile(
        "retro",
        "skyrim-se",
        vec![
            mod_entry("unmatched_a"),
            mod_entry("unmatched_b"),
            mod_entry(matched_id),
        ],
    );

    apply_wabbajack_lock(&mut profile, &manifest);

    assert_eq!(
        profile.mods[0].mod_id, matched_id,
        "matched mods must come first"
    );
    // Unmatched preserve original relative order.
    assert_eq!(profile.mods[1].mod_id, "unmatched_a");
    assert_eq!(profile.mods[2].mod_id, "unmatched_b");
}

#[test]
fn apply_wabbajack_lock_stamps_wabbajack_lock_reason() {
    let manifest = sample_manifest();
    let mut profile = make_profile("retro", "skyrim-se", vec![mod_entry("anything")]);

    let report = apply_wabbajack_lock(&mut profile, &manifest);

    let lock = profile
        .load_order_lock
        .as_ref()
        .expect("lock should be set");
    match &lock.reason {
        LockReason::Wabbajack { manifest_hash } => {
            assert_eq!(*manifest_hash, report.manifest_hash);
            assert_eq!(*manifest_hash, compute_manifest_hash(&manifest));
        }
        other => panic!("expected Wabbajack lock reason, got {other:?}"),
    }
    assert_ne!(lock.locked_at, "");
}

#[test]
fn apply_wabbajack_lock_overwrites_prior_lock_and_reports_it() {
    let manifest = sample_manifest();
    let mut profile = make_profile("retro", "skyrim-se", vec![mod_entry("x")]);
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Manual {
        note: Some("pre-existing".to_string()),
    }));

    let report = apply_wabbajack_lock(&mut profile, &manifest);

    assert!(
        report.replaced_existing_lock,
        "should report that an existing lock was replaced"
    );
    assert!(matches!(
        profile.load_order_lock.as_ref().unwrap().reason,
        LockReason::Wabbajack { .. }
    ));
}

#[test]
fn apply_wabbajack_lock_is_idempotent() {
    // Running the helper twice on the same profile must converge to the
    // same final state (modulo the lock's `locked_at` timestamp). This
    // mirrors running `modde scan --manifest` twice — which the user may
    // do after updating on-disk files.
    let manifest = sample_manifest();
    let mut p1 = make_profile(
        "retro",
        "skyrim-se",
        vec![
            mod_entry("nexus_skyrimspecialedition_42_100"),
            mod_entry("leftover_a"),
            mod_entry("leftover_b"),
        ],
    );
    let mut p2 = p1.clone();

    apply_wabbajack_lock(&mut p1, &manifest);
    apply_wabbajack_lock(&mut p2, &manifest);
    apply_wabbajack_lock(&mut p2, &manifest); // second pass

    let ids1: Vec<&str> = p1.mods.iter().map(|m| m.mod_id.as_str()).collect();
    let ids2: Vec<&str> = p2.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(
        ids1, ids2,
        "mod order should be stable under re-application"
    );
}

// ---------------------------------------------------------------------------
// 11. try_reorder — reorder refusal enforcement
// ---------------------------------------------------------------------------
//
// `try_reorder` is the single source of truth for reorder permission
// checks, called by the UI message handler and (eventually) the CLI.
// These tests pin down every refusal path plus the successful swap.

use modde_core::profile::{ReorderDirection, ReorderError, try_reorder};

fn three_mod_profile() -> Profile {
    make_profile(
        "reorder-test",
        "skyrim-se",
        vec![mod_entry("a"), mod_entry("b"), mod_entry("c")],
    )
}

#[test]
fn try_reorder_swaps_unlocked_mods_up() {
    let mut profile = three_mod_profile();
    try_reorder(&mut profile, "b", ReorderDirection::Up).expect("unlocked swap should succeed");
    let ids: Vec<&str> = profile.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(ids, vec!["b", "a", "c"]);
}

#[test]
fn try_reorder_swaps_unlocked_mods_down() {
    let mut profile = three_mod_profile();
    try_reorder(&mut profile, "a", ReorderDirection::Down).expect("unlocked swap should succeed");
    let ids: Vec<&str> = profile.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(ids, vec!["b", "a", "c"]);
}

#[test]
fn try_reorder_refuses_when_profile_locked() {
    // Regression guard for the core "hard block" enforcement: when a
    // profile carries ANY `load_order_lock`, every reorder attempt must
    // be refused with a structured reason — even reorders that would
    // otherwise be legal (unpinned mod, in bounds).
    let mut profile = three_mod_profile();
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: "test".to_string(),
    }));

    let err = try_reorder(&mut profile, "b", ReorderDirection::Up).unwrap_err();
    match err {
        ReorderError::ProfileLocked {
            reason: LockReason::Wabbajack { manifest_hash },
        } => {
            assert_eq!(manifest_hash, "test");
        }
        other => panic!("expected ProfileLocked(Wabbajack), got {other:?}"),
    }

    // Mod order must be untouched on refusal.
    let ids: Vec<&str> = profile.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b", "c"]);
}

#[test]
fn try_reorder_refuses_when_target_mod_pinned() {
    let mut profile = three_mod_profile();
    profile.mods[1].lock = Some(LockReason::Manual {
        note: Some("hold".to_string()),
    });

    let err = try_reorder(&mut profile, "b", ReorderDirection::Down).unwrap_err();
    match err {
        ReorderError::ModPinned {
            mod_id,
            reason: LockReason::Manual { .. },
        } => {
            assert_eq!(mod_id, "b");
        }
        other => panic!("expected ModPinned(Manual), got {other:?}"),
    }
    // No mutation on refusal.
    assert_eq!(profile.mods[1].mod_id, "b");
}

#[test]
fn try_reorder_refuses_when_adjacent_mod_pinned() {
    // Moving an unpinned mod past a pinned neighbor would silently
    // shift the pinned mod's absolute position — violating the per-mod
    // pin contract. `try_reorder` refuses this, pointing at the pinned
    // neighbor so callers can explain the refusal.
    let mut profile = three_mod_profile();
    profile.mods[1].lock = Some(LockReason::Manual { note: None });

    // Attempt: move "a" down — swap partner is "b" which is pinned.
    let err = try_reorder(&mut profile, "a", ReorderDirection::Down).unwrap_err();
    match err {
        ReorderError::AdjacentPinned { neighbor_id, .. } => {
            assert_eq!(neighbor_id, "b");
        }
        other => panic!("expected AdjacentPinned, got {other:?}"),
    }
    assert_eq!(profile.mods[0].mod_id, "a");
}

#[test]
fn try_reorder_refuses_when_mod_not_found() {
    let mut profile = three_mod_profile();
    let err = try_reorder(&mut profile, "does-not-exist", ReorderDirection::Up).unwrap_err();
    assert!(matches!(err, ReorderError::ModNotFound { mod_id } if mod_id == "does-not-exist"));
}

#[test]
fn try_reorder_refuses_at_top_boundary() {
    let mut profile = three_mod_profile();
    let err = try_reorder(&mut profile, "a", ReorderDirection::Up).unwrap_err();
    assert_eq!(err, ReorderError::AtBoundary);
}

#[test]
fn try_reorder_refuses_at_bottom_boundary() {
    let mut profile = three_mod_profile();
    let err = try_reorder(&mut profile, "c", ReorderDirection::Down).unwrap_err();
    assert_eq!(err, ReorderError::AtBoundary);
}

#[test]
fn try_reorder_refuses_even_when_only_per_mod_lock_set_with_profile_lock() {
    // Precedence test: if BOTH profile-level and per-mod locks are set,
    // `try_reorder` reports the profile-level lock first (it's the
    // coarser refusal and short-circuits earlier). The handler can
    // therefore surface "unlock the profile" rather than "unpin this mod"
    // — more actionable when everything is locked.
    let mut profile = three_mod_profile();
    profile.mods[1].lock = Some(LockReason::Manual { note: None });
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::NexusCollection {
        slug: "x".to_string(),
        version: "1".to_string(),
    }));

    let err = try_reorder(&mut profile, "b", ReorderDirection::Up).unwrap_err();
    assert!(
        matches!(err, ReorderError::ProfileLocked { .. }),
        "profile-level lock must short-circuit before per-mod lock check"
    );
}

// ---------------------------------------------------------------------------
// 12. detect_stale_duplicates — filesystem-scanner dedup against manifest
// ---------------------------------------------------------------------------
//
// Regression guard for the 2026-04-10 profile 3077 incident: 166 leaked
// `cet/*` rows coexisted with their `nexus_*` counterparts because the old
// per-file coverage check in scan.rs failed when CET directories contained
// runtime-generated config files the manifest didn't deploy. The fix moved
// coverage to a directory-prefix check AND introduced this helper so
// existing profiles can be cleaned up after the fact.
//
// The sample manifest deploys files to:
//   - Data/textures/test.dds
//   - Data/meshes/test.nif
// i.e. covered dirs include `data/textures/`, `data/meshes/`, `data/`.

use modde_core::scanner::{DuplicateReport, ModFootprint, detect_stale_duplicates};

/// Helper: Cyberpunk-style prefix → footprint mapping. Mirrors
/// `modde_games::cyberpunk::scanner::mod_id_footprint` but kept inline
/// here so modde-core tests don't have to depend on modde-games.
fn cp_footprint(mod_id: &str) -> Option<ModFootprint> {
    if let Some(name) = mod_id.strip_prefix("cet/") {
        Some(ModFootprint::Directory(format!(
            "bin/x64/plugins/cyber_engine_tweaks/mods/{}/",
            name.to_lowercase()
        )))
    } else {
        mod_id.strip_prefix("archive/").map(|stem| {
            ModFootprint::File(format!("archive/pc/mod/{}.archive", stem.to_lowercase()))
        })
    }
}

/// Helper: footprint mapping for the Skyrim-style paths in the sample
/// manifest fixture. Lets us exercise directory and file matches against
/// the fixture's actual `Data/textures/` / `Data/meshes/` directive
/// paths without bringing in a CP2077 manifest.
fn skyrim_test_footprint(mod_id: &str) -> Option<ModFootprint> {
    if let Some(rest) = mod_id.strip_prefix("dir/") {
        Some(ModFootprint::Directory(format!(
            "data/{}/",
            rest.to_lowercase()
        )))
    } else {
        mod_id
            .strip_prefix("file/")
            .map(|rest| ModFootprint::File(rest.to_lowercase()))
    }
}

#[test]
fn detect_stale_duplicates_flags_directory_overlap_as_leaked() {
    // `dir/textures` → `data/textures/` which IS one of the manifest's
    // covered directories (via the FromArchive directive for test.dds).
    // Classification: LEAKED.
    let manifest = sample_manifest();
    let profile = make_profile(
        "3077-like",
        "skyrim-se",
        vec![
            mod_entry("nexus_skyrimspecialedition_42_100"), // canonical row
            mod_entry("dir/textures"),                      // leaked duplicate
        ],
    );

    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);
    assert_eq!(report.leaked, vec!["dir/textures"]);
    assert_eq!(report.genuine, [] as [std::string::String; 0]);
}

#[test]
fn detect_stale_duplicates_preserves_genuine_additions() {
    // `dir/notinmanifest` → `data/notinmanifest/` which the manifest does
    // NOT cover. Classification: GENUINE — the user added this on top.
    let manifest = sample_manifest();
    let profile = make_profile(
        "mixed",
        "skyrim-se",
        vec![
            mod_entry("dir/textures"),      // leaked (covered)
            mod_entry("dir/notinmanifest"), // genuine (not covered)
        ],
    );

    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);
    assert_eq!(report.leaked, vec!["dir/textures"]);
    assert_eq!(report.genuine, vec!["dir/notinmanifest"]);
}

#[test]
fn detect_stale_duplicates_handles_file_footprints() {
    // File footprints are checked against the manifest's exact `To` paths,
    // not the directory prefix set. This mirrors `archive/<stem>` mods on
    // Cyberpunk 2077, which are single `.archive` files.
    let manifest = sample_manifest();
    let profile = make_profile(
        "file-footprint",
        "skyrim-se",
        vec![
            mod_entry("file/data/textures/test.dds"), // exact match → leaked
            mod_entry("file/data/other.dds"),         // not in manifest → genuine
        ],
    );

    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);
    assert_eq!(report.leaked, vec!["file/data/textures/test.dds"]);
    assert_eq!(report.genuine, vec!["file/data/other.dds"]);
}

#[test]
fn detect_stale_duplicates_skips_manifest_authored_rows() {
    // `nexus_*` and `wj_*` rows are authored by the manifest flow itself
    // — they must NEVER be classified as "duplicates of themselves".
    // The closure returning None is the gate: mod_ids it doesn't
    // recognise are silently skipped (neither leaked nor genuine).
    let manifest = sample_manifest();
    let profile = make_profile(
        "manifest-only",
        "skyrim-se",
        vec![
            mod_entry("nexus_skyrimspecialedition_42_100"),
            mod_entry("wj_98765432109876"),
        ],
    );

    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);
    assert_eq!(report.leaked, [] as [std::string::String; 0]);
    assert_eq!(report.genuine, [] as [std::string::String; 0]);
}

#[test]
fn detect_stale_duplicates_report_partitions_cleanly() {
    // All four classifications at once, on a realistic profile shape:
    // one canonical row, one leaked duplicate, one genuine addition, and
    // one manifest-authored row that must be ignored entirely.
    let manifest = sample_manifest();
    let profile = make_profile(
        "realistic",
        "skyrim-se",
        vec![
            mod_entry("nexus_skyrimspecialedition_42_100"), // skipped (no footprint)
            mod_entry("wj_98765432109876"),                 // skipped (no footprint)
            mod_entry("dir/textures"),                      // leaked
            mod_entry("dir/meshes"),                        // leaked (same covered set)
            mod_entry("dir/usermod"),                       // genuine
            mod_entry("file/data/textures/test.dds"),       // leaked (exact file match)
            mod_entry("file/data/usermod.esp"),             // genuine
        ],
    );

    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);

    let leaked: HashSet<String> = report.leaked.into_iter().collect();
    let genuine: HashSet<String> = report.genuine.into_iter().collect();

    assert_eq!(
        leaked,
        HashSet::from([
            "dir/textures".to_string(),
            "dir/meshes".to_string(),
            "file/data/textures/test.dds".to_string(),
        ])
    );
    assert_eq!(
        genuine,
        HashSet::from([
            "dir/usermod".to_string(),
            "file/data/usermod.esp".to_string(),
        ])
    );
}

#[test]
fn detect_stale_duplicates_is_case_insensitive() {
    // Manifest paths are mixed-case (`Data/textures/test.dds` in the
    // fixture), filesystem scanners produce their own casing, and users
    // might type either. The helper lowercases both sides so equality
    // doesn't depend on how the path was captured.
    let manifest = sample_manifest();
    let profile = make_profile(
        "casey",
        "skyrim-se",
        vec![mod_entry("dir/TEXTURES")], // uppercase
    );
    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);
    assert_eq!(report.leaked, vec!["dir/TEXTURES"]);
}

#[test]
fn detect_stale_duplicates_empty_profile_returns_empty_report() {
    let manifest = sample_manifest();
    let profile = make_profile("empty", "skyrim-se", vec![]);
    let report = detect_stale_duplicates(&profile, &manifest, skyrim_test_footprint);
    assert_eq!(report, DuplicateReport::default());
}

#[test]
fn detect_stale_duplicates_strips_mo2_prefix_from_directive_paths() {
    // Regression guard for profile 3077 (2026-04-10): Wabbajack `To` paths
    // are MO2-staged — `mods\<MO2 Mod Name>\<game-relative-path>`. The
    // classifier must strip the `mods/<name>/` prefix before comparing
    // against game-relative footprints. Without the strip step, a CP2077
    // modlist's directives all look like `mods/<huge mod name>/bin/...`
    // which never overlaps with the `bin/x64/...` footprint of a
    // filesystem-scanner row — every row gets misclassified as GENUINE
    // and no duplicates are ever pruned.
    let manifest_json = r#"{
      "Name": "MO2 Test",
      "Author": "test",
      "Description": "",
      "Game": "Cyberpunk2077",
      "Version": "1.0",
      "Archives": [
        {
          "Hash": 123,
          "Name": "archive.zip",
          "Size": 1,
          "State": {
            "$type": "NexusDownloader, Wabbajack.Lib",
            "GameName": "cyberpunk2077",
            "ModID": 1,
            "FileID": 1
          }
        }
      ],
      "Directives": [
        {
          "$type": "FromArchive, Wabbajack.Lib",
          "ArchiveHashPath": [123, "init.lua"],
          "To": "mods\\Immersive Healing\\bin\\x64\\plugins\\cyber_engine_tweaks\\mods\\ImmersiveHealing\\init.lua"
        }
      ]
    }"#;
    let manifest: WabbajackManifest =
        serde_json::from_str(manifest_json).expect("parse MO2 manifest");

    // Use the CP footprint mapping since the paths are Cyberpunk-style.
    let profile = make_profile(
        "3077-like",
        "cyberpunk2077",
        vec![
            mod_entry("cet/ImmersiveHealing"), // should be LEAKED
            mod_entry("cet/MyOwnCETMod"),      // should be GENUINE
        ],
    );

    let report = detect_stale_duplicates(&profile, &manifest, cp_footprint);
    assert_eq!(
        report.leaked,
        vec!["cet/ImmersiveHealing"],
        "CET dir deployed via MO2-staged path should classify as LEAKED after prefix strip"
    );
    assert_eq!(
        report.genuine,
        vec!["cet/MyOwnCETMod"],
        "a CET mod not in the manifest must stay GENUINE"
    );
}

#[test]
fn detect_stale_duplicates_closure_gate_short_circuits_cp_prefixes() {
    // Sanity check that the `cp_footprint` helper mirrors the production
    // Cyberpunk scanner's scheme (cet/, archive/) — same helper shape as
    // `modde_games::cyberpunk::scanner::mod_id_footprint`.
    // Against the *Skyrim* fixture, none of these paths overlap → genuine.
    let manifest = sample_manifest();
    let profile = make_profile(
        "cp-against-skyrim",
        "cyberpunk2077",
        vec![
            mod_entry("cet/ImmersiveHealing"),
            mod_entry("archive/foo"),
            mod_entry("nexus_cyberpunk2077_1_1"), // closure returns None → skipped
        ],
    );
    let report = detect_stale_duplicates(&profile, &manifest, cp_footprint);
    // cet/ and archive/ paths don't overlap with the Skyrim fixture's
    // `Data/textures/` / `Data/meshes/`, so both are genuine additions.
    let genuine: HashSet<String> = report.genuine.into_iter().collect();
    assert_eq!(
        genuine,
        HashSet::from([
            "cet/ImmersiveHealing".to_string(),
            "archive/foo".to_string(),
        ])
    );
    assert_eq!(report.leaked, [] as [std::string::String; 0]);
}

// ---------------------------------------------------------------------------
// Wabbajack source-file cache (content-addressed)
// ---------------------------------------------------------------------------

#[test]
fn cache_wabbajack_file_copies_and_is_content_addressed() {
    isolated_data_dir();

    // Write a dummy "source" wabbajack file in a scratch tempdir.
    let src_dir = tempfile::tempdir().unwrap();
    let src = src_dir.path().join("modlist.wabbajack");
    let payload = b"fake wabbajack payload bytes";
    std::fs::write(&src, payload).unwrap();

    let hash = "deadbeef00000001";
    let dest = cache_wabbajack_file(&src, hash).expect("cache copy succeeds");

    // Result matches the derived content-addressed path.
    assert_eq!(dest, modde_core::paths::wabbajack_cache_path(hash));

    // Destination exists and is byte-identical to the source.
    let cached = std::fs::read(&dest).unwrap();
    assert_eq!(cached, payload, "cached bytes must match source");
}

#[test]
fn cache_wabbajack_file_is_idempotent() {
    isolated_data_dir();

    let src_dir = tempfile::tempdir().unwrap();
    let first = src_dir.path().join("first.wabbajack");
    let second = src_dir.path().join("second.wabbajack");
    let first_bytes = b"first payload";
    let second_bytes = b"second payload -- DIFFERENT content";
    std::fs::write(&first, first_bytes).unwrap();
    std::fs::write(&second, second_bytes).unwrap();

    // Pick a hash distinct from any other test in this file to avoid
    // cross-test interference (the cache dir is process-wide).
    let hash = "deadbeef00000002";

    let dest1 = cache_wabbajack_file(&first, hash).unwrap();
    let dest2 = cache_wabbajack_file(&second, hash).unwrap();

    // Same derived path, both calls.
    assert_eq!(dest1, dest2);

    // Idempotent skip: the second call does NOT overwrite with `second_bytes`.
    // The hash is a stable content identifier — both install and scan can
    // call this helper without coordination and we rely on first-writer-wins.
    let cached = std::fs::read(&dest2).unwrap();
    assert_eq!(cached, first_bytes, "second call must not overwrite first");
}
