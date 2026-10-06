//! Comprehensive Wabbajack installer, patcher, and validator integration tests.

use std::path::PathBuf;

use modde_core::manifest::wabbajack::{ArchiveEntry, RawDirective, WabbajackManifest};
use modde_sources::wabbajack::patcher::apply_patch;
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

// ── Patcher: comprehensive edge cases ──────────────────────────────

const OCTODELTA_MAGIC: &[u8; 9] = b"OCTODELTA";
const OP_COPY: u8 = 0x60;
const OP_DATA: u8 = 0x80;

fn build_patch(ops: &[(u8, &[u8])]) -> Vec<u8> {
    let mut patch = Vec::new();
    patch.extend_from_slice(OCTODELTA_MAGIC);
    patch.push(1); // version
    patch.push(4); // hash name length
    patch.extend_from_slice(b"SHA1");
    patch.extend_from_slice(&20u32.to_le_bytes());
    patch.extend_from_slice(&[0u8; 20]); // dummy hash
    patch.extend_from_slice(b">>>");
    for (op_type, payload) in ops {
        patch.push(*op_type);
        patch.extend_from_slice(payload);
    }
    patch
}

fn copy_op(offset: u64, length: u64) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&offset.to_le_bytes());
    v.extend_from_slice(&length.to_le_bytes());
    v
}

fn data_op(data: &[u8]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&(data.len() as u64).to_le_bytes());
    v.extend_from_slice(data);
    v
}

#[test]
fn test_patch_reconstruct_from_non_contiguous_fragments() {
    let source = b"ABCDEFGHIJKLMNOP";
    let c1 = copy_op(4, 6); // offset 4, length 6 = "EFGHIJ"
    let c2 = copy_op(12, 4); // offset 12, length 4 = "MNOP"
    let patch = build_patch(&[(OP_COPY, &c1), (OP_COPY, &c2)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"EFGHIJMNOP");
}

#[test]
fn test_patch_interleave_copy_and_insert() {
    let source = b"Hello World";
    let c1 = copy_op(0, 5); // "Hello"
    let d1 = data_op(b", ");
    let c2 = copy_op(6, 5); // "World"
    let d2 = data_op(b"!");
    let patch = build_patch(&[
        (OP_COPY, &c1),
        (OP_DATA, &d1),
        (OP_COPY, &c2),
        (OP_DATA, &d2),
    ]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"Hello, World!");
}

#[test]
fn test_patch_overlapping_copies() {
    let source = b"ABCDE";
    let c1 = copy_op(0, 3); // "ABC"
    let c2 = copy_op(1, 3); // "BCD"
    let patch = build_patch(&[(OP_COPY, &c1), (OP_COPY, &c2)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"ABCBCD");
}

#[test]
fn test_patch_reverse_order_copy() {
    let source = b"ABCDEFGHIJ";
    let c1 = copy_op(7, 3); // "HIJ"
    let c2 = copy_op(0, 3); // "ABC"
    let patch = build_patch(&[(OP_COPY, &c1), (OP_COPY, &c2)]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"HIJABC");
}

#[test]
fn test_patch_single_byte_operations() {
    let source = b"XY";
    let c1 = copy_op(0, 1); // "X"
    let d1 = data_op(b"-");
    let c2 = copy_op(1, 1); // "Y"
    let d2 = data_op(b"!");
    let patch = build_patch(&[
        (OP_COPY, &c1),
        (OP_DATA, &d1),
        (OP_COPY, &c2),
        (OP_DATA, &d2),
    ]);
    let result = apply_patch(source, &patch).unwrap();
    assert_eq!(&result, b"X-Y!");
}

#[test]
fn test_patch_large_binary_data() {
    let source: Vec<u8> = (0..=255).cycle().take(100_000).collect();
    let c1 = copy_op(0, 50000);
    let insert_data = vec![0xFF; 50000];
    let d1 = data_op(&insert_data);
    let patch = build_patch(&[(OP_COPY, &c1), (OP_DATA, &d1)]);
    let result = apply_patch(&source, &patch).unwrap();
    assert_eq!(result.len(), 100000);
    assert_eq!(&result[..50000], &source[..50000]);
    assert!(result[50000..].iter().all(|&b| b == 0xFF));
}

#[test]
fn test_patch_empty_source_empty_output() {
    let patch = build_patch(&[]);
    let result = apply_patch(b"", &patch).unwrap();
    assert_eq!(result, [] as [u8; 0]);
}

#[test]
fn test_patch_error_invalid_magic() {
    let result = apply_patch(b"source", b"BAADMAGIC\x01\x04SHA1");
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("invalid patch magic")
    );
}

#[test]
fn test_patch_error_truncated_header() {
    let result = apply_patch(b"", b"OCTO");
    assert!(result.is_err());
}

#[test]
fn test_patch_error_copy_past_end() {
    let source = b"ABC";
    let c1 = copy_op(0, 5); // request 5 bytes but source only has 3
    let patch = build_patch(&[(OP_COPY, &c1)]);
    let result = apply_patch(source, &patch);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("out of bounds"));
}

#[test]
fn test_patch_error_unknown_op() {
    let mut patch = build_patch(&[]);
    patch.push(99); // unknown op
    patch.extend_from_slice(&5u64.to_le_bytes());
    let result = apply_patch(b"source", &patch);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("unknown"));
}

// ── Validator: comprehensive tests ─────────────────────────────────

#[tokio::test]
async fn test_validator_empty_manifest_returns_empty_report() {
    let staging = tempfile::tempdir().unwrap();
    let manifest = empty_manifest();
    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 0);
    assert_eq!(report.verified, 0);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}

#[tokio::test]
async fn test_validator_all_files_present_and_correct() {
    let staging = tempfile::tempdir().unwrap();

    let content_a = b"file a content";
    let content_b = b"file b content";
    let hash_a = xxh3_64(content_a);
    let hash_b = xxh3_64(content_b);

    tokio::fs::write(staging.path().join("file_a.txt"), content_a)
        .await
        .unwrap();
    tokio::fs::write(staging.path().join("file_b.txt"), content_b)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        directives: vec![
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "file_a.txt".to_string(),
                hash: hash_a,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "file_b.txt".to_string(),
                hash: hash_b,
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
async fn test_validator_all_files_missing() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        directives: vec![
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "a.txt".to_string(),
                hash: 111,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "b.txt".to_string(),
                hash: 222,
            },
            RawDirective::PatchedFromArchive {
                archive_hash_path: vec![serde_json::Value::Number(0.into())],
                patch_id: String::new(),
                size: 0,
                to: "c.txt".to_string(),
                hash: 333,
            },
        ],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 3);
    assert_eq!(report.verified, 0);
    assert_eq!(report.missing.len(), 3);
}

#[tokio::test]
async fn test_validator_hash_mismatch_includes_both_hashes() {
    let staging = tempfile::tempdir().unwrap();
    let content = b"actual content";
    tokio::fs::write(staging.path().join("file.txt"), content)
        .await
        .unwrap();

    let wrong_hash = 99999u64;

    let manifest = WabbajackManifest {
        directives: vec![RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::Value::Number(0.into())],
            patch_id: String::new(),
            size: 0,
            to: "file.txt".to_string(),
            hash: wrong_hash,
        }],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.mismatches.len(), 1);
    assert_eq!(report.mismatches[0].expected_hash, wrong_hash);
    assert_eq!(report.mismatches[0].actual_hash, xxh3_64(content));
    assert_ne!(
        report.mismatches[0].expected_hash,
        report.mismatches[0].actual_hash
    );
}

#[tokio::test]
async fn test_validator_from_archive_checks_presence_only() {
    let staging = tempfile::tempdir().unwrap();

    let content = b"archive content";
    tokio::fs::write(staging.path().join("extracted.txt"), content)
        .await
        .unwrap();

    let archive_hash = 55555u64;

    let manifest = WabbajackManifest {
        archives: vec![ArchiveEntry {
            hash: archive_hash,
            name: "source.zip".to_string(),
            size: 100,
            state: None,
        }],
        directives: vec![RawDirective::FromArchive {
            archive_hash_path: vec![
                serde_json::Value::Number(archive_hash.into()),
                serde_json::Value::String("inner.txt".to_string()),
            ],
            to: "extracted.txt".to_string(),
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
async fn test_validator_deeply_nested_files() {
    let staging = tempfile::tempdir().unwrap();

    let nested_path = staging.path().join("a/b/c/d");
    tokio::fs::create_dir_all(&nested_path).await.unwrap();

    let content = b"deep content";
    let hash = xxh3_64(content);
    tokio::fs::write(nested_path.join("file.txt"), content)
        .await
        .unwrap();

    let manifest = WabbajackManifest {
        directives: vec![RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::Value::Number(0.into())],
            patch_id: String::new(),
            size: 0,
            to: "a/b/c/d/file.txt".to_string(),
            hash,
        }],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.verified, 1);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
}

#[tokio::test]
async fn test_validator_mixed_correct_missing_mismatch() {
    let staging = tempfile::tempdir().unwrap();

    // Correct file
    let correct = b"correct data";
    let correct_hash = xxh3_64(correct);
    tokio::fs::write(staging.path().join("correct.txt"), correct)
        .await
        .unwrap();

    // File with wrong hash
    tokio::fs::write(staging.path().join("wrong.txt"), b"wrong data")
        .await
        .unwrap();

    // Missing file: not created

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
}

#[tokio::test]
async fn test_validator_create_bsa_and_unknown_ignored() {
    let staging = tempfile::tempdir().unwrap();

    let manifest = WabbajackManifest {
        directives: vec![
            RawDirective::CreateBSA {
                temp_id: "bsa1".to_string(),
                to: "output.bsa".to_string(),
                file_states: vec![],
            },
            RawDirective::Unknown,
        ],
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 0);
}

// ── WabbajackInstaller unit tests ──────────────────────────────────

#[test]
fn test_installer_construction() {
    use modde_sources::wabbajack::installer::WabbajackInstaller;

    let manifest = empty_manifest();
    let installer = WabbajackInstaller::new(
        manifest,
        PathBuf::new(),
        PathBuf::from("/store"),
        PathBuf::from("/staging"),
    );
    // Should not panic
    drop(installer);
}

#[test]
fn test_installer_set_concurrency_clamps_to_min_1() {
    use modde_sources::wabbajack::installer::WabbajackInstaller;

    let manifest = empty_manifest();
    let mut installer = WabbajackInstaller::new(
        manifest,
        PathBuf::new(),
        PathBuf::from("/store"),
        PathBuf::from("/staging"),
    );

    installer.set_concurrency(0); // Should clamp to 1
    installer.set_concurrency(1);
    installer.set_concurrency(100);
    // No panic = success
}

#[tokio::test]
async fn test_installer_empty_manifest_completes() {
    use modde_sources::wabbajack::installer::{InstallProgress, WabbajackInstaller};
    use tokio::sync::mpsc;

    let tmp = tempfile::tempdir().unwrap();
    let manifest = empty_manifest();
    let installer = WabbajackInstaller::new(
        manifest,
        PathBuf::new(),
        tmp.path().join("store"),
        tmp.path().join("staging"),
    );

    let (tx, mut rx) = mpsc::unbounded_channel();
    installer.install(tx).await.unwrap();

    // Should receive Starting and Complete
    let mut got_starting = false;
    let mut got_complete = false;
    while let Ok(msg) = rx.try_recv() {
        match msg {
            InstallProgress::Starting { total_downloads } => {
                assert_eq!(total_downloads, 0);
                got_starting = true;
            }
            InstallProgress::Complete => {
                got_complete = true;
            }
            _ => {}
        }
    }
    assert!(got_starting, "should receive Starting");
    assert!(got_complete, "should receive Complete");
}

// ── Manifest parsing from zip ──────────────────────────────────────

#[test]
fn test_parse_wabbajack_file_valid_zip() {
    use modde_sources::wabbajack::manifest::parse_wabbajack_file;

    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("test.wabbajack");

    // Create a zip with a "modlist" entry
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("modlist", options).unwrap();

    let manifest_json = serde_json::to_string(&empty_manifest()).unwrap();
    std::io::Write::write_all(&mut zip, manifest_json.as_bytes()).unwrap();
    zip.finish().unwrap();

    let parsed = parse_wabbajack_file(&zip_path).unwrap();
    assert_eq!(parsed.name, "test");
    assert_eq!(parsed.game, "skyrimse");
}

#[test]
fn test_parse_wabbajack_file_json_entry_fallback() {
    use modde_sources::wabbajack::manifest::parse_wabbajack_file;

    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("test.wabbajack");

    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("manifest.json", options).unwrap();

    let manifest_json = serde_json::to_string(&WabbajackManifest {
        name: "JSON Fallback".to_string(),
        ..empty_manifest()
    })
    .unwrap();
    std::io::Write::write_all(&mut zip, manifest_json.as_bytes()).unwrap();
    zip.finish().unwrap();

    let parsed = parse_wabbajack_file(&zip_path).unwrap();
    assert_eq!(parsed.name, "JSON Fallback");
}

#[test]
fn test_parse_wabbajack_file_no_manifest_entry() {
    use modde_sources::wabbajack::manifest::parse_wabbajack_file;

    let tmp = tempfile::tempdir().unwrap();
    let zip_path = tmp.path().join("empty.wabbajack");

    let file = std::fs::File::create(&zip_path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default();
    zip.start_file("readme.txt", options).unwrap();
    std::io::Write::write_all(&mut zip, b"not a manifest").unwrap();
    zip.finish().unwrap();

    let result = parse_wabbajack_file(&zip_path);
    assert!(result.is_err());
}

#[test]
fn test_parse_wabbajack_file_nonexistent() {
    use modde_sources::wabbajack::manifest::parse_wabbajack_file;

    let result = parse_wabbajack_file(&PathBuf::from("/nonexistent/file.wabbajack"));
    assert!(result.is_err());
}

// ── Validator: large-scale stress test ─────────────────────────────

#[tokio::test]
async fn test_validator_many_files_stress() {
    let staging = tempfile::tempdir().unwrap();

    let mut directives = Vec::new();
    for i in 0..100 {
        let content = format!("content_{i}");
        let hash = xxh3_64(content.as_bytes());
        let rel_path = format!("dir_{}/file_{}.txt", i / 10, i);

        let full_path = staging.path().join(&rel_path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent).await.unwrap();
        }
        tokio::fs::write(&full_path, content.as_bytes())
            .await
            .unwrap();

        directives.push(RawDirective::PatchedFromArchive {
            archive_hash_path: vec![serde_json::Value::Number(0.into())],
            patch_id: String::new(),
            size: 0,
            to: rel_path,
            hash,
        });
    }

    let manifest = WabbajackManifest {
        directives,
        ..empty_manifest()
    };

    let report = validate_install(&manifest, staging.path()).await.unwrap();
    assert_eq!(report.total_files, 100);
    assert_eq!(report.verified, 100);
    assert_eq!(report.missing, [] as [std::string::String; 0]);
    assert!(report.mismatches.is_empty());
}
