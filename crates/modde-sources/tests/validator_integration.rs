use modde_core::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, BSAFileState, RawDirective, WabbajackManifest,
};
use modde_sources::wabbajack::validator::validate_install;
use xxhash_rust::xxh3::xxh3_64;

fn empty_manifest() -> WabbajackManifest {
    WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![],
    }
}

// ── Mixed directive types ───────────────────────────────────────────

#[tokio::test]
async fn test_validate_from_archive_checks_presence_only() {
    let staging = tempfile::tempdir().unwrap();
    let content = b"archive file content";

    // Create the file in staging
    tokio::fs::write(staging.path().join("output.esp"), content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        archives: vec![ArchiveEntry {
            hash: 123456,
            name: "test.zip".to_string(),
            size: 100,
            state: None,
        }],
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(serde_json::Number::from(123456)),
                serde_json::Value::String("inner.esp".to_string()),
            ],
            to: "output.esp".to_string(),
            size: 0,
        }],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.verified, 1);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validate_from_archive_missing_output() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        archives: vec![],
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(99999.into()),
                serde_json::Value::String("file.txt".to_string()),
            ],
            to: "output.txt".to_string(),
            size: 0,
        }],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.verified, 0);
    assert_eq!(report.missing, vec!["output.txt"]);
}

#[tokio::test]
async fn test_validate_mixed_from_archive_and_patched() {
    let staging = tempfile::tempdir().unwrap();

    // Create files
    let correct_content = b"correct archive output";
    let correct_hash = xxh3_64(correct_content);
    tokio::fs::write(staging.path().join("from_archive.esp"), correct_content)
        .await
        .unwrap();

    let patched_content = b"patched output data";
    let patched_hash = xxh3_64(patched_content);
    tokio::fs::write(staging.path().join("patched.esp"), patched_content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        archives: vec![ArchiveEntry {
            hash: correct_hash,
            name: "source.zip".to_string(),
            size: 200,
            state: Some(ArchiveState::NexusDownloader {
                game_name: "SkyrimSE".to_string(),
                mod_id: 42.into(),
                file_id: 99.into(),
            }),
        }],
        directives: vec![
            RawDirective::FromArchive {
                archive_hash_path: vec![
                    serde_json::Value::Number(serde_json::Number::from(correct_hash)),
                    serde_json::Value::String("data.esp".to_string()),
                ],
                to: "from_archive.esp".to_string(),
                size: 0,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "patched.esp".to_string(),
                hash: patched_hash,
            },
        ],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 2);
    assert_eq!(report.verified, 2);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validate_all_directive_types_mixed() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        archives: vec![],
        directives: vec![
            // PatchedFromArchive - file missing
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "missing.txt".to_string(),
                hash: 12345,
            },
            // CreateBSA - should be ignored
            RawDirective::CreateBSA {
                temp_id: "bsa1".to_string(),
                to: "output.bsa".to_string(),
                file_states: vec![BSAFileState {
                    path: "mesh.nif".to_string(),
                    hash: 0,
                    size: 100,
                }],
            },
            // Unknown - should be ignored
            RawDirective::Unknown,
        ],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1, "only PatchedFromArchive counts");
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.missing[0], "missing.txt");
}

// ── Deeply nested files ─────────────────────────────────────────────

#[tokio::test]
async fn test_validate_deeply_nested_file() {
    let staging = tempfile::tempdir().unwrap();
    let deep = staging.path().join("textures/landscape/mountains");
    tokio::fs::create_dir_all(&deep).await.unwrap();

    let content = b"deep texture data";
    let hash = xxh3_64(content);
    tokio::fs::write(deep.join("peak.dds"), content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        directives: vec![RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::Value::Number(0.into())],
            patch_id: String::new(),
            size: 0,
            to: "textures/landscape/mountains/peak.dds".to_string(),
            hash,
        }],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.verified, 1);
}

// ── Report correctness ──────────────────────────────────────────────

#[tokio::test]
async fn test_validate_report_counts_are_consistent() {
    let staging = tempfile::tempdir().unwrap();

    // 3 files: 1 correct, 1 wrong hash, 1 missing
    let correct_content = b"correct";
    let correct_hash = xxh3_64(correct_content);
    tokio::fs::write(staging.path().join("correct.txt"), correct_content)
        .await
        .unwrap();
    tokio::fs::write(staging.path().join("wrong.txt"), b"wrong data")
        .await
        .unwrap();
    // missing.txt not created

    let manifest = WabbajackManifest {
        directives: vec![
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "correct.txt".to_string(),
                hash: correct_hash,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "wrong.txt".to_string(),
                hash: 99999,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "missing.txt".to_string(),
                hash: 88888,
            },
        ],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 3);
    assert_eq!(report.verified, 1);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.mismatches.len(), 1);
    // total_files == verified + missing + mismatches
    assert_eq!(
        report.total_files,
        report.verified + report.missing.len() + report.mismatches.len()
    );
}

#[tokio::test]
async fn test_validate_mismatch_contains_both_hashes() {
    let staging = tempfile::tempdir().unwrap();
    let content = b"actual content";
    let actual_hash = xxh3_64(content);
    tokio::fs::write(staging.path().join("file.txt"), content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        directives: vec![RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::Value::Number(0.into())],
            patch_id: String::new(),
            size: 0,
            to: "file.txt".to_string(),
            hash: 0, // Wrong expected hash
        }],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].expected_hash, 0);
    assert_eq!(report.mismatches[0].actual_hash, actual_hash);
    assert_eq!(report.mismatches[0].path, "file.txt");
}

// ── Many files stress test ──────────────────────────────────────────

#[tokio::test]
async fn test_validate_many_files() {
    let staging = tempfile::tempdir().unwrap();

    let mut directives = Vec::new();
    for i in 0..50 {
        let content = format!("content_{i}");
        let hash = xxh3_64(content.as_bytes());
        let filename = format!("file_{i}.txt");
        tokio::fs::write(staging.path().join(&filename), content.as_bytes())
            .await
            .unwrap();

        directives.push(RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::Value::Number(0.into())],
            patch_id: String::new(),
            size: 0,
            to: filename,
            hash,
        });
    }

    let manifest = WabbajackManifest {
        directives,
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 50);
    assert_eq!(report.verified, 50);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}
