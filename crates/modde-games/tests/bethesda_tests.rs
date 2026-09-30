#![cfg(feature = "bethesda")]

use modde_core::scanner::ModFootprint;
use modde_games::bethesda::scanner::{
    FALLOUT4_SCANNER, FALLOUT76_SCANNER, SKYRIM_SCANNER, STARFIELD_SCANNER,
};
use modde_games::traits::{ModScanner, ModSource, ScanContext};
use tempfile::TempDir;

// ── BethesdaScanner: mod_id_footprint inverse ───────────────────────

#[test]
fn test_skyrim_footprint_round_trip() {
    let fp = SKYRIM_SCANNER
        .mod_id_footprint("plugin/SkyUI_SE.esp")
        .unwrap();
    assert_eq!(fp, ModFootprint::File("skyui_se.esp".to_string()));
}

#[test]
fn test_skyrim_footprint_lowercases() {
    // Manifest `To` paths are lowercased in detect_stale_duplicates,
    // so our footprint must be lowercase to match.
    let fp = SKYRIM_SCANNER
        .mod_id_footprint("plugin/UNPLUGIN.ESM")
        .unwrap();
    assert_eq!(fp, ModFootprint::File("unplugin.esm".to_string()));
}

#[test]
fn test_fallout4_footprint_round_trip() {
    let fp = FALLOUT4_SCANNER
        .mod_id_footprint("plugin/UnofficialFallout4Patch.esp")
        .unwrap();
    assert_eq!(
        fp,
        ModFootprint::File("unofficialfallout4patch.esp".to_string())
    );
}

#[test]
fn test_fallout76_scanner_detects_data_ba2_files() {
    let td = TempDir::new().unwrap();
    let data_dir = td.path().join("Data");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::write(data_dir.join("BetterInventory.ba2"), b"").unwrap();
    std::fs::write(data_dir.join("SeventySix - 00UpdateMain.ba2"), b"").unwrap();
    std::fs::write(data_dir.join("readme.txt"), b"").unwrap();

    let mods = FALLOUT76_SCANNER
        .scan_filesystem(&ScanContext {
            install_dir: td.path(),
        })
        .unwrap();

    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].mod_id, "archive/BetterInventory");
    assert_eq!(mods[0].display_name, "BetterInventory");
    assert_eq!(mods[0].files[0].rel_path, "Data/BetterInventory.ba2");
    match &mods[0].source {
        ModSource::Filesystem { location } => assert_eq!(location, "Data"),
        ModSource::Archive { .. } => panic!("expected filesystem source"),
    }
}

#[test]
fn test_fallout76_scanner_footprint_round_trip() {
    let fp = FALLOUT76_SCANNER
        .mod_id_footprint("archive/BetterInventory")
        .unwrap();
    assert_eq!(
        fp,
        ModFootprint::File("data/betterinventory.ba2".to_string())
    );
}

#[test]
fn test_starfield_footprint_round_trip() {
    let fp = STARFIELD_SCANNER
        .mod_id_footprint("plugin/StarfieldExtender.esm")
        .unwrap();
    assert_eq!(fp, ModFootprint::File("starfieldextender.esm".to_string()));
}

#[test]
fn test_bethesda_footprint_handles_esl() {
    // Light plugins (.esl) are first-class alongside .esp / .esm.
    let fp = SKYRIM_SCANNER
        .mod_id_footprint("plugin/MyLightMod.esl")
        .unwrap();
    assert_eq!(fp, ModFootprint::File("mylightmod.esl".to_string()));
}

#[test]
fn test_bethesda_footprint_rejects_unknown_prefix() {
    assert!(SKYRIM_SCANNER.mod_id_footprint("pak/Foo").is_none());
    assert!(SKYRIM_SCANNER.mod_id_footprint("cet/Foo").is_none());
    assert!(SKYRIM_SCANNER.mod_id_footprint("bare").is_none());
    // `nexus_*` rows are manifest-authored; the scanner must return None so
    // detect_stale_duplicates skips them entirely.
    assert!(
        SKYRIM_SCANNER
            .mod_id_footprint("nexus_skyrimspecialedition_42_100")
            .is_none()
    );
}
