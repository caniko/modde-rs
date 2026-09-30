use std::sync::OnceLock;

use modde_games::tools::ToolGameContext;
use modde_games::{all_games, resolve_game_plugin, supported_game_ids, supported_games};

fn shared_data_dir() -> &'static std::path::PathBuf {
    static DATA_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        let tempdir = tempfile::TempDir::new().expect("create tempdir");
        let data_dir = tempdir.path().join("data");
        std::fs::create_dir_all(data_dir.join("games")).expect("create games dir");

        std::fs::write(
            data_dir.join("games/custom-generic.toml"),
            r#"
                id = "custom-generic"
                display_name = "Custom Generic"
                steam_app_id = "1234567"
                install_dir_name = "Custom Generic"
                executable_dir = "bin/x64"
                mod_dir = "mods"
                nexus_domain = "customgeneric"
                proxy_dlls = ["dxgi"]
            "#,
        )
        .expect("write custom game spec");

        modde_core::paths::set_data_dir(data_dir.clone());

        std::mem::forget(tempdir);
        data_dir
    })
}

#[test]
fn user_game_registered_alongside_built_ins() {
    let _ = shared_data_dir();

    let games = all_games();
    assert!(games.iter().any(|game| game.game_id == "custom-generic"));
    assert_eq!(games.iter().any(|game| game.game_id == "skyrim-se"), cfg!(feature = "bethesda"));

    let supported = supported_games();
    assert!(
        supported
            .iter()
            .any(|(game_id, display_name)| *game_id == "custom-generic"
                && *display_name == "Custom Generic")
    );
    assert!(supported_game_ids().contains(&"custom-generic"));
}

#[test]
fn user_game_resolves_executable_dir_for_tools() {
    let _ = shared_data_dir();

    let plugin = resolve_game_plugin("custom-generic").expect("custom game should resolve");
    assert_eq!(plugin.display_name(), "Custom Generic");

    let install_root = std::path::Path::new("/games/custom-generic");
    assert_eq!(
        plugin.executable_dir(install_root),
        install_root.join("bin/x64")
    );

    let expected_executable_dir = install_root.join("bin/x64");
    let context = ToolGameContext::from_parts(
        "custom-generic",
        plugin.display_name(),
        Some(install_root.to_path_buf()),
        None,
    );
    assert_eq!(
        context.executable_dir.as_deref(),
        Some(expected_executable_dir.as_path())
    );
}

#[test]
fn all_games_is_cached() {
    let _ = shared_data_dir();

    let first = all_games();
    let second = all_games();
    assert!(std::ptr::eq(first, second));
}
