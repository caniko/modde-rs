#![cfg(feature = "bg3")]

use modde_games::GamePlugin;

#[test]
fn bg3_deployment_and_post_hook_use_the_explicit_prefix() {
    let root = tempfile::tempdir().unwrap();
    let install = root.path().join("steamapps/common/BG3");
    let prefix = root.path().join("custom-prefix");
    let staging = root.path().join("staging");
    std::fs::create_dir_all(&staging).unwrap();
    std::fs::write(staging.join("Example.pak"), "fixture").unwrap();
    let game = &modde_games::bg3::BALDURS_GATE3;
    game.deploy_to_install_at(&staging, &install, Some(&prefix))
        .unwrap();
    game.post_deploy_at(&install, Some(&prefix)).unwrap();
    let data = prefix.join("drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3");
    assert!(data.join("Mods/Example.pak").is_file());
    assert!(
        std::fs::read_to_string(data.join("PlayerProfiles/Public/modsettings.lsx"))
            .unwrap()
            .contains("Example")
    );
    assert!(!root.path().join("steamapps/compatdata").exists());
}
