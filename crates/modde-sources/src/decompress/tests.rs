use super::*;
use std::process::Command;

fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
    let file = File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in entries {
        zip.start_file(*name, options).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, data) in entries {
            zip.start_file(*name, options).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap();
    }
    cursor.into_inner()
}

fn write_7z(path: &Path, entries: &[(&str, &[u8])]) {
    let temp = tempfile::tempdir().unwrap();
    for (name, data) in entries {
        let file_path = temp.path().join(name.replace('\\', "/"));
        std::fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        std::fs::write(file_path, data).unwrap();
    }

    let status = Command::new("7zz")
        .arg("a")
        .arg("-t7z")
        .arg("-mx=1")
        .arg(path)
        .arg(".")
        .current_dir(temp.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "7zz failed to create fixture archive");
}

#[test]
fn zip_batch_writes_files_and_returns_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.zip");
    write_zip(
        &archive,
        &[("Data/A.txt", b"alpha"), ("Data/B.txt", b"beta")],
    );
    let out = temp.path().join("out").join("a.txt");

    let output = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[
            ArchiveRequest {
                directive_index: 7,
                from: "data/a.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: None,
                },
            },
            ArchiveRequest {
                directive_index: 9,
                from: "Data/B.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            },
        ],
    )
    .unwrap();

    assert_eq!(std::fs::read(out).unwrap(), b"alpha");
    assert_eq!(output.bytes.get(&9).unwrap(), b"beta");
}

#[test]
fn zip_write_rejects_entry_larger_than_expected_size() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.zip");
    write_zip(&archive, &[("Data/A.txt", b"alpha")]);
    let out = temp.path().join("out").join("a.txt");

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[ArchiveRequest {
            directive_index: 7,
            from: "data/a.txt".to_string(),
            inner_path: None,
            kind: ArchiveRequestKind::WriteFile {
                to: out.clone(),
                expected_size: Some(3),
            },
        }],
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("entry size mismatch"));
}

#[test]
fn zip_write_rejects_entry_smaller_than_expected_size() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.zip");
    write_zip(&archive, &[("Data/A.txt", b"alpha")]);
    let out = temp.path().join("out").join("a.txt");

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[ArchiveRequest {
            directive_index: 7,
            from: "data/a.txt".to_string(),
            inner_path: None,
            kind: ArchiveRequestKind::WriteFile {
                to: out.clone(),
                expected_size: Some(6),
            },
        }],
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("entry size mismatch"));
}

#[test]
fn traversal_request_is_rejected_before_writing() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.zip");
    write_zip(&archive, &[("safe.txt", b"ok")]);
    let out = temp.path().join("out.txt");

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[ArchiveRequest {
            directive_index: 1,
            from: "../escape.txt".to_string(),
            inner_path: None,
            kind: ArchiveRequestKind::WriteFile {
                to: out.clone(),
                expected_size: Some(10),
            },
        }],
    )
    .unwrap_err();

    assert!(format!("{err:#}").contains("path traversal"));
    assert!(!out.exists());
}

#[test]
fn in_memory_zip_batch_writes_files_and_returns_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let archive = zip_bytes(&[("Data/A.txt", b"alpha"), ("Data/B.txt", b"beta")]);
    let out = temp.path().join("out").join("a.txt");

    let output = ArchiveBatchExtractor::extract_selected_from(
        ArchiveInput::Bytes {
            name: "fixture.zip",
            bytes: &archive,
        },
        &[
            ArchiveRequest {
                directive_index: 7,
                from: "data/a.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: None,
                },
            },
            ArchiveRequest {
                directive_index: 9,
                from: "Data/B.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            },
        ],
    )
    .unwrap();

    assert_eq!(std::fs::read(out).unwrap(), b"alpha");
    assert_eq!(output.bytes.get(&9).unwrap(), b"beta");
}

#[test]
fn seven_z_duplicate_write_requests_share_one_entry() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.7z");
    write_7z(&archive, &[("Data/Dupe.txt", b"same bytes")]);
    let out_a = temp.path().join("out-a.txt");
    let out_b = temp.path().join("out-b.txt");

    ArchiveBatchExtractor::extract_selected(
        &archive,
        &[
            ArchiveRequest {
                directive_index: 1,
                from: "data/dupe.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out_a.clone(),
                    expected_size: None,
                },
            },
            ArchiveRequest {
                directive_index: 2,
                from: "Data\\Dupe.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out_b.clone(),
                    expected_size: None,
                },
            },
        ],
    )
    .unwrap();

    assert_eq!(std::fs::read(out_a).unwrap(), b"same bytes");
    assert_eq!(std::fs::read(out_b).unwrap(), b"same bytes");
}

#[test]
fn seven_z_duplicate_bytes_and_write_requests_share_one_entry() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.7z");
    write_7z(&archive, &[("Data/Dupe.txt", b"shared bytes")]);
    let out = temp.path().join("out.txt");

    let output = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[
            ArchiveRequest {
                directive_index: 1,
                from: "Data/Dupe.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            },
            ArchiveRequest {
                directive_index: 2,
                from: "data\\dupe.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: None,
                },
            },
        ],
    )
    .unwrap();

    assert_eq!(output.bytes.get(&1).unwrap(), b"shared bytes");
    assert_eq!(std::fs::read(out).unwrap(), b"shared bytes");
}

#[test]
fn seven_z_duplicate_bytes_and_write_rejects_size_mismatch() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.7z");
    write_7z(&archive, &[("Data/Dupe.txt", b"shared bytes")]);
    let out = temp.path().join("out.txt");

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[
            ArchiveRequest {
                directive_index: 1,
                from: "Data/Dupe.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            },
            ArchiveRequest {
                directive_index: 2,
                from: "data\\dupe.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::WriteFile {
                    to: out.clone(),
                    expected_size: Some(3),
                },
            },
        ],
    )
    .unwrap_err();

    let msg = format!("{err:#}");
    assert!(msg.contains("exceeds expected size") || msg.contains("entry size mismatch"));
    assert!(!out.exists());
}

#[tokio::test]
#[cfg(feature = "bethesda-archives")]
async fn zip_entry_can_satisfy_nested_bsa_request() {
    let temp = tempfile::tempdir().unwrap();
    let bsa_root = temp.path().join("bsa-root");
    let source_path = bsa_root.join("meshes/actors/test.nif");
    std::fs::create_dir_all(source_path.parent().unwrap()).unwrap();
    std::fs::write(&source_path, b"nested nif").unwrap();

    let bsa_path = temp.path().join("inner.bsa");
    crate::wabbajack::bsa_repack::create_bsa(
        &[modde_core::manifest::wabbajack::BSAFileState {
            path: "meshes/actors/test.nif".to_string(),
            hash: 0,
            size: 10,
        }],
        &bsa_root,
        &bsa_path,
    )
    .await
    .unwrap();

    let archive = temp.path().join("outer.zip");
    let bsa_bytes = std::fs::read(&bsa_path).unwrap();
    write_zip(&archive, &[("Inner.bsa", &bsa_bytes)]);
    let out = temp.path().join("out.nif");

    ArchiveBatchExtractor::extract_selected(
        &archive,
        &[ArchiveRequest {
            directive_index: 42,
            from: "inner.bsa".to_string(),
            inner_path: Some("meshes\\actors\\test.nif".to_string()),
            kind: ArchiveRequestKind::WriteFile {
                to: out.clone(),
                expected_size: None,
            },
        }],
    )
    .unwrap();

    assert_eq!(std::fs::read(out).unwrap(), b"nested nif");
}

#[test]
fn missing_entry_error_deduplicates_repeated_paths() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.7z");
    write_7z(&archive, &[("Data/Present.txt", b"present")]);

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[
            ArchiveRequest {
                directive_index: 1,
                from: "Data/Missing.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            },
            ArchiveRequest {
                directive_index: 2,
                from: "Data/Missing.txt".to_string(),
                inner_path: None,
                kind: ArchiveRequestKind::Bytes,
            },
        ],
    )
    .unwrap_err();
    let msg = format!("{err:#}");

    assert!(msg.contains("2 requested entries missing"));
    assert!(msg.contains("1 unique"));
    assert!(msg.contains("Data/Missing.txt (x2)"));
}

#[test]
#[cfg(not(feature = "rar"))]
fn rar_magic_without_rar_feature_reports_explicit_error() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.archive");
    std::fs::write(&archive, b"Rar!\x1A\x07\x01\x00not a complete rar").unwrap();

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[ArchiveRequest {
            directive_index: 1,
            from: "file.txt".to_string(),
            inner_path: None,
            kind: ArchiveRequestKind::Bytes,
        }],
    )
    .unwrap_err();

    assert!(
        format!("{err:#}")
            .contains("RAR archive detected but modde-sources was built without the rar feature")
    );
}

#[test]
#[cfg(feature = "rar")]
fn rar_magic_with_archive_extension_routes_to_rar_reader() {
    let temp = tempfile::tempdir().unwrap();
    let archive = temp.path().join("fixture.archive");
    std::fs::write(&archive, b"Rar!\x1A\x07\x01\x00not a complete rar").unwrap();

    let err = ArchiveBatchExtractor::extract_selected(
        &archive,
        &[ArchiveRequest {
            directive_index: 1,
            from: "file.txt".to_string(),
            inner_path: None,
            kind: ArchiveRequestKind::Bytes,
        }],
    )
    .unwrap_err();
    let msg = format!("{err:#}");

    assert!(!msg.contains("unsupported archive format"), "{msg}");
    assert!(
        msg.contains("failed to open RAR archive") || msg.contains("requested entry missing"),
        "{msg}"
    );
}
