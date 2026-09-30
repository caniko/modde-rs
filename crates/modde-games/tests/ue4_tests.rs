#![cfg(feature = "ue4")]

use std::path::Path;

use modde_core::collision::CollisionSeverity;
use modde_games::traits::ContentCategory;
use modde_games::ue4::scanner::{STELLAR_BLADE_SCANNER, Ue4Scanner};
use modde_games::ue4::{STELLAR_BLADE, Ue4Game};
use modde_games::{GamePlugin, ModScanner, ModSource, SUPPORTED_GAME_IDS, ScanContext};
use tempfile::TempDir;

// ── GamePlugin: identity ────────────────────────────────────────────

#[test]
fn test_stellar_blade_game_id() {
    assert_eq!(STELLAR_BLADE.game_id(), "stellar-blade");
}

#[test]
fn test_stellar_blade_display_name() {
    assert_eq!(STELLAR_BLADE.display_name(), "Stellar Blade");
}

#[test]
fn test_stellar_blade_steam_app_id_u32() {
    assert_eq!(STELLAR_BLADE.steam_app_id_u32(), Some(3489700));
}

#[test]
fn test_stellar_blade_nexus_domain_set() {
    assert_eq!(STELLAR_BLADE.nexus_game_domain(), Some("stellarblade"));
}

#[test]
fn test_stellar_blade_advertises_user_config_target() {
    use modde_games::DeployTargetKind;

    let targets = STELLAR_BLADE.deploy_targets();
    let cfg = targets
        .iter()
        .find(|t| t.kind == DeployTargetKind::UserConfig)
        .expect("stellar blade should advertise a UserConfig deploy target");
    assert_eq!(cfg.id, "ue4-saved-config");
}

#[test]
fn test_stellar_blade_save_profiles_enabled() {
    assert!(STELLAR_BLADE.supports_save_profiles());
}

#[test]
fn test_ue4_game_can_opt_into_save_profiles() {
    const TEST_UE4: Ue4Game =
        Ue4Game::new("test-ue4", "Test UE4", "1", "Test", None).with_save_profiles(true);

    assert!(TEST_UE4.supports_save_profiles());
}

// ── GamePlugin: paths ───────────────────────────────────────────────

#[test]
fn user_configuration_follows_secondary_library_and_explicit_prefix() {
    let root = TempDir::new().unwrap();
    let install = root.path().join("secondary/steamapps/common/Stellar Blade");
    let steam_prefix = root.path().join("secondary/steamapps/compatdata/3489700/pfx");
    let custom_prefix = root.path().join("custom-prefix");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&steam_prefix).unwrap();
    std::fs::create_dir_all(&custom_prefix).unwrap();
    let relative = "drive_c/users/steamuser/AppData/Local/SB/Saved/Config/Windows";
    assert_eq!(STELLAR_BLADE.resolve_deploy_target("ue4-saved-config", &install), Some(steam_prefix.join(relative)));
    assert_eq!(STELLAR_BLADE.resolve_deploy_target_at("ue4-saved-config", &install, Some(&custom_prefix)), Some(custom_prefix.join(relative)));
    assert_eq!(STELLAR_BLADE.save_directory_at(&install, Some(&custom_prefix)), Some(custom_prefix.join("drive_c/users/steamuser/AppData/Local/SB/Saved/SaveGames")));
    assert!(STELLAR_BLADE.resolve_deploy_target("ue4-saved-config", &root.path().join("local-copy")).is_none());
}

#[test]
fn test_stellar_blade_mod_directory() {
    let install = Path::new("/fake/game/Stellar Blade");
    let mod_dir = STELLAR_BLADE.mod_directory(install);
    assert_eq!(
        mod_dir,
        install
            .join("SB")
            .join("Content")
            .join("Paks")
            .join("~mods")
    );
}

#[test]
fn test_stellar_blade_executable_dir() {
    let install = Path::new("/fake/game/Stellar Blade");
    let exe_dir = STELLAR_BLADE.executable_dir(install);
    assert_eq!(exe_dir, install.join("SB").join("Binaries").join("Win64"));
}

#[test]
fn test_stellar_blade_paks_root() {
    let install = Path::new("/fake/game/Stellar Blade");
    assert_eq!(
        STELLAR_BLADE.paks_root(install),
        install.join("SB").join("Content").join("Paks")
    );
}

// ── Extension / archive metadata ────────────────────────────────────

#[test]
fn test_stellar_blade_classify_extension_pak() {
    assert_eq!(
        STELLAR_BLADE.classify_extension("pak"),
        ContentCategory::Archive
    );
    assert_eq!(
        STELLAR_BLADE.classify_extension("ucas"),
        ContentCategory::Archive
    );
    assert_eq!(
        STELLAR_BLADE.classify_extension("utoc"),
        ContentCategory::Archive
    );
}

#[test]
fn test_stellar_blade_classify_extension_dll_binary() {
    assert_eq!(
        STELLAR_BLADE.classify_extension("dll"),
        ContentCategory::Binary
    );
}

#[test]
fn test_stellar_blade_classify_extension_lua_script() {
    assert_eq!(
        STELLAR_BLADE.classify_extension("lua"),
        ContentCategory::Script
    );
}

#[test]
fn test_stellar_blade_archive_extensions() {
    assert_eq!(STELLAR_BLADE.archive_extensions(), &["pak", "ucas", "utoc"]);
}

// ── Save tracking ──────────────────────────────────────────────────

#[test]
fn test_stellar_blade_save_tracker_detects_nested_sav_files() {
    let td = TempDir::new().unwrap();
    let save_root = td.path().join("SaveGames");
    write_empty(&save_root.join("76561198000000000/StellarBladeSave00.sav"));
    write_empty(&save_root.join("StellarBladeSave01.sav"));
    write_empty(&save_root.join("76561198000000000/readme.txt"));

    let tracker = modde_games::resolve_save_tracker("stellar-blade").unwrap();
    let saves = tracker.detect_saves(&save_root).unwrap();
    let mut paths: Vec<_> = saves
        .iter()
        .map(|save| save.rel_path.to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();

    assert_eq!(
        paths,
        vec![
            "76561198000000000/StellarBladeSave00.sav".to_string(),
            "StellarBladeSave01.sav".to_string(),
        ]
    );
    assert!(saves.iter().all(|save| save.category == "manual"));
}

// ── Wine DLL overrides (install dir) ────────────────────────────────

#[test]
fn test_wine_overrides_empty_when_no_dlls() {
    let td = TempDir::new().unwrap();
    std::fs::create_dir_all(td.path().join("SB/Binaries/Win64")).unwrap();
    let overrides = STELLAR_BLADE.wine_dll_overrides(td.path());
    assert!(overrides.is_empty());
}

#[test]
fn test_wine_overrides_detects_dwmapi() {
    let td = TempDir::new().unwrap();
    let exe_dir = td.path().join("SB/Binaries/Win64");
    std::fs::create_dir_all(&exe_dir).unwrap();
    std::fs::write(exe_dir.join("dwmapi.dll"), b"stub").unwrap();

    let overrides = STELLAR_BLADE.wine_dll_overrides(td.path());
    assert_eq!(overrides.as_slice(), &["dwmapi".to_string()]);
}

#[test]
fn test_wine_overrides_detects_multiple() {
    let td = TempDir::new().unwrap();
    let exe_dir = td.path().join("SB/Binaries/Win64");
    std::fs::create_dir_all(&exe_dir).unwrap();
    std::fs::write(exe_dir.join("dwmapi.dll"), b"stub").unwrap();
    std::fs::write(exe_dir.join("dxgi.dll"), b"stub").unwrap();

    let overrides = STELLAR_BLADE.wine_dll_overrides(td.path());
    assert!(overrides.iter().any(|s| s == "dwmapi"));
    assert!(overrides.iter().any(|s| s == "dxgi"));
    assert_eq!(overrides.len(), 2);
}

// ── Wine DLL overrides (staging dir) ────────────────────────────────

#[test]
fn test_wine_overrides_from_staging_detects_ue4ss() {
    let td = TempDir::new().unwrap();
    // staging/<some-mod>/dwmapi.dll
    let mod_dir = td.path().join("ue4ss-base");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(mod_dir.join("dwmapi.dll"), b"stub").unwrap();

    let overrides = STELLAR_BLADE.wine_dll_overrides_from_staging(td.path());
    assert_eq!(overrides.as_slice(), &["dwmapi".to_string()]);
}

// ── Ue4Scanner: pak triple grouping ─────────────────────────────────

fn write_empty(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, b"").unwrap();
}

#[test]
fn test_scanner_groups_pak_triple_by_stem() {
    let td = TempDir::new().unwrap();
    let mods_dir = td.path().join("SB/Content/Paks/~mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    write_empty(&mods_dir.join("MyMod_P.pak"));
    write_empty(&mods_dir.join("MyMod_P.ucas"));
    write_empty(&mods_dir.join("MyMod_P.utoc"));

    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = STELLAR_BLADE_SCANNER.scan_filesystem(&ctx).unwrap();

    assert_eq!(mods.len(), 1, "expected single mod from pak triple");
    let m = &mods[0];
    assert_eq!(m.mod_id, "pak/MyMod_P");
    assert_eq!(m.display_name, "MyMod_P");
    assert_eq!(m.files.len(), 3);
    match &m.source {
        ModSource::Filesystem { location } => assert_eq!(location, "paks-mods"),
        ModSource::Archive { .. } => panic!("expected filesystem source"),
    }
}

#[test]
fn test_scanner_separates_distinct_stems() {
    let td = TempDir::new().unwrap();
    let mods_dir = td.path().join("SB/Content/Paks/~mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    write_empty(&mods_dir.join("Alpha_P.pak"));
    write_empty(&mods_dir.join("Beta_P.pak"));
    write_empty(&mods_dir.join("Beta_P.ucas"));

    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = STELLAR_BLADE_SCANNER.scan_filesystem(&ctx).unwrap();

    assert_eq!(mods.len(), 2);
    let ids: Vec<&str> = mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert!(ids.contains(&"pak/Alpha_P"));
    assert!(ids.contains(&"pak/Beta_P"));
}

#[test]
fn test_scanner_logic_mods_location() {
    let td = TempDir::new().unwrap();
    let logic = td.path().join("SB/Content/Paks/LogicMods");
    std::fs::create_dir_all(&logic).unwrap();
    write_empty(&logic.join("Blueprint_P.pak"));

    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = STELLAR_BLADE_SCANNER.scan_filesystem(&ctx).unwrap();

    assert_eq!(mods.len(), 1);
    match &mods[0].source {
        ModSource::Filesystem { location } => assert_eq!(location, "logic-mods"),
        ModSource::Archive { .. } => panic!("expected filesystem source"),
    }
}

#[test]
fn test_scanner_walks_mod_subdirs() {
    // Some mods ship as a subdir containing the pak triple.
    let td = TempDir::new().unwrap();
    let nested = td.path().join("SB/Content/Paks/~mods/BundledMod");
    std::fs::create_dir_all(&nested).unwrap();
    write_empty(&nested.join("Nested_P.pak"));
    write_empty(&nested.join("Nested_P.ucas"));

    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = STELLAR_BLADE_SCANNER.scan_filesystem(&ctx).unwrap();

    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].mod_id, "pak/Nested_P");
    assert_eq!(mods[0].files.len(), 2);
}

#[test]
fn test_scanner_ignores_non_pak_files() {
    let td = TempDir::new().unwrap();
    let mods_dir = td.path().join("SB/Content/Paks/~mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    write_empty(&mods_dir.join("readme.txt"));
    write_empty(&mods_dir.join("Mod_P.pak"));

    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = STELLAR_BLADE_SCANNER.scan_filesystem(&ctx).unwrap();

    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].mod_id, "pak/Mod_P");
}

#[test]
fn test_scanner_empty_when_no_install() {
    let td = TempDir::new().unwrap();
    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = STELLAR_BLADE_SCANNER.scan_filesystem(&ctx).unwrap();
    assert!(mods.is_empty());
}

#[test]
fn test_scanner_scan_directories() {
    let dirs = STELLAR_BLADE_SCANNER.scan_directories();
    assert!(dirs.contains(&"Content/Paks/~mods"));
    assert!(dirs.contains(&"Content/Paks/LogicMods"));
}

// ── Ue4Scanner: mod_id_footprint inverse ────────────────────────────

#[test]
fn test_scanner_footprint_round_trip() {
    use modde_core::scanner::ModFootprint;
    let fp = STELLAR_BLADE_SCANNER
        .mod_id_footprint("pak/MyMod_P")
        .unwrap();
    assert_eq!(
        fp,
        ModFootprint::File("sb/content/paks/~mods/mymod_p.pak".to_string())
    );
}

#[test]
fn test_scanner_footprint_rejects_unknown_prefix() {
    assert!(STELLAR_BLADE_SCANNER.mod_id_footprint("cet/Foo").is_none());
    assert!(STELLAR_BLADE_SCANNER.mod_id_footprint("bare").is_none());
}

// ── Registration wiring ─────────────────────────────────────────────

#[test]
fn test_stellar_blade_in_supported_ids() {
    assert!(SUPPORTED_GAME_IDS.contains(&"stellar-blade"));
}

#[test]
fn test_resolve_game_plugin_stellar_blade() {
    let plugin =
        modde_games::resolve_game_plugin("stellar-blade").expect("stellar-blade should resolve");
    assert_eq!(plugin.game_id(), "stellar-blade");
}

#[test]
fn test_resolve_mod_scanner_stellar_blade() {
    assert!(modde_games::resolve_mod_scanner("stellar-blade").is_some());
}

#[test]
fn test_resolve_save_tracker_stellar_blade() {
    assert!(modde_games::resolve_save_tracker("stellar-blade").is_some());
}

#[test]
fn test_resolve_collision_classifier_stellar_blade() {
    let classifier = modde_games::resolve_collision_classifier("stellar-blade")
        .expect("stellar-blade should expose collision classification");
    assert_eq!(
        classifier.classify_severity("SB/Content/Paks/~mods/Foo_P.pak"),
        CollisionSeverity::Dangerous
    );
    assert_eq!(
        classifier.classify_severity("SB/Content/Paks/~mods/settings.ini"),
        CollisionSeverity::Config
    );
    assert_eq!(
        classifier.classify_severity("SB/Content/Paks/~mods/icon.dds"),
        CollisionSeverity::Cosmetic
    );
    assert_eq!(classifier.archive_extensions(), &["pak", "ucas", "utoc"]);
}

// ── Ue4Scanner is reusable for a future UE4 game ────────────────────

#[test]
fn test_ue4_scanner_is_data_driven() {
    // Smoke-test that a second UE4 title can be wired up with a different
    // project_name. Scan a fake `Palworld/Content/Paks/~mods/` layout.
    static FAKE_PALWORLD_SCANNER: Ue4Scanner = Ue4Scanner {
        game_id: "palworld",
        project_name: "Pal",
    };
    let td = TempDir::new().unwrap();
    let mods_dir = td.path().join("Pal/Content/Paks/~mods");
    std::fs::create_dir_all(&mods_dir).unwrap();
    write_empty(&mods_dir.join("Creature_P.pak"));

    let ctx = ScanContext {
        install_dir: td.path(),
    };
    let mods = FAKE_PALWORLD_SCANNER.scan_filesystem(&ctx).unwrap();
    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].mod_id, "pak/Creature_P");
}
