//! Heroic config mutation and launcher detection, against temp files only.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use modde_games::launcher::{
    Launcher, WrapperRegistrationReport, apply_tool_environment_heroic, detect_launcher,
    register_heroic_wrapper,
};
use modde_games::tools::WrapperEntry;
use serde_json::{Value, json};

/// `register_heroic_wrapper` reads `modde_data_dir()`; keep it off the real one.
fn isolated_data_dir() {
    static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
    DIR.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        modde_core::paths::set_data_dir(dir.path().to_path_buf());
        dir
    });
}

fn heroic_config(dir: &Path, game: Value) -> Launcher {
    let config_path = dir.join("game.json");
    std::fs::write(&config_path, json!({ "game": game }).to_string()).unwrap();
    Launcher::Heroic {
        config_path,
        game_id: "game".to_string(),
    }
}

fn game_config(launcher: &Launcher) -> Value {
    let Launcher::Heroic { config_path, .. } = launcher else {
        unreachable!()
    };
    let config: Value = serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    config["game"].clone()
}

fn wrapper_exes(launcher: &Launcher) -> Vec<String> {
    game_config(launcher)["wrapperOptions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|w| w["exe"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn heroic_wrapper_goes_after_fgmod_once() {
    isolated_data_dir();
    let tmp = tempfile::tempdir().unwrap();
    let launcher = heroic_config(
        tmp.path(),
        json!({ "wrapperOptions": [
            { "exe": "/opt/fgmod/fgmod", "args": "" },
            { "exe": "gamemoderun", "args": "" }
        ]}),
    );
    let wrapper = PathBuf::from("/tmp/modde-wrapper.sh");

    assert!(matches!(
        register_heroic_wrapper(&launcher, &wrapper).unwrap(),
        Some(WrapperRegistrationReport::HeroicRegistered)
    ));
    assert!(register_heroic_wrapper(&launcher, &wrapper).unwrap().is_none());
    assert_eq!(
        wrapper_exes(&launcher),
        ["/opt/fgmod/fgmod", "/tmp/modde-wrapper.sh", "gamemoderun"]
    );
}

#[test]
fn heroic_wrapper_appends_without_fgmod_and_skips_library_hooks() {
    isolated_data_dir();
    let tmp = tempfile::tempdir().unwrap();
    let launcher = heroic_config(
        tmp.path(),
        json!({ "wrapperOptions": [{ "exe": "gamemoderun", "args": "" }] }),
    );
    register_heroic_wrapper(&launcher, Path::new("/w.sh")).unwrap();
    assert_eq!(wrapper_exes(&launcher), ["gamemoderun", "/w.sh"]);

    let hook = modde_core::paths::modde_data_dir().join("launch-hooks/game.sh");
    let hooked = heroic_config(
        tmp.path(),
        json!({ "wrapperOptions": [{ "exe": hook.to_string_lossy(), "args": "" }] }),
    );
    assert!(register_heroic_wrapper(&hooked, Path::new("/w.sh")).unwrap().is_none());
    assert_eq!(wrapper_exes(&hooked).len(), 1);
}

#[test]
fn non_heroic_wrapper_registration_is_manual() {
    let report = register_heroic_wrapper(&Launcher::Unknown, Path::new("/w.sh")).unwrap();
    assert!(matches!(
        report,
        Some(WrapperRegistrationReport::ManualInstruction { wrapper_path }) if wrapper_path == Path::new("/w.sh")
    ));
}

#[test]
fn heroic_tool_environment_replaces_keys_and_dedupes_wrappers() {
    let tmp = tempfile::tempdir().unwrap();
    let launcher = heroic_config(
        tmp.path(),
        json!({
            "enviromentOptions": [{ "key": "DXVK_HUD", "value": "0" }, { "key": "KEEP", "value": "1" }],
            "wrapperOptions": [{ "exe": "mangohud", "args": "" }]
        }),
    );
    let Launcher::Heroic { config_path, .. } = &launcher else {
        unreachable!()
    };
    let wrappers = [
        WrapperEntry {
            exe: "mangohud".into(),
            args: String::new(),
        },
        WrapperEntry {
            exe: "gamemoderun".into(),
            args: "--".into(),
        },
    ];
    let report = apply_tool_environment_heroic(
        config_path,
        "game",
        &[("DXVK_HUD".into(), "fps".into())],
        &wrappers,
    )
    .unwrap();
    assert_eq!((report.env_var_count, report.wrapper_count), (1, 2));

    let game = game_config(&launcher);
    assert_eq!(
        game["enviromentOptions"],
        json!([{ "key": "KEEP", "value": "1" }, { "key": "DXVK_HUD", "value": "fps" }])
    );
    assert_eq!(wrapper_exes(&launcher), ["mangohud", "gamemoderun"]);
}

#[test]
fn heroic_tool_environment_noop_and_missing_sections() {
    let missing = Path::new("/nonexistent/heroic.json");
    let report = apply_tool_environment_heroic(missing, "game", &[], &[]).unwrap();
    assert_eq!((report.env_var_count, report.wrapper_count), (0, 0));

    let tmp = tempfile::tempdir().unwrap();
    let launcher = heroic_config(tmp.path(), json!({}));
    let Launcher::Heroic { config_path, .. } = &launcher else {
        unreachable!()
    };
    let env = [("A".to_string(), "1".to_string())];
    assert!(apply_tool_environment_heroic(config_path, "game", &env, &[]).is_err());
    assert!(apply_tool_environment_heroic(config_path, "other", &env, &[]).is_err());
}

#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
mod wine {
    use super::*;
    use modde_games::launcher::{WineOverrideReport, apply_wine_overrides};

    fn dlls(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| (*n).to_string()).collect()
    }

    fn wine_value(launcher: &Launcher) -> Value {
        game_config(launcher)["enviromentOptions"].clone()
    }

    #[test]
    fn heroic_overrides_are_added_then_merged() {
        let tmp = tempfile::tempdir().unwrap();
        let launcher = heroic_config(tmp.path(), json!({ "enviromentOptions": [] }));

        apply_wine_overrides(&launcher, &dlls(&["winmm", "dxgi"])).unwrap();
        assert_eq!(
            wine_value(&launcher),
            json!([{ "key": "WINEDLLOVERRIDES", "value": "winmm=n,b" }])
        );

        let launcher = heroic_config(
            tmp.path(),
            json!({ "enviromentOptions": [{ "key": "WINEDLLOVERRIDES", "value": "winmm=n;d3d11=n,b" }] }),
        );
        apply_wine_overrides(&launcher, &dlls(&["winmm", "version"])).unwrap();
        assert_eq!(
            wine_value(&launcher),
            json!([{ "key": "WINEDLLOVERRIDES", "value": "d3d11=n,b;winmm=n,b;version=n,b" }])
        );
    }

    #[test]
    fn heroic_overrides_skip_dxgi_only_and_report_missing_sections() {
        let tmp = tempfile::tempdir().unwrap();
        let launcher = heroic_config(tmp.path(), json!({ "enviromentOptions": [] }));
        assert!(apply_wine_overrides(&launcher, &dlls(&["dxgi"])).unwrap().is_none());
        assert!(apply_wine_overrides(&launcher, &[]).unwrap().is_none());
        assert_eq!(wine_value(&launcher), json!([]));

        let launcher = heroic_config(tmp.path(), json!({}));
        assert!(apply_wine_overrides(&launcher, &dlls(&["winmm"])).is_err());
    }

    #[test]
    fn non_heroic_launchers_get_instructions() {
        let steam = Launcher::Steam {
            app_id: "1".into(),
        };
        assert!(matches!(
            apply_wine_overrides(&steam, &dlls(&["a", "b"])).unwrap(),
            Some(WineOverrideReport::SteamInstruction { override_value }) if override_value == "a=n,b;b=n,b"
        ));
        assert!(matches!(
            apply_wine_overrides(&Launcher::Unknown, &dlls(&["dxgi"])).unwrap(),
            Some(WineOverrideReport::UnknownInstruction { override_value }) if override_value == "dxgi=n,b"
        ));
    }
}

#[test]
fn steam_detection_reads_the_matching_appmanifest() {
    let tmp = tempfile::tempdir().unwrap();
    let steamapps = tmp.path().join("steamapps");
    let game_dir = steamapps.join("common/Cyberpunk 2077");
    std::fs::create_dir_all(&game_dir).unwrap();
    std::fs::write(
        steamapps.join("appmanifest_1091500.acf"),
        "\"installdir\" \"Cyberpunk 2077\"",
    )
    .unwrap();
    std::fs::write(steamapps.join("appmanifest_1.acf"), "\"installdir\" \"Other\"").unwrap();

    assert!(matches!(
        detect_launcher(&game_dir),
        Launcher::Steam { app_id } if app_id == "1091500"
    ));
    assert!(matches!(
        detect_launcher(&tmp.path().join("elsewhere")),
        Launcher::Unknown
    ));
}
