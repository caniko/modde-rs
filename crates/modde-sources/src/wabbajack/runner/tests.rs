use super::*;
use crate::wabbajack::staging::{StagingCompressionPolicy, StagingStore, compressed_path};

#[tokio::test]
async fn deploy_decodes_compressed_files_and_links_plain_files() {
    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("staging");
    let game = temp.path().join("game");
    let store = StagingStore::with_policy(
        &staging,
        StagingCompressionPolicy {
            min_bytes: 1,
            level: 1,
            suffix: ".modde-zst".to_string(),
        },
    );
    store.prepare_fresh().await.unwrap();

    let compressed_rel = "mods/TextureMod/textures/landscape/snow.dds";
    let plain_rel = "mods/PluginMod/plugin.esp";
    let compressed_path_plain = staging.join(compressed_rel);
    let plain_path = staging.join(plain_rel);
    tokio::fs::create_dir_all(compressed_path_plain.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::create_dir_all(plain_path.parent().unwrap())
        .await
        .unwrap();
    let compressed_bytes = vec![3_u8; 128 * 1024];
    tokio::fs::write(&compressed_path_plain, &compressed_bytes)
        .await
        .unwrap();
    tokio::fs::write(&plain_path, b"plugin").await.unwrap();
    store.compress_eligible_files(1).await.unwrap();

    assert!(compressed_path(&compressed_path_plain).exists());
    assert!(!compressed_path_plain.exists());

    deploy_mo2_to_game(&staging, &game, false).await.unwrap();

    assert_eq!(
        tokio::fs::read(game.join("textures/landscape/snow.dds"))
            .await
            .unwrap(),
        compressed_bytes
    );
    assert_eq!(
        tokio::fs::read(game.join("plugin.esp")).await.unwrap(),
        b"plugin"
    );
}

#[tokio::test]
async fn deploy_honours_mo2_priority_and_enabled_flags() {
    let temp = tempfile::tempdir().unwrap();
    let staging = temp.path().join("staging");
    let game = temp.path().join("game");
    for (name, body) in [
        ("Base", "base"),
        ("Expansion", "expansion"),
        ("Disabled", "disabled"),
    ] {
        let dir = staging.join("mods").join(name).join("r6/scripts");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Shared.reds"), body).unwrap();
    }
    std::fs::write(staging.join("mods/Disabled/r6/scripts/only.reds"), "x").unwrap();
    let profile = staging.join("profiles/List");
    std::fs::create_dir_all(&profile).unwrap();
    // MO2 writes the highest-priority mod first, with CRLF line endings.
    std::fs::write(
        profile.join("modlist.txt"),
        "# generated\r\n+Expansion\r\n-Disabled\r\n+Base\r\n-Deps_separator\r\n",
    )
    .unwrap();
    let shared = game.join("r6/scripts/Shared.reds");

    deploy_mo2_to_game(&staging, &game, false).await.unwrap();
    assert_eq!(std::fs::read_to_string(&shared).unwrap(), "expansion");
    assert!(!game.join("r6/scripts/only.reds").exists());

    // A loser left behind by an older deploy is replaced by the winner.
    modde_core::link::link_or_copy(&staging.join("mods/Base/r6/scripts/Shared.reds"), &shared)
        .await
        .unwrap();
    deploy_mo2_to_game(&staging, &game, false).await.unwrap();
    assert_eq!(std::fs::read_to_string(&shared).unwrap(), "expansion");
}
