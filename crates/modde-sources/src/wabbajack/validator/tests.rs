use super::*;
use crate::wabbajack::staging::{StagingCompressionPolicy, StagingStore, compressed_path};
use modde_core::manifest::wabbajack::WabbajackManifest;
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

#[tokio::test]
async fn test_validate_empty_manifest() {
    let staging = tempfile::tempdir().unwrap();
    let manifest = empty_manifest();
    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 0);
    assert_eq!(report.verified, 0);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validate_missing_file() {
    let staging = tempfile::tempdir().unwrap();
    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
            hash: 12345,
            name: "test.zip".to_string(),
            size: 100,
            state: None,
        }],
        directives: vec![modde_core::manifest::wabbajack::RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(12345.into()),
                serde_json::Value::String("inner.txt".to_string()),
            ],
            to: "output/inner.txt".to_string(),
            size: 0,
        }],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.missing[0], "output/inner.txt");
}

#[tokio::test]
async fn preflight_and_validate_accept_compressed_logical_files() {
    let staging = tempfile::tempdir().unwrap();
    let rel = "mods/test/texture.dds";
    let content = vec![7_u8; 128 * 1024];
    let store = StagingStore::with_policy(
        staging.path(),
        StagingCompressionPolicy {
            min_bytes: 1,
            level: 1,
            suffix: ".modde-zst".to_string(),
        },
    );
    store.prepare_fresh().await.unwrap();
    let file_path = staging.path().join(rel);
    tokio::fs::create_dir_all(file_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&file_path, &content).await.unwrap();
    store.compress_eligible_files(1).await.unwrap();

    let manifest = WabbajackManifest {
        directives: vec![modde_core::manifest::wabbajack::RawDirective::InlineFile {
            hash: xxh3_64(&content),
            size: content.len() as u64,
            source_data_id: "inline".into(),
            to: rel.into(),
        }],
        ..empty_manifest()
    };

    assert!(!file_path.exists());
    assert!(compressed_path(&file_path).exists());
    assert!(preflight_staging(&manifest, staging.path()).await);
    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.verified, 1);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn validate_fails_on_corrupt_compressed_logical_file() {
    let staging = tempfile::tempdir().unwrap();
    let rel = "mods/test/texture.dds";
    let content = vec![9_u8; 128 * 1024];
    let store = StagingStore::with_policy(
        staging.path(),
        StagingCompressionPolicy {
            min_bytes: 1,
            level: 1,
            suffix: ".modde-zst".to_string(),
        },
    );
    store.prepare_fresh().await.unwrap();
    let file_path = staging.path().join(rel);
    tokio::fs::create_dir_all(file_path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&file_path, &content).await.unwrap();
    store.compress_eligible_files(1).await.unwrap();
    tokio::fs::write(compressed_path(&file_path), b"not zstd")
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        directives: vec![modde_core::manifest::wabbajack::RawDirective::InlineFile {
            hash: xxh3_64(&content),
            size: content.len() as u64,
            source_data_id: "inline".into(),
            to: rel.into(),
        }],
        ..empty_manifest()
    };

    assert!(validate_install(&manifest, staging.path()).await.is_err());
}

#[tokio::test]
async fn test_validate_hash_mismatch() {
    let staging = tempfile::tempdir().unwrap();
    let file_path = staging.path().join("test.txt");
    tokio::fs::write(&file_path, b"wrong content")
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "test.txt".to_string(),
                hash: 99999, // wrong hash
            },
        ],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.mismatches.len(), 1);
}

#[tokio::test]
async fn test_validate_correct_file() {
    let staging = tempfile::tempdir().unwrap();
    let content = b"correct file content";
    let file_path = staging.path().join("test.txt");
    tokio::fs::write(&file_path, content).await.unwrap();

    let expected_hash = xxh3_64(content);

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "test.txt".to_string(),
                hash: expected_hash,
            },
        ],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.verified, 1);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validate_multiple_missing() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "file_a.txt".to_string(),
                hash: 111,
            },
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "file_b.txt".to_string(),
                hash: 222,
            },
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "file_c.txt".to_string(),
                hash: 333,
            },
        ],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 3);
    assert_eq!(report.verified, 0);
    assert_eq!(report.missing.len(), 3);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validate_mixed_results() {
    let staging = tempfile::tempdir().unwrap();

    // File with correct hash
    let correct_content = b"correct data";
    let correct_hash = xxh3_64(correct_content);
    tokio::fs::write(staging.path().join("correct.txt"), correct_content)
        .await
        .unwrap();

    // File with wrong hash
    tokio::fs::write(staging.path().join("wrong.txt"), b"actual data")
        .await
        .unwrap();

    // missing.txt is not created

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "correct.txt".to_string(),
                hash: correct_hash,
            },
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "wrong.txt".to_string(),
                hash: 99999,
            },
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "missing.txt".to_string(),
                hash: 55555,
            },
        ],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 3);
    assert_eq!(report.verified, 1);
    assert_eq!(report.missing.len(), 1);
    assert_eq!(report.missing[0], "missing.txt");
    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].path, "wrong.txt");
}

#[tokio::test]
async fn test_validate_patched_directive_uses_hash_field() {
    let staging = tempfile::tempdir().unwrap();
    let content = b"patched output data";
    let patched_hash = xxh3_64(content);
    tokio::fs::write(staging.path().join("patched.esp"), content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
            hash: 77777,
            name: "source.zip".to_string(),
            size: 200,
            state: None,
        }],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(77777.into())],
                patch_id: String::new(),
                size: 0,
                to: "patched.esp".to_string(),
                hash: patched_hash, // uses the directive's own hash, not archive hash
            },
        ],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.verified, 1);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validate_create_bsa_ignored() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![modde_core::manifest::wabbajack::RawDirective::CreateBSA {
            temp_id: "bsa_temp_001".to_string(),
            to: "output.bsa".to_string(),
            file_states: vec![],
        }],
    };

    let expected = collect_expected_files(&manifest);
    assert!(
        expected.is_empty(),
        "CreateBSA should not produce expected files"
    );

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 0);
}

#[tokio::test]
async fn test_validate_unknown_directive_ignored() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![modde_core::manifest::wabbajack::RawDirective::Unknown],
    };

    let expected = collect_expected_files(&manifest);
    assert!(
        expected.is_empty(),
        "Unknown directives should be filtered out"
    );

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 0);
}

#[tokio::test]
async fn test_validate_nested_file_path() {
    let staging = tempfile::tempdir().unwrap();
    let subdir = staging.path().join("subdir");
    tokio::fs::create_dir_all(&subdir).await.unwrap();

    let content = b"nested file content";
    let expected_hash = xxh3_64(content);
    tokio::fs::write(subdir.join("test.txt"), content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        name: "test".to_string(),
        author: "test".to_string(),
        description: "test".to_string(),
        game: "skyrimse".to_string(),
        version: "1.0".to_string(),
        archives: vec![],
        directives: vec![
            modde_core::manifest::wabbajack::RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "subdir/test.txt".to_string(),
                hash: expected_hash,
            },
        ],
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 1);
    assert_eq!(report.verified, 1);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}
