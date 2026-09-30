#![cfg(feature = "bethesda")]

use std::path::Path;

use modde_games::bethesda::archives::{BSA_EXTENSIONS, is_archive, staging_path};

// ── is_archive tests ────────────────────────────────────────────────

#[test]
fn test_is_archive_bsa() {
    assert!(is_archive(Path::new("Skyrim - Textures.bsa")));
}

#[test]
fn test_is_archive_ba2() {
    assert!(is_archive(Path::new("Fallout4 - Textures.ba2")));
}

#[test]
fn test_is_archive_bsa_uppercase() {
    assert!(is_archive(Path::new("Skyrim.BSA")));
}

#[test]
fn test_is_archive_ba2_mixed_case() {
    assert!(is_archive(Path::new("Fallout.Ba2")));
}

#[test]
fn test_is_archive_not_archive_esp() {
    assert!(!is_archive(Path::new("mod.esp")));
}

#[test]
fn test_is_archive_not_archive_esm() {
    assert!(!is_archive(Path::new("Skyrim.esm")));
}

#[test]
fn test_is_archive_not_archive_dds() {
    assert!(!is_archive(Path::new("texture.dds")));
}

#[test]
fn test_is_archive_not_archive_nif() {
    assert!(!is_archive(Path::new("mesh.nif")));
}

#[test]
fn test_is_archive_no_extension() {
    assert!(!is_archive(Path::new("somefile")));
}

#[test]
fn test_is_archive_empty_name() {
    assert!(!is_archive(Path::new("")));
}

#[test]
fn test_is_archive_with_full_path() {
    assert!(is_archive(Path::new("/game/Data/Skyrim - Textures.bsa")));
    assert!(is_archive(Path::new("mods/textures/content.ba2")));
}

#[test]
fn test_is_archive_dot_only() {
    assert!(!is_archive(Path::new(".")));
    assert!(!is_archive(Path::new(".bsa"))); // hidden file named .bsa (extension is bsa, but file starts with .)
}

#[test]
fn test_is_archive_zip_not_archive() {
    assert!(!is_archive(Path::new("mod.zip")));
    assert!(!is_archive(Path::new("mod.7z")));
    assert!(!is_archive(Path::new("mod.rar")));
}

// ── staging_path tests ──────────────────────────────────────────────

#[test]
fn test_staging_path_basic() {
    let result = staging_path(Path::new("/staging"), "Skyrim.bsa");
    assert_eq!(result, Path::new("/staging/Skyrim.bsa"));
}

#[test]
fn test_staging_path_preserves_name() {
    let result = staging_path(Path::new("/game/Data"), "Textures01.bsa");
    assert_eq!(result, Path::new("/game/Data/Textures01.bsa"));
}

#[test]
fn test_staging_path_nested_root() {
    let result = staging_path(Path::new("/a/b/c/staging"), "mod.ba2");
    assert_eq!(result, Path::new("/a/b/c/staging/mod.ba2"));
}

// ── BSA_EXTENSIONS constant ─────────────────────────────────────────

#[test]
fn test_bsa_extensions_contains_expected() {
    assert!(BSA_EXTENSIONS.contains(&".bsa"));
    assert!(BSA_EXTENSIONS.contains(&".ba2"));
    assert_eq!(BSA_EXTENSIONS.len(), 2);
}
