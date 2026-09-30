use std::collections::HashSet;

use modde_games::{SUPPORTED_GAME_IDS, all_games, normalize_wabbajack_game};

#[test]
fn registry_game_ids_are_unique() {
    let ids: Vec<_> = all_games().iter().map(|game| game.game_id).collect();
    let unique: HashSet<_> = ids.iter().copied().collect();

    assert_eq!(ids.len(), unique.len(), "registry has duplicate game IDs");
}

#[test]
fn supported_game_ids_are_registry_order() {
    let registry_ids: Vec<_> = all_games().iter().map(|game| game.game_id).collect();

    assert_eq!(SUPPORTED_GAME_IDS, registry_ids.as_slice());
}

#[test]
fn registry_resolves_public_components_consistently() {
    for game in all_games() {
        let plugin = modde_games::resolve_game_plugin(game.game_id).unwrap();
        assert_eq!(plugin.game_id(), game.game_id);
        assert_eq!(plugin.display_name(), game.display_name);
        assert_eq!(
            modde_games::resolve_mod_scanner(game.game_id).is_some(),
            game.scanner.is_some(),
            "{} scanner mismatch",
            game.game_id
        );
        assert_eq!(
            modde_games::resolve_save_tracker(game.game_id).is_some(),
            game.save_tracker.is_some(),
            "{} save tracker mismatch",
            game.game_id
        );
        assert_eq!(
            modde_games::resolve_collision_classifier(game.game_id).is_some(),
            game.collision_classifier.is_some(),
            "{} collision classifier mismatch",
            game.game_id
        );
        assert_eq!(
            modde_games::supports_save_profiles(game.game_id),
            game.supports_save_profiles,
            "{} save support mismatch",
            game.game_id
        );
    }
}

#[test]
fn registry_preserves_known_nexus_metadata() {
    let expected = [
        ("skyrim-se", Some("skyrimspecialedition"), Some(1704)),
        ("skyrim-ae", Some("skyrimspecialedition"), Some(1704)),
        ("fallout4", Some("fallout4"), Some(1151)),
        ("fallout76", Some("fallout76"), Some(2590)),
        ("starfield", Some("starfield"), Some(4187)),
        ("cyberpunk2077", Some("cyberpunk2077"), Some(3333)),
        ("stellar-blade", Some("stellarblade"), None),
    ];

    for (game_id, domain, numeric_id) in expected {
        let Some(game) = modde_games::resolve_game(game_id) else {
            continue;
        };
        assert_eq!(game.nexus_domain, domain, "{game_id} Nexus domain");
        assert_eq!(game.nexus_game_id, numeric_id, "{game_id} Nexus numeric ID");
    }
}

#[test]
fn registry_preserves_wabbajack_normalization() {
    for (name, id, enabled) in [
        (
            "Cyberpunk2077",
            "cyberpunk2077",
            cfg!(feature = "cyberpunk"),
        ),
        (
            "SkyrimSpecialEdition",
            "skyrim-se",
            cfg!(feature = "bethesda"),
        ),
        ("SkyrimAE", "skyrim-ae", cfg!(feature = "bethesda")),
        ("Fallout4", "fallout4", cfg!(feature = "bethesda")),
        ("Fallout76", "fallout76", cfg!(feature = "bethesda")),
        ("Starfield", "starfield", cfg!(feature = "bethesda")),
        ("StellarBlade", "stellar-blade", cfg!(feature = "ue4")),
    ] {
        assert_eq!(normalize_wabbajack_game(name), enabled.then_some(id));
    }
}

#[test]
fn registry_contains_exactly_the_enabled_games() {
    for (enabled, ids) in [
        (
            cfg!(feature = "bethesda"),
            &[
                "skyrim-se",
                "skyrim-ae",
                "fallout4",
                "fallout76",
                "starfield",
            ][..],
        ),
        (
            cfg!(feature = "gamebryo"),
            &["fallout-new-vegas", "oblivion"][..],
        ),
        (cfg!(feature = "cyberpunk"), &["cyberpunk2077"][..]),
        (cfg!(feature = "ue4"), &["stellar-blade", "subnautica2"][..]),
        (cfg!(feature = "bg3"), &["baldurs-gate3"][..]),
        (cfg!(feature = "stardew"), &["stardew-valley"][..]),
        (cfg!(feature = "bannerlord"), &["bannerlord"][..]),
        (cfg!(feature = "witcher3"), &["witcher3"][..]),
        (
            cfg!(feature = "oblivion-remastered"),
            &["oblivion-remastered"][..],
        ),
    ] {
        for id in ids {
            assert_eq!(SUPPORTED_GAME_IDS.contains(id), enabled, "{id}");
            assert_eq!(modde_games::resolve_game(id).is_some(), enabled, "{id}");
            assert_eq!(
                modde_games::resolve_game_plugin(id).is_some(),
                enabled,
                "{id}"
            );
            assert_eq!(
                modde_games::registry::launcher_games().any(|game| game.game_id == *id),
                enabled && *id != "skyrim-ae",
                "{id}"
            );
            let spec = modde_games::generic::spec::GameSpec {
                id: (*id).into(),
                display_name: "Override".into(),
                steam_app_id: None,
                install_dir_name: None,
                install_path_override: None,
                executable_dir: ".".into(),
                mod_dir: None,
                nexus_domain: None,
                proxy_dlls: vec![],
            };
            assert!(spec.validate().is_err(), "reserved ID {id} accepted");
        }
    }
}
