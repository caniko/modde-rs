use super::*;
use crate::wabbajack::staging::{StagingPrepareStatus, StagingStore, compressed_path};

#[test]
fn validate_archive_entry_rejects_traversal_and_absolute() {
    // The batch FromArchive path (install_directives) relies on this to block
    // zip-slip via a malicious manifest `to` field.
    assert!(validate_archive_entry("mods/Foo/textures/x.dds").is_ok());
    assert!(validate_archive_entry("../escape.txt").is_err());
    assert!(validate_archive_entry("a/b/../../../escape").is_err());
    assert!(validate_archive_entry("..\\escape.txt").is_err());
    assert!(validate_archive_entry("/etc/passwd").is_err());
}
use std::collections::HashMap;
use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use xxhash_rust::xxh64::xxh64;

/// Helper: create a zip file on disk with the given entries.
fn create_zip_file(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let file = std::fs::File::create(path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for (name, data) in entries {
        writer.start_file(*name, options).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap();
}

fn zip_bytes(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, data) in entries {
            writer.start_file(*name, options).unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap();
    }
    cursor.into_inner()
}

fn data_patch(data: &[u8]) -> Vec<u8> {
    let mut patch = Vec::new();
    patch.extend_from_slice(b"OCTODELTA");
    patch.push(1);
    patch.push(4);
    patch.extend_from_slice(b"SHA1");
    patch.extend_from_slice(&20_u32.to_le_bytes());
    patch.extend_from_slice(&[0_u8; 20]);
    patch.extend_from_slice(b">>>");
    patch.push(0x80);
    patch.extend_from_slice(&(data.len() as u64).to_le_bytes());
    patch.extend_from_slice(data);
    patch
}

/// Helper: open a zip for `find_entry_in_archive` tests.
fn open_zip(path: &std::path::Path) -> zip::ZipArchive<std::fs::File> {
    let file = std::fs::File::open(path).unwrap();
    zip::ZipArchive::new(file).unwrap()
}

fn minimal_manifest() -> WabbajackManifest {
    WabbajackManifest {
        name: "test".into(),
        author: "a".into(),
        description: "d".into(),
        game: "SkyrimSE".into(),
        version: "1.0".into(),
        archives: vec![],
        directives: vec![],
    }
}

fn game_file_archive(hash: u64, rel_path: &str) -> modde_core::manifest::wabbajack::ArchiveEntry {
    modde_core::manifest::wabbajack::ArchiveEntry {
        hash,
        name: rel_path.replace(['\\', '/'], "_"),
        size: 0,
        state: Some(ArchiveState::GameFileSourceDownloader {
            metadata: HashMap::from([(
                "File".to_string(),
                serde_json::Value::String(rel_path.to_string()),
            )]),
        }),
    }
}

fn manifest_with_game_file(hash: u64, rel_path: &str, to: &str) -> WabbajackManifest {
    WabbajackManifest {
        archives: vec![game_file_archive(hash, rel_path)],
        directives: vec![modde_core::manifest::wabbajack::RawDirective::FromArchive {
            archive_hash_path: vec![serde_json::Value::Number(hash.into())],
            to: to.into(),
            size: 0,
        }],
        ..minimal_manifest()
    }
}

fn manifest_with_nexus_download(hash: u64) -> WabbajackManifest {
    WabbajackManifest {
        archives: vec![modde_core::manifest::wabbajack::ArchiveEntry {
            hash,
            name: "nexus archive".into(),
            size: 1,
            state: Some(ArchiveState::NexusDownloader {
                game_name: "skyrimspecialedition".into(),
                mod_id: 123.into(),
                file_id: 456.into(),
            }),
        }],
        ..minimal_manifest()
    }
}

mod admission;
mod game_files;
mod helpers;
mod pipeline;
mod trust;
