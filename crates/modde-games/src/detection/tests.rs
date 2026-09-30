use super::*;
// ── Heroic store file parsing ─────────────────────────────────────

fn write_heroic_installed(dir: &std::path::Path, entries: &[(&str, &str)]) {
    let items: Vec<serde_json::Value> = entries
        .iter()
        .map(|(app_name, install_path)| {
            serde_json::json!({
                "appName": app_name,
                "install_path": install_path,
            })
        })
        .collect();
    let json = serde_json::json!({ "installed": items });
    std::fs::write(dir, serde_json::to_string(&json).unwrap()).unwrap();
}

#[test]
#[cfg(feature = "cyberpunk")]
fn scan_heroic_gog_detects_known_game() {
    let tmp = tempfile::tempdir().unwrap();
    let install_dir = tmp.path().join("cyberpunk");
    std::fs::create_dir_all(&install_dir).unwrap();

    let store_file = tmp.path().join("installed.json");
    write_heroic_installed(
        &store_file,
        &[("1423049311", &install_dir.to_string_lossy())],
    );

    let mut detected = Vec::new();
    scan_heroic_store_file(
        &store_file,
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Gog))
        },
        &mut detected,
    );

    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].game_id, "cyberpunk2077");
    assert_eq!(detected[0].install_path, install_dir);
    assert!(matches!(
        detected[0].source,
        LauncherSource::HeroicGog { .. }
    ));
}

#[test]
fn scan_heroic_gog_unknown_game_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let install_dir = tmp.path().join("some_game");
    std::fs::create_dir_all(&install_dir).unwrap();

    let store_file = tmp.path().join("installed.json");
    write_heroic_installed(
        &store_file,
        &[("9999999999", &install_dir.to_string_lossy())],
    );

    let mut detected = Vec::new();
    scan_heroic_store_file(
        &store_file,
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Gog))
        },
        &mut detected,
    );

    assert_eq!(detected.len(), 0, "unknown game should not be added");
}

#[test]
fn scan_heroic_nonexistent_install_path_skipped() {
    let tmp = tempfile::tempdir().unwrap();
    let store_file = tmp.path().join("installed.json");
    // Path does not exist on disk
    write_heroic_installed(&store_file, &[("1423049311", "/nonexistent/cyberpunk")]);

    let mut detected = Vec::new();
    scan_heroic_store_file(
        &store_file,
        |app_id| {
            launcher_games()
                .find(|g| g.launcher.heroic_gog_app_id == Some(app_id))
                .map(|g| (g, HeroicStoreKind::Gog))
        },
        &mut detected,
    );

    assert_eq!(
        detected.len(),
        0,
        "nonexistent install path should be skipped"
    );
}

#[test]
fn scan_heroic_missing_file_is_no_op() {
    let mut detected = Vec::new();
    // Should not panic
    scan_heroic_store_file(
        std::path::Path::new("/nonexistent/installed.json"),
        |_| None,
        &mut detected,
    );
    assert_eq!(detected.len(), 0);
}

#[test]
fn scan_heroic_malformed_json_is_no_op() {
    let tmp = tempfile::tempdir().unwrap();
    let store_file = tmp.path().join("installed.json");
    std::fs::write(&store_file, "this is not json").unwrap();

    let mut detected = Vec::new();
    scan_heroic_store_file(&store_file, |_| None, &mut detected);
    assert_eq!(detected.len(), 0);
}

#[test]
fn scan_heroic_empty_installed_array() {
    let tmp = tempfile::tempdir().unwrap();
    let store_file = tmp.path().join("installed.json");
    std::fs::write(&store_file, r#"{"installed":[]}"#).unwrap();

    let mut detected = Vec::new();
    scan_heroic_store_file(&store_file, |_| None, &mut detected);
    assert_eq!(detected.len(), 0);
}

#[test]
#[cfg(feature = "cyberpunk")]
fn scan_heroic_sideload_matches_by_dirname() {
    let tmp = tempfile::tempdir().unwrap();
    // Create a dir named like the Cyberpunk Steam dir
    let install_dir = tmp.path().join("Cyberpunk 2077");
    std::fs::create_dir_all(&install_dir).unwrap();

    let store_file = tmp.path().join("installed.json");
    write_heroic_installed(
        &store_file,
        &[("some_sideload_id", &install_dir.to_string_lossy())],
    );

    let mut detected = Vec::new();
    scan_heroic_sideload(&store_file, &mut detected);

    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].game_id, "cyberpunk2077");
    assert!(matches!(
        detected[0].source,
        LauncherSource::HeroicSideload { .. }
    ));
}

#[test]
fn scan_heroic_sideload_unknown_dirname_ignored() {
    let tmp = tempfile::tempdir().unwrap();
    let install_dir = tmp.path().join("Some Unknown Game 2077");
    std::fs::create_dir_all(&install_dir).unwrap();

    let store_file = tmp.path().join("installed.json");
    write_heroic_installed(&store_file, &[("some_id", &install_dir.to_string_lossy())]);

    let mut detected = Vec::new();
    scan_heroic_sideload(&store_file, &mut detected);

    assert_eq!(detected.len(), 0);
}

#[test]
fn scan_heroic_sideload_missing_file_is_no_op() {
    let mut detected = Vec::new();

    scan_heroic_sideload(
        std::path::Path::new("/nonexistent/installed.json"),
        &mut detected,
    );

    assert_eq!(detected.len(), 0);
}

// ── Steam library scanning ────────────────────────────────────────

#[cfg(feature = "ue4")]
fn write_steam_appmanifest(
    steamapps_dir: &std::path::Path,
    appid: &str,
    name: &str,
    installdir: &str,
) -> PathBuf {
    let manifest = format!(
        r#""AppState"
{{
"appid"        "{appid}"
"Universe"        "1"
"name"        "{name}"
"StateFlags"        "4"
"installdir"        "{installdir}"
}}
"#
    );
    let path = steamapps_dir.join(format!("appmanifest_{appid}.acf"));
    std::fs::write(&path, manifest).unwrap();
    path
}

#[test]
fn parse_steam_appmanifest_reads_required_fields() {
    let content = r#""AppState"
{
"appid"        "3489700"
"Universe"        "1"
"name"        "Stellar Blade™"
"StateFlags"        "4"
"installdir"        "StellarBlade"
}
"#;

    let manifest = parse_steam_appmanifest(content).unwrap();

    assert_eq!(
        manifest,
        SteamAppManifest {
            appid: "3489700".to_string(),
            name: "Stellar Blade™".to_string(),
            installdir: "StellarBlade".to_string(),
        }
    );
}

#[test]
#[cfg(feature = "ue4")]
fn scan_steam_library_detects_manifest_installdir_in_standard_library() {
    let tmp = tempfile::tempdir().unwrap();
    let steamapps = tmp.path().join("steamapps");
    let install_path = steamapps.join("common/StellarBlade");
    std::fs::create_dir_all(&install_path).unwrap();
    write_steam_appmanifest(&steamapps, "3489700", "Stellar Blade™", "StellarBlade");

    let mut detected = Vec::new();
    scan_steam_library(tmp.path(), &mut detected);

    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].game_id, "stellar-blade");
    assert_eq!(detected[0].install_path, install_path);
    assert!(matches!(
        detected[0].source,
        LauncherSource::Steam { ref app_id, .. } if app_id == "3489700"
    ));
}

#[test]
#[cfg(feature = "ue4")]
fn scan_steam_library_detects_manifest_installdir_in_nested_steamapps_library() {
    let tmp = tempfile::tempdir().unwrap();
    let reported_library = tmp.path().join("steamapps");
    let steamapps = reported_library.join("steamapps");
    let install_path = steamapps.join("common/StellarBlade");
    std::fs::create_dir_all(&install_path).unwrap();
    write_steam_appmanifest(&steamapps, "3489700", "Stellar Blade™", "StellarBlade");

    let mut detected = Vec::new();
    scan_steam_library(&reported_library, &mut detected);

    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].game_id, "stellar-blade");
    assert_eq!(detected[0].install_path, install_path);
}

#[test]
#[cfg(feature = "ue4")]
fn scan_steam_library_uses_manifest_installdir_not_known_steam_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let steamapps = tmp.path().join("steamapps");
    let install_path = steamapps.join("common/StellarBlade");
    std::fs::create_dir_all(&install_path).unwrap();
    write_steam_appmanifest(&steamapps, "3489700", "Stellar Blade™", "StellarBlade");

    assert_eq!(
        launcher_games()
            .find(|g| g.game_id == "stellar-blade")
            .unwrap()
            .launcher
            .steam_dir,
        Some("Stellar Blade")
    );

    let mut detected = Vec::new();
    scan_steam_library(tmp.path(), &mut detected);

    assert_eq!(detected.len(), 1);
    assert_eq!(detected[0].install_path, install_path);
}

// ── LauncherSource display ────────────────────────────────────────

#[test]
fn launcher_source_display_steam() {
    let src = LauncherSource::Steam {
        app_id: "1091500".to_string(),
        library_path: PathBuf::from("/games"),
    };
    assert_eq!(src.to_string(), "Steam (1091500)");
}

#[test]
fn launcher_source_display_heroic_gog() {
    let src = LauncherSource::HeroicGog {
        app_id: "1423049311".to_string(),
    };
    assert_eq!(src.to_string(), "Heroic/GOG (1423049311)");
}

#[test]
fn launcher_source_display_heroic_epic() {
    let src = LauncherSource::HeroicEpic {
        app_id: "Ginger".to_string(),
    };
    assert_eq!(src.to_string(), "Heroic/Epic (Ginger)");
}

#[test]
fn launcher_source_display_sideload() {
    let src = LauncherSource::HeroicSideload {
        app_id: "custom_app".to_string(),
    };
    assert_eq!(src.to_string(), "Heroic/Sideload (custom_app)");
}

// ── Registry integrity ────────────────────────────────────────────

#[test]
fn launcher_game_ids_are_unique() {
    let ids: Vec<_> = launcher_games().map(|g| g.game_id).collect();
    let deduped: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(
        ids.len(),
        deduped.len(),
        "launcher registry has duplicate game_ids"
    );
}

#[test]
fn launcher_registry_includes_detectable_supported_games() {
    use crate::SUPPORTED_GAME_IDS;
    for &game_id in SUPPORTED_GAME_IDS.iter().filter(|g| **g != "skyrim-ae")
    // AE intentionally shares SE's steam dir
    {
        if ["skyrim-se", "fallout4", "cyberpunk2077"].contains(&game_id) {
            assert!(
                launcher_games().any(|g| g.game_id == game_id),
                "launcher registry missing {game_id}"
            );
        }
    }
}
