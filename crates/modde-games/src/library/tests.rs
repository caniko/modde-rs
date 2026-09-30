use super::*;

#[test]
fn owned_catalogue_keeps_unregistered_and_uninstalled_titles() {
    let value = serde_json::json!({"response": {"game_count": 2, "games": [
        {"appid": 987654321, "name": "Unregistered title"}, {"appid": 987654322, "name": "Other title"}
    ]}});
    let games = providers::parse_owned_steam(&value).unwrap();
    assert_eq!(games.len(), 2);
    assert!(
        games
            .iter()
            .all(|game| game.install_path.is_none() && game.game_id.is_none())
    );
}

#[test]
fn private_or_partial_steam_results_cannot_erase_ownership_cache() {
    assert!(providers::parse_owned_steam(&serde_json::json!({"response": {}})).is_err());
    assert!(
        providers::parse_owned_steam(
            &serde_json::json!({"response": {"game_count": 2, "games": []}})
        )
        .is_err()
    );
    assert!(
        providers::parse_owned_steam(&serde_json::json!({"response": {"game_count": 0}}))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn installs_replace_owned_placeholder_without_collapsing_distinct_installs() {
    let make = |path: Option<&str>| {
        LibraryGame::new(
            Store::Steam,
            "1".into(),
            "A".into(),
            path.map(PathBuf::from),
        )
    };
    let entries = merge_games(vec![
        make(None),
        make(Some("/a")),
        make(Some("/b")),
        make(Some("/a")),
    ]);
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].id, entries[1].id);
}

#[test]
fn heroic_cache_is_ownership_not_an_installation_claim() {
    let value =
        serde_json::json!({"games": [{"app_name": "42", "title": "A", "is_installed": true}]});
    let entries = providers::parse_heroic_library(&value, Store::Gog, "games").unwrap();
    assert_eq!(entries[0].entitlement, "gog:42");
    assert!(entries[0].install_path.is_none());
    assert!(providers::parse_heroic_library(&value, Store::Epic, "library").is_err());
}

#[test]
fn local_rediscovery_by_a_store_keeps_settings_and_one_catalogue_row() {
    let dir = tempfile::tempdir().unwrap();
    let local = LibraryGame::new(
        Store::Local,
        "example".into(),
        "Example".into(),
        Some(dir.path().into()),
    );
    let mut prefs = modde_core::library::LibraryPreferences::default();
    let mut launch = modde_core::library::LaunchSettings::default();
    launch.sandbox.enabled = true;
    launch.profile = Some("saved-profile".into());
    prefs.launches.insert(local.id.clone(), launch.clone());
    prefs.favorites.insert(local.entitlement.clone());
    let mut games = vec![
        LibraryGame::new(
            Store::Steam,
            "42".into(),
            "Example".into(),
            Some(dir.path().into()),
        ),
        local.clone(),
    ];
    reconcile_installations(&mut games, &mut prefs).unwrap();
    let merged = merge_games(games);
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].store, Store::Steam);
    assert_eq!(merged[0].id, local.id);
    assert_eq!(prefs.launch_for(&merged[0].id), launch);
    assert!(prefs.favorites.contains("steam:42"));
    prefs.favorites.remove("steam:42");
    let mut games = vec![merged[0].clone(), local];
    reconcile_installations(&mut games, &mut prefs).unwrap();
    assert!(!prefs.favorites.contains("steam:42"));
}
