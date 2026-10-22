//! Default `GamePlugin` behaviour for a plugin implementing only the
//! required methods.

use std::path::{Path, PathBuf};

use modde_games::traits::ContentCategory;
use modde_games::{GamePlugin, HotDeploySupport, ModSafety};

struct Minimal;

impl GamePlugin for Minimal {
    fn game_id(&self) -> &'static str {
        "minimal"
    }

    fn display_name(&self) -> &'static str {
        "Minimal Game"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("mods")
    }
}

fn write(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

#[test]
fn defaults_describe_a_game_without_optional_features() {
    let game = Minimal;
    let install = Path::new("/games/minimal");

    assert_eq!(game.mod_root(install).unwrap(), install.join("mods"));
    assert_eq!(
        game.mod_root_at(install, Some(Path::new("/prefix")))
            .unwrap(),
        install.join("mods")
    );
    assert_eq!(game.executable_dir(install), install);
    assert_eq!(
        game.hot_deploy_capability().support,
        HotDeploySupport::Unsupported
    );
    assert!(game.post_deploy_at(install, None).is_ok());
    assert_eq!(game.save_directory(), None);
    assert_eq!(game.save_directory_at(install, None), None);
    assert!(matches!(game.deploy_targets(), []));
    assert_eq!(game.resolve_deploy_target_at("any", install, None), None);
    assert_eq!(
        game.resolve_deploy_target_at("any", install, Some(Path::new("/prefix"))),
        None
    );
    assert!(!game.supports_save_profiles());
    assert_eq!(game.classify_mod(install), ModSafety::Unknown);
    assert!(matches!(game.wine_dll_overrides(install).as_slice(), []));
    assert!(matches!(
        game.wine_dll_overrides_from_staging(install).as_slice(),
        []
    ));
    assert!(matches!(
        (game.ini_file_names(), game.archive_extensions()),
        ([], [])
    ));
    assert!(!game.has_plugin_system());
    assert_eq!(game.steam_app_id_u32(), None);
    assert_eq!(game.plugins_txt_folder(), None);
    assert_eq!(game.nexus_game_domain(), None);
    assert_eq!(game.nexus_game_id_u32(), None);
    assert!(game.analyze_mod_archive(install).is_none());
    assert!(!game.recognizes_bare_layout(install));
}

#[test]
fn default_deploy_links_staging_into_the_mod_root() {
    let tmp = tempfile::tempdir().unwrap();
    let staging = tmp.path().join("staging");
    write(&staging.join("ModA/data.pak"));
    let install = tmp.path().join("install");

    Minimal
        .deploy_to_install_at(&staging, &install, None)
        .unwrap();
    let deployed = install.join("mods/ModA/data.pak");
    assert_eq!(std::fs::read(&deployed).unwrap(), b"x");
    assert!(
        std::fs::symlink_metadata(&deployed)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}

#[test]
fn content_is_classified_by_extension() {
    let game = Minimal;
    for (ext, category) in [
        ("esp", ContentCategory::Plugin),
        ("dds", ContentCategory::Texture),
        ("nif", ContentCategory::Mesh),
        ("ogg", ContentCategory::Sound),
        ("reds", ContentCategory::Script),
        ("swf", ContentCategory::Interface),
        ("ba2", ContentCategory::Archive),
        ("toml", ContentCategory::Config),
        ("so", ContentCategory::Binary),
        ("txt", ContentCategory::Other),
    ] {
        assert_eq!(game.classify_extension(ext), category, "{ext}");
    }

    let tmp = tempfile::tempdir().unwrap();
    for file in [
        "a.ESP",
        "textures/b.dds",
        "textures/c.DDS",
        "readme",
        "notes.txt",
    ] {
        write(&tmp.path().join(file));
    }
    let summary = game.summarize_content(tmp.path());
    assert_eq!(summary.counts.get(&ContentCategory::Plugin), Some(&1));
    assert_eq!(summary.counts.get(&ContentCategory::Texture), Some(&2));
    assert_eq!(summary.counts.get(&ContentCategory::Other), Some(&1));
    assert_eq!(
        game.summarize_content(&tmp.path().join("missing")).counts,
        std::collections::HashMap::new()
    );
}
