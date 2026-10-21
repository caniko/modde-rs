#![allow(clippy::wildcard_imports)]
use super::*;
use std::io::Write as _;

// ── parse_nexus_url ──────────────────────────────────────────────

#[test]
fn parse_nexus_url_basic_mod_url() {
    let (game, mod_id, file_id) =
        parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/12345").unwrap();
    assert_eq!(game, "skyrimspecialedition");
    assert_eq!(mod_id, NexusModId::from(12345));
    assert_eq!(file_id, None);
}

#[test]
fn parse_nexus_url_with_file_id() {
    let (game, mod_id, file_id) = parse_nexus_url(
        "https://www.nexusmods.com/skyrimspecialedition/mods/12345?tab=files&file_id=67890",
    )
    .unwrap();
    assert_eq!(game, "skyrimspecialedition");
    assert_eq!(mod_id, NexusModId::from(12345));
    assert_eq!(file_id, Some(NexusFileId::from(67890)));
}

#[test]
fn parse_nexus_url_file_id_only_query_param() {
    let (game, mod_id, file_id) =
        parse_nexus_url("https://www.nexusmods.com/fallout4/mods/999?file_id=42").unwrap();
    assert_eq!(game, "fallout4");
    assert_eq!(mod_id, NexusModId::from(999));
    assert_eq!(file_id, Some(NexusFileId::from(42)));
}

#[test]
fn parse_nexus_url_trailing_slash() {
    // Trailing slash means the 4th segment is empty, but segments[0..3] still work.
    let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/12345/");
    assert!(result.is_ok());
    let (game, mod_id, _) = result.unwrap();
    assert_eq!(game, "skyrimspecialedition");
    assert_eq!(mod_id, NexusModId::from(12345));
}

#[test]
fn parse_nexus_url_invalid_not_a_url() {
    let result = parse_nexus_url("not-a-url");
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_invalid_wrong_path_structure() {
    let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition");
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_invalid_empty_game_domain() {
    let result = parse_nexus_url("https://www.nexusmods.com//mods/12345");
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_invalid_missing_mods_segment() {
    let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/files/12345");
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_invalid_extra_path_segments() {
    let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/12345/files");
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_invalid_non_numeric_mod_id() {
    let result = parse_nexus_url("https://www.nexusmods.com/skyrimspecialedition/mods/abc");
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_different_game_domains() {
    for domain in &["fallout4", "cyberpunk2077", "morrowind", "oblivion"] {
        let url = format!("https://www.nexusmods.com/{domain}/mods/1");
        let (game, mod_id, _) = parse_nexus_url(&url).unwrap();
        assert_eq!(game, *domain);
        assert_eq!(mod_id, NexusModId::from(1));
    }
}

// ── find_fomod_config / has_fomod ────────────────────────────────

#[test]
fn find_fomod_config_exact_case() {
    let tmp = tempfile::tempdir().unwrap();
    let fomod = tmp.path().join("fomod");
    std::fs::create_dir_all(&fomod).unwrap();
    std::fs::write(fomod.join("ModuleConfig.xml"), "<config/>").unwrap();

    let result = find_fomod_config(tmp.path());
    assert!(result.is_some());
    assert!(result.unwrap().ends_with("ModuleConfig.xml"));
}

#[test]
fn find_fomod_config_uppercase_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let fomod = tmp.path().join("FOMOD");
    std::fs::create_dir_all(&fomod).unwrap();
    std::fs::write(fomod.join("moduleconfig.xml"), "<config/>").unwrap();

    let result = find_fomod_config(tmp.path());
    assert!(result.is_some());
}

#[test]
fn find_fomod_config_mixed_case() {
    let tmp = tempfile::tempdir().unwrap();
    let fomod = tmp.path().join("FoMod");
    std::fs::create_dir_all(&fomod).unwrap();
    std::fs::write(fomod.join("MODULECONFIG.XML"), "<config/>").unwrap();

    let result = find_fomod_config(tmp.path());
    assert!(result.is_some());
}

#[test]
fn find_fomod_config_not_present() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("data")).unwrap();

    let result = find_fomod_config(tmp.path());
    assert!(result.is_none());
}

#[test]
fn find_fomod_config_empty_fomod_dir() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("fomod")).unwrap();
    // No ModuleConfig.xml inside

    let result = find_fomod_config(tmp.path());
    assert!(result.is_none());
}

#[test]
fn has_fomod_returns_true_when_present() {
    let tmp = tempfile::tempdir().unwrap();
    let fomod = tmp.path().join("fomod");
    std::fs::create_dir_all(&fomod).unwrap();
    std::fs::write(fomod.join("ModuleConfig.xml"), "<config/>").unwrap();

    assert!(find_fomod_config(tmp.path()).is_some());
}

#[test]
fn has_fomod_returns_false_when_missing() {
    let tmp = tempfile::tempdir().unwrap();
    assert!(find_fomod_config(tmp.path()).is_none());
}

// ── extract_archive ──────────────────────────────────────────────

fn create_test_zip(dir: &Path, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
    let zip_path = dir.join(name);
    let file = std::fs::File::create(&zip_path).unwrap();
    let mut writer = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);

    for (entry_name, content) in entries {
        writer.start_file(entry_name.to_string(), options).unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap();
    zip_path
}

#[test]
fn extract_archive_simple_zip() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = create_test_zip(
        tmp.path(),
        "test.zip",
        &[
            ("hello.txt", b"Hello, world!"),
            ("data/readme.md", b"# Readme"),
        ],
    );

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    extract_archive(&zip_path, &dest).unwrap();

    assert_eq!(
        std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
        "Hello, world!"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("data/readme.md")).unwrap(),
        "# Readme"
    );
}

#[test]
fn extract_archive_nested_directories() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = create_test_zip(
        tmp.path(),
        "nested.zip",
        &[
            ("a/b/c/deep.txt", b"deep content"),
            ("top.txt", b"top content"),
        ],
    );

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    extract_archive(&zip_path, &dest).unwrap();

    assert!(dest.join("a/b/c/deep.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("a/b/c/deep.txt")).unwrap(),
        "deep content"
    );
}

#[test]
fn extract_archive_empty_zip() {
    let tmp = tempfile::tempdir().unwrap();
    let zip_path = create_test_zip(tmp.path(), "empty.zip", &[]);

    let dest = tmp.path().join("out");
    std::fs::create_dir_all(&dest).unwrap();
    extract_archive(&zip_path, &dest).unwrap();

    // dest exists but is empty
    let count = std::fs::read_dir(&dest).unwrap().count();
    assert_eq!(count, 0);
}

#[test]
fn extract_archive_nonexistent_file() {
    let tmp = tempfile::tempdir().unwrap();
    let result = extract_archive(&tmp.path().join("noexist.zip"), &tmp.path().join("out"));
    assert!(result.is_err());
}

#[test]
fn parse_nexus_url_rejects_unsafe_game_domains() {
    for url in [
        "https://www.nexusmods.com/sky.rim/mods/1",
        "https://www.nexusmods.com/%2E%2E%2Fhome/mods/1",
        "https://www.nexusmods.com/sky%20rim/mods/1",
    ] {
        assert!(parse_nexus_url(url).is_err(), "{url} should be rejected");
    }
}
