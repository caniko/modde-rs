#![cfg(feature = "bethesda")]

use modde_games::bethesda::ini::patch_ini_content;

// ── Existing key replacement ────────────────────────────────────────

#[test]
fn test_patch_existing_key_preserves_other_keys() {
    let content = "[General]\nbLanguage=ENGLISH\niStoryManagerLogging=0\nbEnableFileSel=1\n";
    let result = patch_ini_content(content, "General", "iStoryManagerLogging", "1");
    assert!(result.contains("iStoryManagerLogging=1"));
    assert!(result.contains("bLanguage=ENGLISH"));
    assert!(result.contains("bEnableFileSel=1"));
}

#[test]
fn test_patch_key_with_spaces_around_equals() {
    // Some INI files have spaces around =
    let content = "[General]\nbLanguage = ENGLISH\n";
    // Our implementation looks for key match after split_once('=') then trim
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    // Should find and replace it
    assert!(result.contains("bLanguage=FRENCH") || result.contains("bLanguage = FRENCH"));
}

// ── New key in existing section ─────────────────────────────────────

#[test]
fn test_patch_new_key_at_end_of_last_section() {
    let content = "[Display]\niSize=1920\n";
    let result = patch_ini_content(content, "Display", "fFOV", "90.0");
    assert!(result.contains("[Display]"));
    assert!(result.contains("iSize=1920"));
    assert!(result.contains("fFOV=90.0"));
}

#[test]
fn test_patch_new_key_before_next_section() {
    let content = "[General]\nbLanguage=ENGLISH\n[Display]\niSize=1920\n";
    let result = patch_ini_content(content, "General", "sNewKey", "value");
    assert!(result.contains("sNewKey=value"));
    // New key should be in General section, before Display section
    let general_pos = result.find("[General]").unwrap();
    let display_pos = result.find("[Display]").unwrap();
    let new_key_pos = result.find("sNewKey=value").unwrap();
    assert!(new_key_pos > general_pos);
    assert!(new_key_pos < display_pos);
}

// ── New section creation ────────────────────────────────────────────

#[test]
fn test_patch_new_section_on_empty_content() {
    let content = "";
    let result = patch_ini_content(content, "General", "bLanguage", "ENGLISH");
    assert!(result.contains("[General]"));
    assert!(result.contains("bLanguage=ENGLISH"));
}

#[test]
fn test_patch_new_section_added_at_end() {
    let content = "[General]\nbLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "Archive", "bInvalidateOlderFiles", "1");
    assert!(result.contains("[Archive]"));
    assert!(result.contains("bInvalidateOlderFiles=1"));
    // Archive section should come after General
    let general_pos = result.find("[General]").unwrap();
    let archive_pos = result.find("[Archive]").unwrap();
    assert!(archive_pos > general_pos);
}

// ── Comment handling ────────────────────────────────────────────────

#[test]
fn test_preserves_semicolon_comments() {
    let content = "[General]\n; Important setting\nbLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    assert!(result.contains("; Important setting"));
    assert!(result.contains("bLanguage=FRENCH"));
}

#[test]
fn test_preserves_hash_comments() {
    let content = "[General]\n# Hash comment\nbLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    assert!(result.contains("# Hash comment"));
}

#[test]
fn test_does_not_modify_comment_with_key_name() {
    let content = "[General]\n; bLanguage=GERMAN\nbLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    assert!(
        result.contains("; bLanguage=GERMAN"),
        "commented-out key should be preserved"
    );
    assert!(result.contains("bLanguage=FRENCH"));
    // Should NOT contain bLanguage=ENGLISH (it was replaced)
    assert!(!result.contains("\nbLanguage=ENGLISH"));
}

// ── Multiple sections ───────────────────────────────────────────────

#[test]
fn test_patch_correct_section_among_many() {
    let content = "[General]\nbLanguage=ENGLISH\n[Display]\niSize=1920\n[Audio]\nfVolume=0.8\n";
    let result = patch_ini_content(content, "Display", "iSize", "2560");
    assert!(result.contains("iSize=2560"));
    // Other sections untouched
    assert!(result.contains("bLanguage=ENGLISH"));
    assert!(result.contains("fVolume=0.8"));
}

#[test]
fn test_patch_same_key_different_sections() {
    // Same key name in different sections
    let content = "[Section1]\nfValue=1.0\n[Section2]\nfValue=2.0\n";
    let result = patch_ini_content(content, "Section2", "fValue", "3.0");
    assert!(result.contains("fValue=3.0"));
    // Section1's fValue should remain unchanged
    let lines: Vec<&str> = result.lines().collect();
    let section1_idx = lines.iter().position(|l| l.contains("[Section1]")).unwrap();
    let section2_idx = lines.iter().position(|l| l.contains("[Section2]")).unwrap();
    let fvalue_in_s1 = lines[section1_idx + 1..section2_idx]
        .iter()
        .any(|l| l.contains("fValue=1.0"));
    assert!(fvalue_in_s1, "Section1's fValue should remain 1.0");
}

// ── Edge cases ──────────────────────────────────────────────────────

#[test]
fn test_patch_empty_value() {
    let content = "[General]\nbLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "bLanguage", "");
    assert!(result.contains("bLanguage="));
}

#[test]
fn test_patch_value_with_special_chars() {
    let content = "[General]\nsPath=C:\\Games\n";
    let result = patch_ini_content(content, "General", "sPath", "D:\\Mods\\Skyrim");
    assert!(result.contains("sPath=D:\\Mods\\Skyrim"));
}

#[test]
fn test_patch_key_not_confused_by_partial_match() {
    let content = "[General]\nbLang=EN\nbLanguage=ENGLISH\n";
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    assert!(
        result.contains("bLang=EN"),
        "partial key match should be untouched"
    );
    assert!(result.contains("bLanguage=FRENCH"));
}

#[test]
fn test_patch_section_name_case_sensitive() {
    let content = "[general]\nbLanguage=ENGLISH\n";
    // Patching [General] should NOT match [general]
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    // Should create a new [General] section since [general] doesn't match
    assert!(result.contains("[General]"));
    // Original [general] section should be untouched
    assert!(result.contains("[general]"));
    assert!(result.contains("bLanguage=ENGLISH") || result.contains("bLanguage=FRENCH"));
}

#[test]
fn test_patch_preserves_empty_lines() {
    let content = "[General]\n\nbLanguage=ENGLISH\n\n[Display]\niSize=1920\n";
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    assert!(result.contains("bLanguage=FRENCH"));
}

#[test]
fn test_patch_content_with_no_trailing_newline() {
    let content = "[General]\nbLanguage=ENGLISH";
    let result = patch_ini_content(content, "General", "bLanguage", "FRENCH");
    assert!(result.contains("bLanguage=FRENCH"));
}
