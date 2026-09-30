#![cfg(feature = "bethesda")]

//! Comprehensive INI patching tests covering edge cases.

use modde_games::bethesda::ini::patch_ini_content;

#[test]
fn test_patch_existing_key_in_existing_section() {
    let content = "[General]\nfDefaultFOV=65\nsLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "fDefaultFOV", "90");
    assert!(result.contains("fDefaultFOV=90"));
    assert!(result.contains("sLanguage=ENGLISH"));
}

#[test]
fn test_patch_adds_key_to_existing_section() {
    let content = "[General]\nsLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "fDefaultFOV", "90");
    assert!(result.contains("fDefaultFOV=90"));
    assert!(result.contains("sLanguage=ENGLISH"));
}

#[test]
fn test_patch_creates_new_section() {
    let content = "[General]\nsLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "Display", "iSize W", "1920");
    assert!(result.contains("[Display]"));
    assert!(result.contains("iSize W=1920"));
    assert!(result.contains("[General]"));
}

#[test]
fn test_patch_empty_content() {
    let content = "";
    let result = patch_ini_content(content, "Section", "key", "value");
    assert!(result.contains("[Section]"));
    assert!(result.contains("key=value"));
}

#[test]
fn test_patch_preserves_comments_semicolon() {
    let content = "; This is a comment\n[General]\n; Another comment\nfDefaultFOV=65\n";
    let result = patch_ini_content(content, "General", "fDefaultFOV", "90");
    assert!(result.contains("; This is a comment"));
    assert!(result.contains("; Another comment"));
    assert!(result.contains("fDefaultFOV=90"));
}

#[test]
fn test_patch_preserves_comments_hash() {
    let content = "# Hash comment\n[General]\n# Another\nfDefaultFOV=65\n";
    let result = patch_ini_content(content, "General", "fDefaultFOV", "90");
    assert!(result.contains("# Hash comment"));
    assert!(result.contains("# Another"));
}

#[test]
fn test_patch_handles_spaces_around_equals() {
    let content = "[General]\nfDefaultFOV = 65\n";
    let result = patch_ini_content(content, "General", "fDefaultFOV", "90");
    assert!(result.contains("fDefaultFOV=90") || result.contains("fDefaultFOV = 90"));
}

#[test]
fn test_patch_does_not_partial_match_key() {
    let content = "[General]\nfDefaultFOV=65\nfDefaultFOVSomething=100\n";
    let result = patch_ini_content(content, "General", "fDefaultFOV", "90");
    // Should only change fDefaultFOV, not fDefaultFOVSomething
    assert!(result.contains("fDefaultFOVSomething=100"));
}

#[test]
fn test_patch_multiple_sections_targets_correct_one() {
    let content = "[Section1]\nkey=val1\n[Section2]\nkey=val2\n[Section3]\nkey=val3\n";
    let result = patch_ini_content(content, "Section2", "key", "updated");
    assert!(result.contains("[Section1]"));
    // Section2's key should be updated
    let lines: Vec<&str> = result.lines().collect();
    let s2_idx = lines.iter().position(|l| *l == "[Section2]").unwrap();
    // Next non-empty line after [Section2] should have the updated value
    let next_key_line = lines[s2_idx + 1..]
        .iter()
        .find(|l| l.starts_with("key="))
        .unwrap();
    assert_eq!(*next_key_line, "key=updated");
}

#[test]
fn test_patch_same_key_different_sections() {
    let content = "[SectionA]\nresolution=1080\n[SectionB]\nresolution=720\n";
    let result = patch_ini_content(content, "SectionB", "resolution", "1440");
    // SectionA's resolution should be unchanged
    let lines: Vec<&str> = result.lines().collect();
    let sa_idx = lines.iter().position(|l| *l == "[SectionA]").unwrap();
    let sa_key = lines[sa_idx + 1..]
        .iter()
        .find(|l| l.starts_with("resolution="))
        .unwrap();
    assert_eq!(*sa_key, "resolution=1080");
}

#[test]
fn test_patch_empty_value() {
    let content = "[General]\nkey=old_value\n";
    let result = patch_ini_content(content, "General", "key", "");
    assert!(result.contains("key="));
}

#[test]
fn test_patch_value_with_special_chars() {
    let content = "[Paths]\nbase_dir=C:\\Games\n";
    let result = patch_ini_content(content, "Paths", "base_dir", "D:\\Mods\\Skyrim");
    assert!(result.contains("base_dir=D:\\Mods\\Skyrim"));
}

#[test]
fn test_patch_section_case_sensitive() {
    let content = "[general]\nkey=value\n[General]\nkey=other\n";
    let result = patch_ini_content(content, "General", "key", "changed");
    // Should only change [General], not [general]
    assert!(result.contains("[general]"));
    assert!(result.contains("[General]"));
}

#[test]
fn test_patch_result_is_nonempty() {
    let content = "[General]\nkey=value\n";
    let result = patch_ini_content(content, "General", "key", "new");
    assert!(!result.is_empty());
    assert!(result.contains("key=new"));
}

#[test]
fn test_patch_file_io() {
    use modde_games::bethesda::ini::patch_ini;

    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("test.ini");
    std::fs::write(&path, "[General]\nfDefaultFOV=65\n").unwrap();

    patch_ini(&path, "General", "fDefaultFOV", "90").unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("fDefaultFOV=90"));
}

#[test]
fn test_patch_file_creates_section() {
    use modde_games::bethesda::ini::patch_ini;

    let tmp = tempfile::TempDir::new().unwrap();
    let path = tmp.path().join("test.ini");
    std::fs::write(&path, "").unwrap();

    patch_ini(&path, "NewSection", "newkey", "newval").unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("[NewSection]"));
    assert!(content.contains("newkey=newval"));
}
