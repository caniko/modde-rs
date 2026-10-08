#![cfg(all(unix, feature = "cyberpunk"))]

mod common;

use common::Fixture;
use modde_core::library::LaunchSettings;
use modde_core::settings::AppSettings;
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_retargeted_alias_can_be_rebound_without_transferring_launch_settings() {
    let fixture = Fixture::new();
    let a = fixture.root().join("a");
    let b = fixture.root().join("b");
    let alias = fixture.root().join("current");
    for path in [&a, &b] {
        std::fs::create_dir(path).unwrap();
    }
    std::os::unix::fs::symlink(&a, &alias).unwrap();
    let config = fixture.home().join(".config/modde");
    std::fs::create_dir_all(&config).unwrap();
    let mut app = AppSettings::default();
    app.set_game_path(&modde_core::GameId::from("cyberpunk2077"), alias.clone());
    std::fs::write(config.join("settings.toml"), toml::to_string(&app).unwrap()).unwrap();
    let output = fixture
        .cmd()
        .args(["library", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let catalogue: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let old = catalogue["games"][0]["id"].as_str().unwrap();
    let preferences_path = fixture.data_dir().join("library.json");
    let mut preferences =
        modde_core::library::LibraryPreferences::load_at(&preferences_path).unwrap();
    preferences.launches.insert(
        old.into(),
        LaunchSettings {
            profile: Some("original".into()),
            ..Default::default()
        },
    );
    modde_core::library::atomic_json(&preferences_path, &preferences).unwrap();
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(&b, &alias).unwrap();
    fixture
        .cmd()
        .args(["library", "list", "--json"])
        .assert()
        .failure();
    let before = std::fs::read(&preferences_path).unwrap();
    fixture
        .cmd()
        .args(["library", "rebind", old, "--alias"])
        .arg(&alias)
        .arg("--target")
        .arg(&a)
        .assert()
        .failure();
    assert_eq!(std::fs::read(&preferences_path).unwrap(), before);
    fixture
        .cmd()
        .args(["library", "rebind", old, "--alias"])
        .arg(&alias)
        .arg("--target")
        .arg(&b)
        .assert()
        .success();
    let output = fixture
        .cmd()
        .args(["library", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let catalogue: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let new = catalogue["games"][0]["id"].as_str().unwrap();
    assert_ne!(new, old);
    let preferences = modde_core::library::LibraryPreferences::load_at(&preferences_path).unwrap();
    assert_eq!(
        preferences.launch_for(old).profile.as_deref(),
        Some("original")
    );
    assert_eq!(preferences.launch_for(new), LaunchSettings::default());
    assert_eq!(catalogue["games"][0]["install_path"], b.to_str().unwrap());
}

#[test]
fn a_disappeared_wrapper_does_not_switch_live_saves_or_the_active_profile() {
    let fixture = Fixture::new();
    let install = fixture.root().join("game");
    let saves = fixture.root().join("saves");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("slot.sav"), "original save").unwrap();
    let executable = install.join("game-executable");
    let wrapper = install.join("wrapper");
    for program in [&executable, &wrapper] {
        std::fs::write(program, "not executed by this test").unwrap();
        std::fs::set_permissions(program, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let config = fixture.home().join(".config/modde");
    std::fs::create_dir_all(&config).unwrap();
    let mut app = AppSettings::default();
    app.set_game_path(&modde_core::GameId::from("cyberpunk2077"), install);
    std::fs::write(config.join("settings.toml"), toml::to_string(&app).unwrap()).unwrap();
    for name in ["original", "target"] {
        fixture
            .cmd()
            .args(["profile", "create", name, "--game", "cyberpunk2077"])
            .assert()
            .success();
    }
    let output = fixture
        .cmd()
        .args(["library", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let catalogue: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let id = catalogue["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["game_id"] == "cyberpunk2077")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let path = fixture.root().join("launch.json");
    let mut launch = LaunchSettings {
        executable: Some(executable),
        profile: Some("original".into()),
        save_directory: Some(saves.clone()),
        ..Default::default()
    };
    std::fs::write(&path, serde_json::to_vec(&launch).unwrap()).unwrap();
    fixture
        .cmd()
        .args(["library", "configure", &id, "--file"])
        .arg(&path)
        .assert()
        .success();
    fixture
        .cmd()
        .args(["library", "adopt", &id, "--profile", "original"])
        .assert()
        .success();
    launch.profile = Some("target".into());
    launch.wrappers = vec![vec![wrapper.to_string_lossy().into_owned()]];
    std::fs::write(&path, serde_json::to_vec(&launch).unwrap()).unwrap();
    fixture
        .cmd()
        .args(["library", "configure", &id, "--file"])
        .arg(&path)
        .assert()
        .success();
    std::fs::remove_file(&wrapper).unwrap();

    fixture
        .cmd()
        .args(["library", "play", &id])
        .assert()
        .failure();
    assert_eq!(
        std::fs::read_to_string(saves.join("slot.sav")).unwrap(),
        "original save"
    );
    let active = fixture
        .cmd()
        .args(["profile", "active", "--game", "cyberpunk2077"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    assert!(String::from_utf8_lossy(&active).contains("Active profile: original"));
    assert!(!config.join("sessions/pending-session.json").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn sandbox_launch_uses_the_executable_selected_by_the_new_deployment() {
    let fixture = Fixture::new();
    let install = fixture.root().join("game");
    let saves = fixture.root().join("saves");
    let bin = fixture.root().join("bin");
    for path in [install.join("mods"), saves.clone(), bin.clone()] {
        std::fs::create_dir_all(path).unwrap();
    }
    let old = fixture.root().join("old-game");
    std::fs::write(&old, "#!/bin/sh\nprintf old > \"$1\"\n").unwrap();
    std::fs::set_permissions(&old, std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = install.join("mods/game");
    std::os::unix::fs::symlink(&old, &executable).unwrap();
    // This fixture tests command selection, not containment. It executes only
    // the command after bwrap's delimiter and never contacts the host manager.
    for (name, script) in [
        (
            "bwrap",
            "#!/bin/sh\nwhile [ \"$#\" -gt 0 ] && [ \"$1\" != -- ]; do shift; done\n[ \"$#\" -gt 0 ] || exit 2\nshift\nexec \"$@\"\n",
        ),
        ("systemctl", "#!/bin/sh\nexit 1\n"),
    ] {
        let path = bin.join(name);
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut search = vec![bin];
    search.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let search = std::env::join_paths(search).unwrap();
    let config = fixture.home().join(".config/modde");
    let mut app = AppSettings::default();
    app.set_game_path(&modde_core::GameId::from("cyberpunk2077"), install);
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("settings.toml"), toml::to_string(&app).unwrap()).unwrap();
    fixture
        .cmd()
        .args([
            "profile",
            "create",
            "replacement",
            "--game",
            "cyberpunk2077",
        ])
        .assert()
        .success();
    let replacement = fixture
        .data_dir()
        .join("profiles/replacement/overrides/game");
    std::fs::create_dir_all(replacement.parent().unwrap()).unwrap();
    std::fs::write(&replacement, "#!/bin/sh\nprintf new > \"$1\"\n").unwrap();
    std::fs::set_permissions(&replacement, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = fixture
        .cmd()
        .args(["library", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let catalogue: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let id = catalogue["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["game_id"] == "cyberpunk2077")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let marker = fixture.root().join("played-version");
    let mut settings = LaunchSettings {
        executable: Some(executable),
        profile: Some("replacement".into()),
        arguments: vec![marker.to_string_lossy().into_owned()],
        save_directory: Some(saves),
        ..Default::default()
    };
    settings.sandbox.enabled = true;
    let path = fixture.root().join("launch.json");
    modde_core::library::atomic_json(&path, &settings).unwrap();
    fixture
        .cmd()
        .args(["library", "configure", id, "--file"])
        .arg(path)
        .assert()
        .success();

    fixture
        .cmd()
        .env("PATH", search)
        .args(["library", "play", id])
        .assert()
        .success();

    assert_eq!(std::fs::read_to_string(marker).unwrap(), "new");
    assert_eq!(
        std::fs::read_to_string(old).unwrap(),
        "#!/bin/sh\nprintf old > \"$1\"\n"
    );
    assert!(!config.join("sessions/pending-session.json").exists());
}

#[test]
fn pending_sessions_block_other_data_directories_and_require_the_owner_for_recovery() {
    let fixture = Fixture::new();
    let sessions = fixture.home().join(".config/modde/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join("pending-session.json"),
        serde_json::to_vec(&serde_json::json!({
            "installation": "install-test", "name": "Example", "game_id": null,
            "scope": "install-test", "profile": null, "save_directory": null,
            "capture": false, "phase": "preparing", "data_directory": fixture.data_dir()
        }))
        .unwrap(),
    )
    .unwrap();
    let other = fixture.root().join("other-data");
    let blocked = fixture
        .cmd()
        .env("MODDE_DATA_DIR", &other)
        .args(["profile", "create", "blocked", "--game", "cyberpunk2077"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&blocked).contains("unfinished"));
    let wrong_owner = fixture
        .cmd()
        .env("MODDE_DATA_DIR", &other)
        .args(["library", "recover"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&wrong_owner).contains("session belongs to"));
    assert!(sessions.join("pending-session.json").exists());
    fixture
        .cmd()
        .args(["library", "recover"])
        .assert()
        .success();
    assert!(!sessions.join("pending-session.json").exists());
}

#[test]
fn heroic_cloud_sync_is_rejected_before_store_dispatch_or_save_changes() {
    let fixture = Fixture::new();
    let install = fixture.root().join("game");
    let saves = fixture.root().join("saves");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::create_dir_all(&saves).unwrap();
    std::fs::write(saves.join("slot.sav"), "local save").unwrap();
    let heroic = fixture.home().join(".config/heroic");
    modde_core::library::atomic_json(&heroic.join("gog_store/installed.json"), &serde_json::json!({
        "installed": [{"appName": "1423049311", "title": "Cyberpunk 2077", "install_path": install}]
    })).unwrap();
    modde_core::library::atomic_json(
        &heroic.join("GamesConfig/1423049311.json"),
        &serde_json::json!({
            "1423049311": {"autoSyncSaves": true}
        }),
    )
    .unwrap();
    fixture
        .cmd()
        .args(["profile", "create", "managed", "--game", "cyberpunk2077"])
        .assert()
        .success();
    let output = fixture
        .cmd()
        .args(["library", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let catalogue: serde_json::Value = serde_json::from_slice(&output).unwrap();
    let id = catalogue["games"]
        .as_array()
        .unwrap()
        .iter()
        .find(|game| game["store"] == "gog")
        .unwrap()["id"]
        .as_str()
        .unwrap();
    let launch = LaunchSettings {
        store_hook: true,
        profile: Some("managed".into()),
        save_directory: Some(saves.clone()),
        ..Default::default()
    };
    let config = fixture.root().join("launch.json");
    modde_core::library::atomic_json(&config, &launch).unwrap();
    fixture
        .cmd()
        .args(["library", "configure", id, "--file"])
        .arg(config)
        .assert()
        .success();
    let error = fixture
        .cmd()
        .args(["library", "play", id])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(String::from_utf8_lossy(&error).contains("disable automatic cloud-save sync"));
    assert_eq!(
        std::fs::read_to_string(saves.join("slot.sav")).unwrap(),
        "local save"
    );
    assert!(
        !fixture
            .home()
            .join(".config/modde/sessions/pending-session.json")
            .exists()
    );

    // Even with cloud sync disabled, a hook copied to another game or store
    // must fail before its command or this installation's preparation can run.
    modde_core::library::atomic_json(
        &heroic.join("GamesConfig/1423049311.json"),
        &serde_json::json!({
            "1423049311": {"autoSyncSaves": false}
        }),
    )
    .unwrap();
    for (key, value, message) in [
        ("HEROIC_APP_NAME", "another-game", "different game"),
        ("HEROIC_APP_RUNNER", "legendary", "different store"),
    ] {
        let error = fixture
            .cmd()
            .env(key, value)
            .args(["library", "wrap", id, "--"])
            .arg(fixture.root().join("never-executed"))
            .assert()
            .failure()
            .get_output()
            .stderr
            .clone();
        assert!(String::from_utf8_lossy(&error).contains(message));
        assert!(
            !fixture
                .home()
                .join(".config/modde/sessions/pending-session.json")
                .exists()
        );
        assert_eq!(
            std::fs::read_to_string(saves.join("slot.sav")).unwrap(),
            "local save"
        );
    }
}
