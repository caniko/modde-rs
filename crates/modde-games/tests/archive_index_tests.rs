#![cfg(feature = "bethesda")]

use std::io::Write;

use modde_games::bethesda::archive_index::ArchiveIndex;
use tempfile::NamedTempFile;

/// Build a minimal v105 BSA in memory with the given folder->files mapping.
fn build_test_bsa(folders: &[(&str, &[(&str, u32)])]) -> Vec<u8> {
    let mut buf = Vec::new();

    let folder_count = folders.len() as u32;
    let file_count: u32 = folders.iter().map(|(_, files)| files.len() as u32).sum();

    let total_folder_name_len: u32 = folders.iter().map(|(name, _)| name.len() as u32 + 2).sum();

    let _total_file_name_len: u32 = folders
        .iter()
        .flat_map(|(_, files)| files.iter())
        .map(|(name, _)| name.len() as u32 + 1)
        .sum();

    // Magic
    buf.extend_from_slice(b"BSA\0");
    buf.extend_from_slice(&105u32.to_le_bytes());
    buf.extend_from_slice(&36u32.to_le_bytes());
    buf.extend_from_slice(&0x03u32.to_le_bytes());
    buf.extend_from_slice(&folder_count.to_le_bytes());
    buf.extend_from_slice(&file_count.to_le_bytes());
    buf.extend_from_slice(&total_folder_name_len.to_le_bytes());
    buf.extend_from_slice(&_total_file_name_len.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Folder records (v105)
    for (_, files) in folders {
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&(files.len() as u32).to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
    }

    // File record blocks
    for (folder_name, files) in folders {
        let bstr_len = (folder_name.len() + 1) as u8;
        buf.push(bstr_len);
        buf.extend_from_slice(folder_name.as_bytes());
        buf.push(0);

        for (_, size) in *files {
            buf.extend_from_slice(&0u64.to_le_bytes());
            buf.extend_from_slice(&size.to_le_bytes());
            buf.extend_from_slice(&0u32.to_le_bytes());
        }
    }

    // File name block
    for (_, files) in folders {
        for (name, _) in *files {
            buf.extend_from_slice(name.as_bytes());
            buf.push(0);
        }
    }

    buf
}

/// Build a minimal GNRL BA2 in memory.
fn build_test_ba2(files: &[(&str, u32)]) -> Vec<u8> {
    let mut buf = Vec::new();
    let file_count = files.len() as u32;

    buf.extend_from_slice(b"BTDX");
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(b"GNRL");
    buf.extend_from_slice(&file_count.to_le_bytes());

    let header_size: u64 = 24;
    let records_size: u64 = u64::from(file_count) * 36;
    let name_table_offset = header_size + records_size;
    buf.extend_from_slice(&name_table_offset.to_le_bytes());

    for (_, size) in files {
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&0u64.to_le_bytes());
        buf.extend_from_slice(&0u32.to_le_bytes());
        buf.extend_from_slice(&size.to_le_bytes());
        buf.extend_from_slice(&0xBAADF00Du32.to_le_bytes());
    }

    for (name, _) in files {
        let len = name.len() as u16;
        buf.extend_from_slice(&len.to_le_bytes());
        buf.extend_from_slice(name.as_bytes());
    }

    buf
}

fn write_temp(data: &[u8]) -> NamedTempFile {
    let mut tmp = NamedTempFile::new().unwrap();
    tmp.write_all(data).unwrap();
    tmp.flush().unwrap();
    tmp
}

#[test]
fn read_bsa_via_archive_index() {
    let data = build_test_bsa(&[
        ("textures", &[("sky.dds", 4096), ("ground.dds", 8192)]),
        ("meshes", &[("tree.nif", 2048)]),
    ]);
    let tmp = write_temp(&data);

    let index = ArchiveIndex::read(tmp.path()).unwrap();
    assert_eq!(index.files.len(), 3);

    let paths: Vec<&str> = index.files.iter().map(|e| e.path.as_str()).collect();
    assert!(paths.contains(&"textures/sky.dds"));
    assert!(paths.contains(&"textures/ground.dds"));
    assert!(paths.contains(&"meshes/tree.nif"));
}

#[test]
fn read_ba2_via_archive_index() {
    let data = build_test_ba2(&[
        ("Data\\Textures\\sky.dds", 4096),
        ("Data\\Meshes\\tree.nif", 2048),
    ]);
    let tmp = write_temp(&data);

    let index = ArchiveIndex::read(tmp.path()).unwrap();
    assert_eq!(index.files.len(), 2);
    // Paths should be normalized to forward-slash lowercase.
    assert_eq!(index.files[0].path, "data/textures/sky.dds");
    assert_eq!(index.files[1].path, "data/meshes/tree.nif");
}

#[test]
fn archive_path_is_preserved() {
    let data = build_test_bsa(&[("meshes", &[("rock.nif", 100)])]);
    let tmp = write_temp(&data);

    let index = ArchiveIndex::read(tmp.path()).unwrap();
    assert_eq!(index.archive_path, tmp.path());
}

#[test]
fn invalid_file_rejected() {
    let tmp = write_temp(b"this is not an archive");
    let result = ArchiveIndex::read(tmp.path());
    assert!(result.is_err());
}
