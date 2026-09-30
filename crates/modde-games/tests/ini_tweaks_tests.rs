#![cfg(feature = "bethesda")]

use std::fs;

use modde_games::bethesda::ini_tweaks::{IniTweak, apply_ini_tweaks, scan_mod_ini_tweaks};
use tempfile::TempDir;

#[test]
fn test_scan_empty_dir() {
    let tmp = TempDir::new().unwrap();
    let tweaks = scan_mod_ini_tweaks("empty_mod", tmp.path()).unwrap();
    assert!(tweaks.is_empty());
}

#[test]
fn test_scan_single_ini() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("Skyrim.ini"),
        "[General]\nbLanguage=FRENCH\n",
    )
    .unwrap();

    let tweaks = scan_mod_ini_tweaks("my_mod", tmp.path()).unwrap();
    assert_eq!(tweaks.len(), 1);
    assert_eq!(tweaks[0].mod_id, "my_mod");
    assert_eq!(tweaks[0].ini_file, "Skyrim.ini");
    assert_eq!(tweaks[0].section, "General");
    assert_eq!(tweaks[0].key, "bLanguage");
    assert_eq!(tweaks[0].value, "FRENCH");
}

#[test]
fn test_scan_multiple_sections() {
    let tmp = TempDir::new().unwrap();
    let content = "\
[General]
bLanguage=FRENCH
iStoryManagerLogging=1

[Display]
iSize W=1920
iSize H=1080
";
    fs::write(tmp.path().join("Skyrim.ini"), content).unwrap();

    let tweaks = scan_mod_ini_tweaks("multi_mod", tmp.path()).unwrap();
    assert_eq!(tweaks.len(), 4);

    let general: Vec<_> = tweaks.iter().filter(|t| t.section == "General").collect();
    assert_eq!(general.len(), 2);

    let display: Vec<_> = tweaks.iter().filter(|t| t.section == "Display").collect();
    assert_eq!(display.len(), 2);
    assert!(
        display
            .iter()
            .any(|t| t.key == "iSize W" && t.value == "1920")
    );
}

#[test]
fn test_apply_tweaks() {
    let tmp = TempDir::new().unwrap();
    let ini_path = tmp.path().join("Skyrim.ini");
    fs::write(&ini_path, "[General]\nbLanguage=ENGLISH\n").unwrap();

    let tweaks = vec![IniTweak {
        mod_id: "test_mod".to_string(),
        ini_file: "Skyrim.ini".to_string(),
        section: "General".to_string(),
        key: "bLanguage".to_string(),
        value: "FRENCH".to_string(),
    }];

    let count = apply_ini_tweaks(&tweaks, tmp.path()).unwrap();
    assert_eq!(count, 1);

    let result = fs::read_to_string(&ini_path).unwrap();
    assert!(result.contains("bLanguage=FRENCH"));
    assert!(!result.contains("ENGLISH"));
}

#[test]
fn test_scan_nonexistent_dir() {
    let tweaks =
        scan_mod_ini_tweaks("ghost", std::path::Path::new("/tmp/does_not_exist_xyz")).unwrap();
    assert!(tweaks.is_empty());
}

#[test]
fn test_scan_ignores_comments() {
    let tmp = TempDir::new().unwrap();
    let content = "\
[General]
; this is a comment
# this is also a comment
bLanguage=FRENCH
";
    fs::write(tmp.path().join("Test.ini"), content).unwrap();

    let tweaks = scan_mod_ini_tweaks("comment_mod", tmp.path()).unwrap();
    assert_eq!(tweaks.len(), 1);
    assert_eq!(tweaks[0].key, "bLanguage");
}
