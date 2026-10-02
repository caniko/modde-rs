//! Manifest validation and parsing edge case tests.
//!
//! Tests `WabbajackManifest` and `CollectionManifest` serialization,
//! directive extraction, and edge cases in JSON parsing.

use std::collections::HashMap;

use modde_core::GameId;
use modde_core::manifest::collection::{
    CollectionAuthor, CollectionGame, CollectionManifest, CollectionMod, CollectionPatch,
    CollectionVersion,
};
use modde_core::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, BSAFileState, DownloadDirective, InstallDirective, RawDirective,
    WabbajackManifest,
};

fn test_manifest() -> WabbajackManifest {
    WabbajackManifest {
        name: "Test Modlist".to_string(),
        author: "Test Author".to_string(),
        description: "A test modlist".to_string(),
        game: "SkyrimSE".to_string(),
        version: "1.0.0".to_string(),
        archives: vec![],
        directives: vec![],
    }
}

// ── WabbajackManifest: download directives ─────────────────────────

#[test]
fn test_download_directives_all_types() {
    let manifest = WabbajackManifest {
        archives: vec![
            ArchiveEntry {
                hash: 100,
                name: "nexus.zip".to_string(),
                size: 1000,
                state: Some(ArchiveState::NexusDownloader {
                    game_name: "skyrimspecialedition".to_string(),
                    mod_id: 42.into(),
                    file_id: 99.into(),
                }),
            },
            ArchiveEntry {
                hash: 200,
                name: "github.zip".to_string(),
                size: 2000,
                state: Some(ArchiveState::GitHubDownloader {
                    user: "modder".to_string(),
                    repo: "my-mod".to_string(),
                    tag: "v1.0".to_string(),
                    asset: "release.zip".to_string(),
                }),
            },
            ArchiveEntry {
                hash: 300,
                name: "gdrive.zip".to_string(),
                size: 3000,
                state: Some(ArchiveState::GoogleDriveDownloader {
                    id: "1aBcDeF".to_string(),
                }),
            },
            ArchiveEntry {
                hash: 400,
                name: "mega.zip".to_string(),
                size: 4000,
                state: Some(ArchiveState::MegaDownloader {
                    url: "https://mega.nz/file/HANDLE#KEY".to_string(),
                }),
            },
            ArchiveEntry {
                hash: 500,
                name: "direct.zip".to_string(),
                size: 5000,
                state: Some(ArchiveState::HttpDownloader {
                    url: "https://example.com/mod.zip".to_string(),
                    headers: HashMap::from([(
                        "Authorization".to_string(),
                        "Bearer xyz".to_string(),
                    )]),
                }),
            },
        ],
        ..test_manifest()
    };

    let directives = manifest.download_directives();
    assert_eq!(directives.len(), 5);

    match &directives[0] {
        DownloadDirective::Nexus {
            game_id,
            mod_id,
            file_id,
            hash,
        } => {
            assert_eq!(game_id, "skyrimspecialedition");
            assert_eq!(mod_id.get(), 42);
            assert_eq!(file_id.get(), 99);
            assert_eq!(*hash, 100);
        }
        _ => panic!("expected Nexus"),
    }

    match &directives[1] {
        DownloadDirective::GitHub {
            user,
            repo,
            tag,
            asset,
            hash,
        } => {
            assert_eq!(user, "modder");
            assert_eq!(repo, "my-mod");
            assert_eq!(tag, "v1.0");
            assert_eq!(asset, "release.zip");
            assert_eq!(*hash, 200);
        }
        _ => panic!("expected GitHub"),
    }

    match &directives[2] {
        DownloadDirective::GoogleDrive { id, hash } => {
            assert_eq!(id, "1aBcDeF");
            assert_eq!(*hash, 300);
        }
        _ => panic!("expected GoogleDrive"),
    }

    match &directives[3] {
        DownloadDirective::Mega { url, hash } => {
            assert_eq!(url, "https://mega.nz/file/HANDLE#KEY");
            assert_eq!(*hash, 400);
        }
        _ => panic!("expected Mega"),
    }

    match &directives[4] {
        DownloadDirective::DirectURL {
            url, headers, hash, ..
        } => {
            assert_eq!(url, "https://example.com/mod.zip");
            assert_eq!(headers.get("Authorization").unwrap(), "Bearer xyz");
            assert_eq!(*hash, 500);
        }
        _ => panic!("expected DirectURL"),
    }
}

#[test]
fn test_download_directives_archives_without_state_filtered() {
    let manifest = WabbajackManifest {
        archives: vec![
            ArchiveEntry {
                hash: 100,
                name: "stateless.zip".to_string(),
                size: 1000,
                state: None,
            },
            ArchiveEntry {
                hash: 200,
                name: "has_state.zip".to_string(),
                size: 2000,
                state: Some(ArchiveState::NexusDownloader {
                    game_name: "skyrimse".to_string(),
                    mod_id: 1.into(),
                    file_id: 1.into(),
                }),
            },
        ],
        ..test_manifest()
    };

    let directives = manifest.download_directives();
    assert_eq!(directives.len(), 1);
}

#[test]
fn test_download_directives_empty_archives() {
    let manifest = test_manifest();
    assert!(manifest.download_directives().is_empty());
}

// ── WabbajackManifest: install directives ──────────────────────────

#[test]
fn test_install_directives_from_archive() {
    let manifest = WabbajackManifest {
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(12345.into()),
                serde_json::Value::String("data/textures/sky.dds".to_string()),
            ],
            to: "Data/textures/sky.dds".to_string(),
            size: 0,
        }],
        ..test_manifest()
    };

    let directives = manifest.install_directives();
    assert_eq!(directives.len(), 1);
    match &directives[0] {
        InstallDirective::FromArchive {
            archive_hash,
            from,
            inner_path,
            to,
            size,
        } => {
            assert_eq!(*archive_hash, 12345);
            assert_eq!(from, "data/textures/sky.dds");
            assert!(inner_path.is_none());
            assert_eq!(to, "Data/textures/sky.dds");
            assert_eq!(*size, 0);
        }
        _ => panic!("expected FromArchive"),
    }
}

#[test]
fn test_install_directives_patched() {
    let manifest = WabbajackManifest {
        directives: vec![RawDirective::PatchedFromArchive {
            patch_id: String::new(),
            archive_hash_path: vec![
                serde_json::Value::Number(55555.into()),
                serde_json::Value::String("original.esp".to_string()),
            ],
            to: "patched.esp".to_string(),
            hash: 99999,
            size: 12345,
        }],
        ..test_manifest()
    };

    let directives = manifest.install_directives();
    assert_eq!(directives.len(), 1);
    match &directives[0] {
        InstallDirective::PatchedFromArchive {
            archive_hash,
            from,
            inner_path,
            to,
            patch_id,
            size,
        } => {
            assert_eq!(*archive_hash, 55555);
            assert_eq!(from, "original.esp");
            assert!(inner_path.is_none());
            assert_eq!(to, "patched.esp");
            assert_eq!(patch_id, "");
            assert_eq!(*size, 12345);
        }
        _ => panic!("expected PatchedFromArchive"),
    }
}

#[test]
fn test_install_directives_nested_archive_hash_path() {
    let manifest = WabbajackManifest {
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(55555.into()),
                serde_json::Value::String("outer.bsa".to_string()),
                serde_json::Value::String("meshes/inner.nif".to_string()),
            ],
            to: "mods/out/inner.nif".to_string(),
            size: 42,
        }],
        ..test_manifest()
    };

    let directives = manifest.install_directives();
    match &directives[0] {
        InstallDirective::FromArchive {
            archive_hash,
            from,
            inner_path,
            to,
            size,
        } => {
            assert_eq!(*archive_hash, 55555);
            assert_eq!(from, "outer.bsa");
            assert_eq!(inner_path.as_deref(), Some("meshes/inner.nif"));
            assert_eq!(to, "mods/out/inner.nif");
            assert_eq!(*size, 42);
        }
        _ => panic!("expected FromArchive"),
    }
}

#[test]
fn test_install_directives_create_bsa() {
    let manifest = WabbajackManifest {
        directives: vec![RawDirective::CreateBSA {
            temp_id: "bsa_001".to_string(),
            to: "Data/Textures.bsa".to_string(),
            file_states: vec![
                BSAFileState {
                    path: "textures/sky.dds".to_string(),
                    hash: 111,
                    size: 1024,
                },
                BSAFileState {
                    path: "textures/ground.dds".to_string(),
                    hash: 222,
                    size: 2048,
                },
            ],
        }],
        ..test_manifest()
    };

    let directives = manifest.install_directives();
    assert_eq!(directives.len(), 1);
    match &directives[0] {
        InstallDirective::CreateBSA {
            temp_id,
            to,
            file_states,
        } => {
            assert_eq!(temp_id, "bsa_001");
            assert_eq!(to, "Data/Textures.bsa");
            assert_eq!(file_states.len(), 2);
        }
        _ => panic!("expected CreateBSA"),
    }
}

#[test]
fn test_install_directives_unknown_filtered_out() {
    let manifest = WabbajackManifest {
        directives: vec![
            RawDirective::Unknown,
            RawDirective::FromArchive {
                archive_hash_path: vec![serde_json::Value::Number(1.into())],
                to: "file.txt".to_string(),
                size: 0,
            },
            RawDirective::Unknown,
        ],
        ..test_manifest()
    };

    let directives = manifest.install_directives();
    assert_eq!(directives.len(), 1);
}

#[test]
fn test_install_directives_empty_archive_hash_path() {
    let manifest = WabbajackManifest {
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![], // empty
            to: "file.txt".to_string(),
            size: 0,
        }],
        ..test_manifest()
    };

    let directives = manifest.install_directives();
    assert_eq!(directives.len(), 1);
    match &directives[0] {
        InstallDirective::FromArchive {
            archive_hash, from, ..
        } => {
            assert_eq!(*archive_hash, 0); // defaults to 0
            assert_eq!(from, ""); // defaults to empty
        }
        _ => panic!("expected FromArchive"),
    }
}

// ── WabbajackManifest: JSON serialization roundtrip ────────────────

#[test]
fn test_wabbajack_manifest_json_roundtrip() {
    let manifest = WabbajackManifest {
        name: "Test List".to_string(),
        author: "Author".to_string(),
        description: "Desc".to_string(),
        game: "SkyrimSE".to_string(),
        version: "2.0".to_string(),
        archives: vec![ArchiveEntry {
            hash: 42,
            name: "mod.zip".to_string(),
            size: 12345,
            state: Some(ArchiveState::NexusDownloader {
                game_name: "skyrimse".to_string(),
                mod_id: 100.into(),
                file_id: 200.into(),
            }),
        }],
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(42.into()),
                serde_json::Value::String("inner.esp".to_string()),
            ],
            to: "Data/inner.esp".to_string(),
            size: 0,
        }],
    };

    let json = serde_json::to_string(&manifest).unwrap();
    let parsed: WabbajackManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, manifest.name);
    assert_eq!(parsed.archives.len(), 1);
    assert_eq!(parsed.directives.len(), 1);
}

// ── CollectionManifest tests ───────────────────────────────────────

#[test]
fn test_collection_manifest_full_parse() {
    let json = r#"{
        "slug": "ultimate-skyrim",
        "name": "Ultimate Skyrim",
        "summary": "The best collection",
        "description": "A comprehensive modlist for Skyrim SE.",
        "author": {"name": "ModMaster", "member_id": 12345},
        "game": {"id": 1704, "domain_name": "skyrimspecialedition", "name": "Skyrim Special Edition"},
        "mods": [
            {
                "mod_id": 2014,
                "file_id": 50000,
                "name": "SkyUI",
                "version": "5.2",
                "optional": false,
                "install_order": 1,
                "patch": null
            },
            {
                "mod_id": 3863,
                "file_id": 60000,
                "name": "USSEP",
                "version": "4.2.8",
                "optional": true,
                "install_order": 0,
                "patch": {"hash": "abc123", "url": "https://example.com/patch.bin"}
            }
        ],
        "version": {"version": "3.0.0", "created_at": "2025-01-15T00:00:00Z"},
        "endorsements": 9999,
        "image_url": "https://example.com/cover.jpg"
    }"#;

    let manifest: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.slug, "ultimate-skyrim");
    assert_eq!(manifest.name, "Ultimate Skyrim");
    assert_eq!(manifest.summary.as_deref(), Some("The best collection"));
    assert_eq!(manifest.author.name, "ModMaster");
    assert_eq!(manifest.author.member_id, Some(12345));
    assert_eq!(manifest.game.domain_name, "skyrimspecialedition");
    assert_eq!(manifest.mods.len(), 2);
    assert_eq!(manifest.mods[0].name, "SkyUI");
    assert!(!manifest.mods[0].optional);
    assert!(manifest.mods[1].optional);
    assert!(manifest.mods[1].patch.is_some());
    assert_eq!(manifest.endorsements, 9999);
    assert!(manifest.image_url.is_some());
}

#[test]
fn test_collection_manifest_minimal() {
    let json = r#"{
        "slug": "minimal",
        "name": "Minimal",
        "author": {"name": "Someone"},
        "game": {"id": 1, "domain_name": "skyrimse", "name": "Skyrim SE"},
        "version": {"version": "1.0"}
    }"#;

    let manifest: CollectionManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.slug, "minimal");
    assert!(manifest.summary.is_none());
    assert!(manifest.description.is_none());
    assert!(manifest.mods.is_empty());
    assert_eq!(manifest.endorsements, 0);
    assert!(manifest.image_url.is_none());
    assert!(manifest.version.created_at.is_none());
    assert!(manifest.author.member_id.is_none());
}

#[test]
fn test_collection_manifest_roundtrip() {
    let manifest = CollectionManifest {
        slug: "test-collection".to_string(),
        name: "Test Collection".to_string(),
        summary: Some("A test".to_string()),
        description: None,
        author: CollectionAuthor {
            name: "Tester".to_string(),
            member_id: Some(42),
        },
        game: CollectionGame {
            id: 1704,
            domain_name: "skyrimse".to_string(),
            name: "Skyrim SE".to_string(),
        },
        mods: vec![CollectionMod {
            mod_id: 100.into(),
            file_id: 200.into(),
            name: "TestMod".to_string(),
            version: "1.0".to_string(),
            optional: false,
            install_order: 0,
            patch: Some(CollectionPatch {
                hash: "deadbeef".to_string(),
                url: "https://example.com/patch".to_string(),
            }),
        }],
        version: CollectionVersion {
            version: "1.0.0".to_string(),
            created_at: Some("2025-06-01".to_string()),
        },
        endorsements: 50,
        image_url: Some("https://example.com/img.png".to_string()),
    };

    let json = serde_json::to_string(&manifest).unwrap();
    let parsed: CollectionManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.slug, manifest.slug);
    assert_eq!(parsed.mods.len(), 1);
    assert!(parsed.mods[0].patch.is_some());
}

#[test]
fn test_collection_mod_install_order_sorting() {
    let json = r#"{
        "slug": "ordered",
        "name": "Ordered",
        "author": {"name": "A"},
        "game": {"id": 1, "domain_name": "skyrimse", "name": "Skyrim SE"},
        "mods": [
            {"mod_id": 1, "file_id": 1, "name": "C", "version": "1", "install_order": 3},
            {"mod_id": 2, "file_id": 2, "name": "A", "version": "1", "install_order": 1},
            {"mod_id": 3, "file_id": 3, "name": "B", "version": "1", "install_order": 2}
        ],
        "version": {"version": "1.0"}
    }"#;

    let manifest: CollectionManifest = serde_json::from_str(json).unwrap();
    let mut sorted = manifest.mods.clone();
    sorted.sort_by_key(|m| m.install_order);
    assert_eq!(sorted[0].name, "A");
    assert_eq!(sorted[1].name, "B");
    assert_eq!(sorted[2].name, "C");
}

// ── DownloadDirective serialization ────────────────────────────────

#[test]
fn test_download_directive_roundtrip() {
    let directives = vec![
        DownloadDirective::Nexus {
            game_id: GameId::from("skyrimse"),
            mod_id: 42.into(),
            file_id: 99.into(),
            hash: 12345,
        },
        DownloadDirective::GitHub {
            user: "user".to_string(),
            repo: "repo".to_string(),
            tag: "v1".to_string(),
            asset: "file.zip".to_string(),
            hash: 67890,
        },
        DownloadDirective::GoogleDrive {
            id: "abc".to_string(),
            hash: 11111,
        },
        DownloadDirective::Mega {
            url: "https://mega.nz/file/X#Y".to_string(),
            hash: 22222,
        },
        DownloadDirective::DirectURL {
            url: "https://example.com/f.zip".to_string(),
            headers: HashMap::new(),
            mirror_resolver: None,
            hash: 33333,
        },
    ];

    for d in &directives {
        let json = serde_json::to_string(d).unwrap();
        let parsed: DownloadDirective = serde_json::from_str(&json).unwrap();
        let json2 = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, json2);
    }
}

// ── BSAFileState ───────────────────────────────────────────────────

#[test]
fn test_bsa_file_state_default_size() {
    let json = r#"{"Path": "textures/sky.dds", "Hash": 42}"#;
    let state: BSAFileState = serde_json::from_str(json).unwrap();
    assert_eq!(state.path, "textures/sky.dds");
    assert_eq!(state.hash, 42);
    assert_eq!(state.size, 0); // default
}

#[test]
fn test_bsa_file_state_with_size() {
    let json = r#"{"Path": "meshes/body.nif", "Hash": 99, "Size": 4096}"#;
    let state: BSAFileState = serde_json::from_str(json).unwrap();
    assert_eq!(state.size, 4096);
}

// ── HttpDownloader empty headers ───────────────────────────────────

#[test]
fn test_http_downloader_empty_headers() {
    let manifest = WabbajackManifest {
        archives: vec![ArchiveEntry {
            hash: 1,
            name: "file.zip".to_string(),
            size: 100,
            state: Some(ArchiveState::HttpDownloader {
                url: "https://example.com/file.zip".to_string(),
                headers: HashMap::new(),
            }),
        }],
        ..test_manifest()
    };

    let directives = manifest.download_directives();
    assert_eq!(directives.len(), 1);
    match &directives[0] {
        DownloadDirective::DirectURL { headers, .. } => {
            assert!(headers.is_empty());
        }
        _ => panic!("expected DirectURL"),
    }
}

#[test]
fn test_game_file_source_downloader_parses_and_is_not_download_directive() {
    let json = r#"{
        "Hash": 123,
        "Name": "Data_Update.esm",
        "Size": 4096,
        "State": {
            "$type": "GameFileSourceDownloader, Wabbajack.Lib",
            "Game": "SkyrimSpecialEdition",
            "GameVersion": "1.6.1170.0",
            "File": "Data\\Update.esm"
        }
    }"#;

    let archive: ArchiveEntry = serde_json::from_str(json).unwrap();
    let state = archive.state.as_ref().unwrap();
    assert_eq!(state.game_file_path(), Some("Data\\Update.esm"));

    let manifest = WabbajackManifest {
        archives: vec![archive],
        ..test_manifest()
    };
    assert!(manifest.download_directives().is_empty());
}

// ── Mixed directives ───────────────────────────────────────────────

#[test]
fn test_mixed_install_and_download_directives() {
    let manifest = WabbajackManifest {
        archives: vec![
            ArchiveEntry {
                hash: 100,
                name: "archive_a.zip".to_string(),
                size: 5000,
                state: Some(ArchiveState::NexusDownloader {
                    game_name: "skyrimse".to_string(),
                    mod_id: 1.into(),
                    file_id: 1.into(),
                }),
            },
            ArchiveEntry {
                hash: 200,
                name: "stateless.zip".to_string(),
                size: 3000,
                state: None,
            },
        ],
        directives: vec![
            RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(100.into()),
                    serde_json::Value::String("file.txt".to_string()),
                ],
                to: "Data/file.txt".to_string(),
                size: 0,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(100.into())],
                patch_id: String::new(),
                to: "patched.esp".to_string(),
                hash: 999,
                size: 0,
            },
            RawDirective::CreateBSA {
                temp_id: "bsa1".to_string(),
                to: "archive.bsa".to_string(),
                file_states: vec![],
            },
            RawDirective::Unknown,
        ],
        ..test_manifest()
    };

    assert_eq!(manifest.download_directives().len(), 1); // only archive with state
    assert_eq!(manifest.install_directives().len(), 3); // FromArchive + Patched + CreateBSA (Unknown filtered)
}
