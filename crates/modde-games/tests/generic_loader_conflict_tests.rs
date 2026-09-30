use modde_games::{load_user_games, resolve_game_plugin};
use tracing_test::traced_test;

#[test]
#[traced_test]
fn load_user_games_warns_and_skips_conflicting_or_invalid_specs() {
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let data_dir = tempdir.path().join("data");
    std::fs::create_dir_all(data_dir.join("games")).expect("create games dir");

    std::fs::write(data_dir.join("games/bad.toml"), "not valid toml").expect("write bad spec");
    std::fs::write(
        data_dir.join("games/skyrim-se.toml"),
        r#"
            id = "skyrim-se"
            display_name = "Shadow Skyrim"
            executable_dir = "bin/x64"
        "#,
    )
    .expect("write colliding spec");

    modde_core::paths::set_data_dir(data_dir.clone());

    let games = load_user_games();
    assert!(games.is_empty());
    assert!(logs_contain("bad.toml"));
    assert!(logs_contain("skyrim-se.toml"));

    let plugin = resolve_game_plugin("skyrim-se");
    assert_eq!(plugin.is_some(), cfg!(feature = "bethesda"));
    if let Some(plugin) = plugin {
        assert_eq!(plugin.display_name(), "The Elder Scrolls V: Skyrim Special Edition");
    }
}
