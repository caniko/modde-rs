#![cfg(all(
    feature = "gamebryo",
    feature = "bg3",
    feature = "stardew",
    feature = "bannerlord",
    feature = "witcher3",
    feature = "oblivion-remastered",
    feature = "ue4"
))]

use std::path::Path;

use modde_core::installer::{self, InstallMethod};
use modde_games::traits::{GamePlugin, ModScanner};
use modde_games::{bannerlord, game_probe};
use tempfile::TempDir;

#[test]
fn high_impact_games_are_registered_with_steam_ids() {
    let expected = [
        ("baldurs-gate3", "Baldur's Gate 3", Some("1086940")),
        ("stardew-valley", "Stardew Valley", Some("413150")),
        ("fallout-new-vegas", "Fallout: New Vegas", Some("22380")),
        ("oblivion", "The Elder Scrolls IV: Oblivion", Some("22330")),
        (
            "oblivion-remastered",
            "The Elder Scrolls IV: Oblivion Remastered",
            Some("2623190"),
        ),
        ("bannerlord", "Mount & Blade II: Bannerlord", Some("261550")),
        ("witcher3", "The Witcher 3: Wild Hunt", Some("292030")),
    ];

    for (game_id, display_name, steam_id) in expected {
        let registration = modde_games::resolve_game(game_id).unwrap();
        assert_eq!(registration.display_name, display_name);
        assert_eq!(registration.launcher.steam_app_id, steam_id);
        assert!(registration.scanner.is_some(), "{game_id} scanner");
        assert!(
            registration.save_tracker.is_some(),
            "{game_id} save tracker"
        );
        assert!(
            registration.collision_classifier.is_some(),
            "{game_id} conflicts"
        );
        assert!(registration.supports_save_profiles, "{game_id} saves");

        let plugin = modde_games::resolve_game_plugin(game_id).unwrap();
        assert_eq!(plugin.game_id(), game_id);
        assert_eq!(plugin.display_name(), display_name);
    }
}

#[test]
fn new_game_mod_roots_are_resolved() {
    let install = Path::new("/games/common/Game");

    assert_eq!(
        modde_games::stardew::STARDEW_VALLEY
            .mod_root(install)
            .unwrap(),
        install.join("Mods")
    );
    assert_eq!(
        modde_games::gamebryo::FALLOUT_NEW_VEGAS
            .mod_root(install)
            .unwrap(),
        install.join("Data")
    );
    assert_eq!(
        modde_games::bannerlord::BANNERLORD
            .mod_root(install)
            .unwrap(),
        install.join("Modules")
    );
    assert_eq!(
        modde_games::witcher3::WITCHER3.mod_root(install).unwrap(),
        install.join("mods")
    );
    assert!(
        modde_games::bg3::BALDURS_GATE3
            .mod_root(install)
            .unwrap()
            .ends_with("Larian Studios/Baldur's Gate 3/Mods")
    );
}

#[test]
fn bg3_modsettings_round_trip_preserves_order() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("PlayerProfiles/Public/modsettings.lsx");
    let mods = vec!["A".to_string(), "B".to_string(), "C".to_string()];

    modde_games::bg3::write_modsettings(&path, &mods).unwrap();

    assert_eq!(modde_games::bg3::read_modsettings(&path).unwrap(), mods);
}

#[test]
fn bg3_deploy_updates_modsettings_from_paks() {
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("Baldurs Gate 3");
    let staging = tmp.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("Cool.pak"), "pak").unwrap();

    modde_games::bg3::BALDURS_GATE3
        .deploy_to_install(&staging, &install)
        .unwrap();
    modde_games::bg3::BALDURS_GATE3
        .post_deploy(&install)
        .unwrap();

    let modsettings = modde_games::bg3::modsettings_path_from_install(&install);
    assert_eq!(
        modde_games::bg3::read_modsettings(&modsettings).unwrap(),
        vec!["Cool"]
    );
}

#[test]
fn bg3_hot_deploy_is_experimental() {
    let capability = modde_games::bg3::BALDURS_GATE3.hot_deploy_capability();
    assert_eq!(
        capability.support,
        modde_games::traits::HotDeploySupport::Experimental
    );
    assert!(capability.cosmetic_only);
}

#[test]
fn bg3_hot_deploy_patches_staging_and_refreshes_root_symlink() {
    let tmp = TempDir::new().unwrap();
    let install = tmp.path().join("Baldurs Gate 3");
    let staging = tmp.path().join("staging");
    let source = tmp.path().join("source");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::create_dir_all(&source).unwrap();

    let source_file = source.join("Cool.pak");
    std::fs::write(&source_file, b"pak").unwrap();

    // A stale deployed root that must be replaced by a fresh symlink.
    let mods_dir = modde_games::bg3::mods_dir_from_install(&install);
    std::fs::create_dir_all(&mods_dir).unwrap();
    std::fs::write(mods_dir.join("Cool.pak"), b"stale").unwrap();

    let patch = modde_core::hot_deploy::HotDeployPatch {
        changes: vec![modde_core::hot_deploy::HotDeployChange {
            rel_path: "Cool.pak".to_string(),
            before: None,
            after: Some(source_file),
        }],
    };

    modde_games::bg3::BALDURS_GATE3
        .apply_hot_deploy_patch(&patch, &staging, &install)
        .unwrap();

    let staged = staging.join("Cool.pak");
    assert!(staged.symlink_metadata().unwrap().file_type().is_symlink());

    let deployed_root = mods_dir.join("Cool.pak");
    assert!(
        deployed_root
            .symlink_metadata()
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&deployed_root).unwrap(), b"pak");

    // post_deploy must regenerate modsettings.lsx from the deployed paks.
    let modsettings = modde_games::bg3::modsettings_path_from_install(&install);
    assert_eq!(
        modde_games::bg3::read_modsettings(&modsettings).unwrap(),
        vec!["Cool"]
    );
}

#[test]
fn stardew_scanner_discovers_smapi_mods() {
    let tmp = TempDir::new().unwrap();
    let mod_dir = tmp.path().join("Mods/BetterRanching");
    std::fs::create_dir_all(&mod_dir).unwrap();
    std::fs::write(mod_dir.join("manifest.json"), "{}").unwrap();

    let ctx = modde_games::traits::ScanContext {
        install_dir: tmp.path(),
    };
    let mods = modde_games::stardew::scanner::STARDEW_SCANNER
        .scan_filesystem(&ctx)
        .unwrap();

    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].mod_id, "smapi/BetterRanching");
    assert!((mods[0].confidence - 0.98).abs() < f64::EPSILON);
}

#[test]
fn gamebryo_scanner_discovers_plugin_and_companion_bsa() {
    let tmp = TempDir::new().unwrap();
    let data = tmp.path().join("Data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("SomeMod.esp"), "plugin").unwrap();
    std::fs::write(data.join("SomeMod.bsa"), "archive").unwrap();

    let ctx = modde_games::traits::ScanContext {
        install_dir: tmp.path(),
    };
    let mods = modde_games::gamebryo::scanner::FALLOUT_NEW_VEGAS_SCANNER
        .scan_filesystem(&ctx)
        .unwrap();

    assert_eq!(mods.len(), 1);
    assert_eq!(mods[0].mod_id, "plugin/SomeMod.esp");
    assert_eq!(mods[0].files.len(), 2);
}

#[test]
fn bannerlord_parser_and_scanner_read_submodule_metadata() {
    let tmp = TempDir::new().unwrap();
    let module = tmp.path().join("Modules/MyModule");
    std::fs::create_dir_all(&module).unwrap();
    std::fs::write(
        module.join("SubModule.xml"),
        r#"<Module><Name value="My Module"/><Id value="MyModule"/><DependedModules><DependedModule Id="Native"/></DependedModules></Module>"#,
    )
    .unwrap();

    let info = bannerlord::parse_submodule_xml(&module.join("SubModule.xml")).unwrap();
    assert_eq!(info.id, "MyModule");
    assert_eq!(info.name, "My Module");
    assert_eq!(info.dependencies, vec!["Native"]);
    assert_eq!(
        bannerlord::missing_dependencies(std::slice::from_ref(&info)),
        vec![("MyModule".to_string(), "Native".to_string())]
    );

    let ctx = modde_games::traits::ScanContext {
        install_dir: tmp.path(),
    };
    let mods = modde_games::bannerlord::scanner::BANNERLORD_SCANNER
        .scan_filesystem(&ctx)
        .unwrap();
    assert_eq!(mods[0].mod_id, "module/MyModule");
    assert_eq!(mods[0].display_name, "My Module");
}

#[test]
fn witcher3_scanner_and_script_conflict_detection_work() {
    let tmp = TempDir::new().unwrap();
    let scripts = tmp.path().join("mods/modGameplay/content/scripts/game");
    std::fs::create_dir_all(&scripts).unwrap();
    std::fs::write(scripts.join("player.ws"), "script").unwrap();
    let scripts2 = tmp.path().join("mods/modOther/content/scripts/game");
    std::fs::create_dir_all(&scripts2).unwrap();
    std::fs::write(scripts2.join("player.ws"), "script").unwrap();

    assert!(modde_games::witcher3::has_script_conflict(
        &tmp.path().join("mods/modGameplay")
    ));

    let ctx = modde_games::traits::ScanContext {
        install_dir: tmp.path(),
    };
    let mods = modde_games::witcher3::scanner::WITCHER3_SCANNER
        .scan_filesystem(&ctx)
        .unwrap();
    let ids = mods
        .iter()
        .map(|item| item.mod_id.as_str())
        .collect::<Vec<_>>();
    assert!(ids.contains(&"mod/modGameplay"));
    assert_eq!(
        modde_games::witcher3::script_conflict_paths(&tmp.path().join("mods")),
        vec!["content/scripts/game/player.ws"]
    );
}

#[test]
fn game_specific_analyzers_choose_hardened_install_methods() {
    let tmp = TempDir::new().unwrap();
    let fnv = tmp.path().join("fnv");
    std::fs::create_dir_all(fnv.join("Data")).unwrap();
    std::fs::write(fnv.join("Data/Foo.esp"), "plugin").unwrap();
    let probe = game_probe(&modde_games::gamebryo::FALLOUT_NEW_VEGAS);
    let plan = installer::analyze(&fnv, &probe, "h".into()).unwrap();
    assert!(matches!(
        plan.method,
        InstallMethod::StripContentRoot { ref root } if root == "Data"
    ));

    let stardew = tmp.path().join("stardew");
    std::fs::create_dir_all(&stardew).unwrap();
    std::fs::write(stardew.join("manifest.json"), "{}").unwrap();
    let probe = game_probe(&modde_games::stardew::STARDEW_VALLEY);
    let plan = installer::analyze(&stardew, &probe, "h".into()).unwrap();
    assert!(matches!(plan.method, InstallMethod::DirectoryMod { .. }));

    let bg3 = tmp.path().join("bg3");
    std::fs::create_dir_all(&bg3).unwrap();
    std::fs::write(bg3.join("Cool.pak"), "pak").unwrap();
    let probe = game_probe(&modde_games::bg3::BALDURS_GATE3);
    let plan = installer::analyze(&bg3, &probe, "h".into()).unwrap();
    assert!(matches!(plan.method, InstallMethod::SingleFileSet));
}

#[test]
fn gamebryo_plugin_order_file_round_trips() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("plugins.txt");
    let plugins = vec!["FalloutNV.esm".to_string(), "SomeMod.esp".to_string()];

    modde_games::gamebryo::write_plugin_order_file(&path, &plugins).unwrap();

    assert_eq!(
        modde_games::gamebryo::read_plugin_order_file(&path).unwrap(),
        plugins
    );
}

#[test]
fn oblivion_remastered_scanner_discovers_pak_groups_and_plugins() {
    let tmp = TempDir::new().unwrap();
    let paks = tmp.path().join("OblivionRemastered/Content/Paks/~mods");
    std::fs::create_dir_all(&paks).unwrap();
    std::fs::write(paks.join("Visuals.pak"), "pak").unwrap();
    std::fs::write(paks.join("Visuals.ucas"), "ucas").unwrap();
    let data = tmp.path().join("Data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("Quest.esp"), "plugin").unwrap();

    let ctx = modde_games::traits::ScanContext {
        install_dir: tmp.path(),
    };
    let mods = modde_games::oblivion_remastered::scanner::OBLIVION_REMASTERED_SCANNER
        .scan_filesystem(&ctx)
        .unwrap();
    let ids = mods
        .iter()
        .map(|item| item.mod_id.as_str())
        .collect::<Vec<_>>();

    assert!(ids.contains(&"pak/Visuals"));
    assert!(ids.contains(&"plugin/Quest"));
}

fn touch(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "x").unwrap();
}

#[test]
fn plugin_metadata_describes_each_game() {
    use modde_games::traits::{ContentCategory, ModSafety};

    let install = Path::new("/games/common/Game");
    let prefix = Path::new("/prefix");
    let tmp = TempDir::new().unwrap();
    // (plugin, Steam id, Nexus domain, archive extensions, save path tail, save-breaking file)
    let games: [(&dyn GamePlugin, u32, &str, &[&str], &str, &str); 5] = [
        (
            &modde_games::gamebryo::FALLOUT_NEW_VEGAS,
            22380,
            "newvegas",
            &["bsa"],
            "Documents/My Games/FalloutNV/Saves",
            "Mod.esp",
        ),
        (
            &modde_games::gamebryo::OBLIVION,
            22330,
            "oblivion",
            &["bsa"],
            "Documents/My Games/Oblivion/Saves",
            "Mod.esm",
        ),
        (
            &modde_games::witcher3::WITCHER3,
            292030,
            "witcher3",
            &["bundle", "cache"],
            "Documents/The Witcher 3/gamesaves",
            "content/scripts/a.ws",
        ),
        (
            &modde_games::oblivion_remastered::OBLIVION_REMASTERED,
            2623190,
            "oblivionremastered",
            &["pak", "ucas", "utoc"],
            "Documents/My Games/Oblivion Remastered/Saves",
            "Mod.pak",
        ),
        (
            &modde_games::bg3::BALDURS_GATE3,
            1086940,
            "baldursgate3",
            &["pak"],
            "Baldur's Gate 3/PlayerProfiles/Public/Savegames",
            "Mod.pak",
        ),
    ];

    for (game, steam_id, domain, archives, save_tail, breaking) in games {
        let id = game.game_id();
        assert_eq!(game.steam_app_id_u32(), Some(steam_id), "{id}");
        assert_eq!(game.nexus_game_domain(), Some(domain), "{id}");
        assert_eq!(game.archive_extensions(), archives, "{id}");
        assert!(game.supports_save_profiles(), "{id}");
        assert!(game.save_directory().is_some(), "{id}");
        assert_eq!(game.save_directory_at(install, None), None, "{id}");
        assert!(
            game.save_directory_at(install, Some(prefix))
                .unwrap()
                .ends_with(save_tail),
            "{id}"
        );
        assert_eq!(
            game.classify_extension(archives[0]),
            ContentCategory::Archive,
            "{id}"
        );

        let mod_dir = tmp.path().join(id);
        touch(&mod_dir.join(breaking));
        assert_eq!(game.classify_mod(&mod_dir), ModSafety::SaveBreaking, "{id}");
        assert_eq!(
            game.classify_mod(&tmp.path().join("missing")),
            ModSafety::Unknown
        );
    }

    let fnv = &modde_games::gamebryo::FALLOUT_NEW_VEGAS;
    assert!(fnv.has_plugin_system());
    assert!(fnv.ini_file_names().contains(&"Fallout.ini"));
    assert_eq!(fnv.mod_directory(install), install.join("Data"));
    let or = &modde_games::oblivion_remastered::OBLIVION_REMASTERED;
    assert!(or.has_plugin_system());
    assert_eq!(
        or.mod_root(install).unwrap(),
        install.join("OblivionRemastered/Content/Paks/~mods")
    );
}

#[test]
fn game_plugins_analyze_archives_and_proxy_dlls() {
    let tmp = TempDir::new().unwrap();
    let dir = |name: &str, files: &[&str]| {
        let root = tmp.path().join(name);
        std::fs::create_dir_all(&root).unwrap();
        for file in files {
            touch(&root.join(file));
        }
        root
    };
    let strip = |root: &str| InstallMethod::StripContentRoot {
        root: root.to_string(),
    };
    let or = &modde_games::oblivion_remastered::OBLIVION_REMASTERED;
    let bg3 = &modde_games::bg3::BALDURS_GATE3;
    let fnv = &modde_games::gamebryo::FALLOUT_NEW_VEGAS;
    let witcher = &modde_games::witcher3::WITCHER3;
    let empty = dir("empty", &[]);

    let pak = dir("pak", &["Mod.PAK"]);
    assert_eq!(
        or.analyze_mod_archive(&pak),
        Some(InstallMethod::SingleFileSet)
    );
    assert_eq!(
        bg3.analyze_mod_archive(&pak),
        Some(InstallMethod::SingleFileSet)
    );
    let data = dir("data", &["Data/Mod.esp"]);
    assert_eq!(or.analyze_mod_archive(&data), Some(strip("Data")));
    assert_eq!(fnv.analyze_mod_archive(&data), Some(strip("Data")));
    let mods = dir("mods", &["Mods/Mod.pak"]);
    assert_eq!(bg3.analyze_mod_archive(&mods), Some(strip("Mods")));
    for game in [or as &dyn GamePlugin, bg3, fnv, witcher] {
        assert_eq!(game.analyze_mod_archive(&empty), None, "{}", game.game_id());
    }

    let overlay = dir("overlay", &["mods/modA/a.ws", "dlc/x/b.bundle"]);
    assert_eq!(
        witcher.analyze_mod_archive(&overlay),
        Some(InstallMethod::MultiRootOverlay {
            roots: vec!["mods".to_string(), "dlc".to_string()]
        })
    );
    let wrapped = dir("wrapped", &["modFoo/content/a.ws"]);
    assert_eq!(
        witcher.analyze_mod_archive(&wrapped),
        Some(InstallMethod::DirectoryMod {
            directory_name: Some("modFoo".to_string())
        })
    );
    assert!(witcher.recognizes_bare_layout(&wrapped.join("modFoo")));
    assert!(!witcher.recognizes_bare_layout(&empty));
    assert!(or.recognizes_bare_layout(&dir("bare-or", &["~mods/a.pak"])));
    assert!(fnv.recognizes_bare_layout(&data));
    assert!(bg3.recognizes_bare_layout(&mods));

    let witcher_install = dir("witcher-install", &["bin/x64/dxgi.dll"]);
    assert_eq!(
        witcher.wine_dll_overrides(&witcher_install).as_slice(),
        ["dxgi"]
    );
    let or_install = dir(
        "or-install",
        &["OblivionRemastered/Binaries/Win64/dwmapi.dll"],
    );
    assert_eq!(or.wine_dll_overrides(&or_install).as_slice(), ["dwmapi"]);
    let staging = dir("staging", &["ModA/version.dll"]);
    assert_eq!(
        or.wine_dll_overrides_from_staging(&staging).as_slice(),
        ["version"]
    );
    assert_eq!(
        witcher.wine_dll_overrides_from_staging(&staging).as_slice(),
        ["version"]
    );
}

#[test]
fn witcher3_deploys_rooted_and_plain_staging() {
    let tmp = TempDir::new().unwrap();
    let witcher = &modde_games::witcher3::WITCHER3;

    let rooted = tmp.path().join("rooted");
    touch(&rooted.join("mods/modA/content/a.ws"));
    touch(&rooted.join("bin/config/b.xml"));
    let install = tmp.path().join("install-rooted");
    witcher.deploy_to_install(&rooted, &install).unwrap();
    assert!(install.join("mods/modA/content/a.ws").exists());
    assert!(install.join("bin/config/b.xml").exists());

    let plain = tmp.path().join("plain");
    touch(&plain.join("modB/content/c.ws"));
    let install = tmp.path().join("install-plain");
    witcher.deploy_to_install(&plain, &install).unwrap();
    assert!(install.join("mods/modB/content/c.ws").exists());
}

#[test]
fn bannerlord_plugin_metadata_and_module_analysis() {
    use modde_games::traits::{ContentCategory, ModSafety};

    let game = &bannerlord::BANNERLORD;
    let install = Path::new("/games/common/Bannerlord");
    assert_eq!(game.steam_app_id_u32(), Some(261550));
    assert_eq!(game.nexus_game_domain(), Some("mountandblade2bannerlord"));
    assert!(game.supports_save_profiles());
    assert!(game.save_directory().is_some());
    assert_eq!(game.save_directory_at(install, None), None);
    assert!(
        game.save_directory_at(install, Some(Path::new("/prefix")))
            .unwrap()
            .ends_with("Game Saves/Native")
    );
    assert_eq!(
        game.executable_dir(install),
        install.join("bin/Win64_Shipping_Client")
    );
    assert_eq!(game.classify_extension("dll"), ContentCategory::Binary);

    let tmp = TempDir::new().unwrap();
    let module = tmp.path().join("MyModule");
    touch(&module.join("SubModule.xml"));
    touch(&module.join("bin/Win64_Shipping_Client/My.dll"));
    assert_eq!(game.classify_mod(&module), ModSafety::SaveBreaking);
    assert_eq!(
        game.analyze_mod_archive(&module),
        Some(InstallMethod::DirectoryModFromXml {
            marker: "SubModule.xml".into(),
            id_attr: "Id.value".to_string(),
            fallback_name: None,
        })
    );
    assert!(game.recognizes_bare_layout(&module));
    let wrapped = tmp.path().join("wrapped");
    touch(&wrapped.join("Modules/MyModule/SubModule.xml"));
    assert_eq!(
        game.analyze_mod_archive(&wrapped),
        Some(InstallMethod::StripContentRoot {
            root: "Modules".to_string()
        })
    );
    let empty = tmp.path().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(game.analyze_mod_archive(&empty), None);
    assert!(!game.recognizes_bare_layout(&empty));

    // Without <Id>/<Name> the module is named after its directory.
    let info = bannerlord::parse_submodule_xml(&module.join("SubModule.xml")).unwrap();
    assert_eq!(
        (info.id.as_str(), info.name.as_str()),
        ("MyModule", "MyModule")
    );
    assert!(bannerlord::parse_submodule_xml(&empty.join("SubModule.xml")).is_err());
}
