#![cfg(target_os = "linux")]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use modde_core::library::LaunchSettings;
use modde_games::library::{LibraryGame, Store, launch};

fn program(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn sandbox_grants_runtime_trees_but_not_arbitrary_launcher_parent_directories() {
    let root = tempfile::tempdir().unwrap();
    modde_core::paths::set_data_dir(root.path().join("data"));
    modde_core::paths::set_config_dir(root.path().join("config"));
    let home = root.path().join("private-home");
    let wrapper = home.join("wrapper");
    let runner = root.path().join("wine-runtime/bin/wine");
    let link = home.join("wine");
    program(&wrapper);
    program(&runner);
    std::os::unix::fs::symlink(&runner, &link).unwrap();
    let install = root.path().join("game");
    std::fs::create_dir(&install).unwrap();
    let game = LibraryGame::new(Store::Local, "custom".into(), "Custom".into(), Some(install.clone()));
    let mut settings = LaunchSettings::default();
    settings.sandbox.enabled = true;
    settings.environment.insert("HOME".into(), home.to_string_lossy().into_owned());
    settings.runner = Some(link.clone());
    settings.executable = Some(install.join("Game.exe"));
    settings.wrappers = vec![vec![wrapper.to_string_lossy().into_owned()]];

    // Construct only; no bubblewrap or fixture executable is run here.
    let command = launch::direct_command(&game, &settings).unwrap();
    let args: Vec<_> = command.get_args().map(PathBuf::from).collect();
    let readonly: Vec<_> = args.windows(3).filter(|chunk| chunk[0] == Path::new("--ro-bind"))
        .map(|chunk| chunk[1].clone()).collect();
    assert!(readonly.contains(&wrapper.canonicalize().unwrap()));
    assert!(readonly.contains(&runner.canonicalize().unwrap()));
    assert!(readonly.contains(&root.path().join("wine-runtime")));
    assert!(!readonly.contains(&home));
    assert!(!readonly.contains(&root.path().to_path_buf()));

    // A store may put its own wrappers before Wine. The Wine argv entry still
    // needs its runtime, even though it is not argv[0] of the store boundary.
    settings.runner = None;
    settings.executable = None;
    settings.wrappers.clear();
    let supplied = vec![wrapper.into_os_string(), link.into_os_string(), install.join("Game.exe").into_os_string()];
    let command = launch::boundary_command(&game, &settings, &supplied).unwrap();
    let args: Vec<_> = command.get_args().map(PathBuf::from).collect();
    assert!(args.windows(3).any(|chunk| chunk[0] == Path::new("--ro-bind") && chunk[1] == root.path().join("wine-runtime")));
    assert!(args.ends_with(&[runner, install.join("Game.exe")]));

    // UMU's container root and physical pfx stay distinct in a sandbox too.
    // The container is writable for prefix locks, shader caches and metadata.
    let compat = root.path().join("compat");
    let prefix = compat.join("pfx");
    std::fs::create_dir_all(prefix.join("drive_c")).unwrap();
    settings.prefix = Some(prefix.clone());
    settings.environment.insert("STEAM_COMPAT_DATA_PATH".into(), compat.to_string_lossy().into_owned());
    settings.environment.insert("WINEPREFIX".into(), compat.to_string_lossy().into_owned());
    let command = launch::boundary_command(&game, &settings, &supplied).unwrap();
    let args: Vec<_> = command.get_args().map(PathBuf::from).collect();
    assert!(args.windows(3).any(|chunk| chunk[0] == Path::new("--bind") && chunk[1] == compat));
    assert!(args.windows(3).any(|chunk| chunk[0] == Path::new("--bind") && chunk[1] == prefix));
    let wine_environment: Vec<_> = args.windows(3).filter(|chunk|
        chunk[0] == Path::new("--setenv") && chunk[1] == Path::new("WINEPREFIX"))
        .map(|chunk| chunk[2].clone()).collect();
    assert!(!wine_environment.is_empty());
    assert!(wine_environment.iter().all(|path| *path == compat));

    // Heroic's downloaded UMU launcher may load modules beside its entry point.
    // A custom-named symlink must still grant that dedicated runtime tree.
    let umu = root.path().join("umu-runtime/umu-run");
    program(&umu);
    let alias = home.join("my-runner");
    std::os::unix::fs::symlink(&umu, &alias).unwrap();
    settings.runner = Some(alias.clone());
    settings.executable = Some(install.join("Game.exe"));
    let command = launch::direct_command(&game, &settings).unwrap();
    let args: Vec<_> = command.get_args().map(PathBuf::from).collect();
    assert!(args.windows(3).any(|chunk| chunk[0] == Path::new("--ro-bind") && chunk[1] == root.path().join("umu-runtime")));
    assert!(!args.windows(3).any(|chunk| chunk[0] == Path::new("--ro-bind") && chunk[1] == home));
    assert!(args.ends_with(&[umu.clone(), install.join("Game.exe")]));
    settings.runner = None;
    settings.executable = None;
    let supplied = vec![home.join("wrapper").into_os_string(), alias.into_os_string(), install.join("Game.exe").into_os_string()];
    let command = launch::boundary_command(&game, &settings, &supplied).unwrap();
    let args: Vec<_> = command.get_args().map(PathBuf::from).collect();
    assert!(args.windows(3).any(|chunk| chunk[0] == Path::new("--ro-bind") && chunk[1] == root.path().join("umu-runtime")));
    assert!(args.ends_with(&[umu, install.join("Game.exe")]));
}
