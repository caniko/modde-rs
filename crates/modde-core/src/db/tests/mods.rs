use super::*;
use crate::db::rows::{decode_source, encode_source};

fn plugin(name: &str, sort_index: i64) -> PluginEntry {
    PluginEntry {
        plugin_name: name.to_string(),
        sort_index,
        enabled: true,
    }
}

fn plugin_names(order: &[PluginEntry]) -> Vec<&str> {
    order.iter().map(|p| p.plugin_name.as_str()).collect()
}

fn staged(rel_path: &str, merge_group: Option<&str>) -> StagedFile {
    StagedFile {
        rel_path: rel_path.to_string(),
        origin_rel_path: format!("archive/{rel_path}"),
        size: 3,
        merge_group: merge_group.map(String::from),
    }
}

fn plan(staged_files: Vec<StagedFile>) -> InstallPlan {
    InstallPlan {
        method: InstallMethod::BareExtract,
        strip_prefix: None,
        source_archive_hash: "abc".to_string(),
        staged_files,
    }
}

fn rel_paths(files: Vec<StagedFile>) -> Vec<String> {
    files.into_iter().map(|f| f.rel_path).collect()
}

#[tokio::test]
async fn plugin_order_is_sorted_toggled_and_replaced() {
    let db = test_db().await;
    let id = db
        .create_profile(&sample_profile("p", "skyrim-se"))
        .await
        .unwrap();

    db.set_plugin_order(id, &[plugin("b.esp", 2), plugin("a.esp", 1)])
        .await
        .unwrap();
    db.toggle_plugin(id, "b.esp", false).await.unwrap();
    let order = db.get_plugin_order(id).await.unwrap();
    assert_eq!(plugin_names(&order), ["a.esp", "b.esp"]);
    assert!(order[0].enabled && !order[1].enabled);

    db.set_plugin_order(id, &[plugin("c.esp", 0)])
        .await
        .unwrap();
    assert_eq!(
        plugin_names(&db.get_plugin_order(id).await.unwrap()),
        ["c.esp"]
    );
}

#[tokio::test]
async fn failed_plugin_order_write_keeps_previous_order() {
    let db = test_db().await;
    let id = db
        .create_profile(&sample_profile("p", "skyrim-se"))
        .await
        .unwrap();
    db.set_plugin_order(id, &[plugin("a.esp", 0)])
        .await
        .unwrap();

    // UNIQUE(profile_id, plugin_name) rejects the second row.
    let duplicate = [plugin("x.esp", 0), plugin("x.esp", 1)];
    assert!(db.set_plugin_order(id, &duplicate).await.is_err());
    assert_eq!(
        plugin_names(&db.get_plugin_order(id).await.unwrap()),
        ["a.esp"]
    );
}

#[tokio::test]
async fn record_install_replaces_manifest_and_remove_returns_files() {
    let db = test_db().await;
    let id = db
        .create_profile(&sample_profile("p", "skyrim-se"))
        .await
        .unwrap();
    let mod_a = ModId::from("mod_a");

    db.record_install(
        id,
        &mod_a,
        &plan(vec![staged("old.esp", None)]),
        InstallStatus::Installed,
    )
    .await
    .unwrap();
    db.record_install(
        id,
        &mod_a,
        &plan(vec![staged("z.esp", None), staged("a.esp", None)]),
        InstallStatus::Installed,
    )
    .await
    .unwrap();
    assert_eq!(
        rel_paths(db.installed_files_for_mod(id, &mod_a).await.unwrap()),
        ["a.esp", "z.esp"]
    );

    assert_eq!(
        rel_paths(db.remove_installed_mod(id, &mod_a).await.unwrap()),
        ["a.esp", "z.esp"]
    );
    assert_eq!(
        rel_paths(db.installed_files_for_mod(id, &mod_a).await.unwrap()),
        [] as [&str; 0]
    );
    let loaded = db
        .load_profile("p", &GameId::from("skyrim-se"))
        .await
        .unwrap();
    let mods: Vec<&str> = loaded.mods.iter().map(|m| m.mod_id.as_str()).collect();
    assert_eq!(mods, ["mod_b"]);
}

#[tokio::test]
async fn merge_group_spans_mods_in_mod_then_path_order() {
    let db = test_db().await;
    let id = db
        .create_profile(&sample_profile("p", "skyrim-se"))
        .await
        .unwrap();
    db.record_install(
        id,
        &ModId::from("mod_b"),
        &plan(vec![
            staged("s.psc", Some("scripts")),
            staged("other", None),
        ]),
        InstallStatus::Installed,
    )
    .await
    .unwrap();
    db.record_install(
        id,
        &ModId::from("mod_a"),
        &plan(vec![staged("t.psc", Some("scripts"))]),
        InstallStatus::Installed,
    )
    .await
    .unwrap();

    let group: Vec<String> = db
        .files_in_merge_group(id, "scripts")
        .await
        .unwrap()
        .into_iter()
        .map(|(mod_id, file)| format!("{mod_id}:{}", file.rel_path))
        .collect();
    assert_eq!(group, ["mod_a:t.psc", "mod_b:s.psc"]);
}

#[tokio::test]
async fn applied_files_dedupe_and_clear_per_tool_and_game() {
    let db = test_db().await;
    let game = GameId::from("cyberpunk2077");
    let other_game = GameId::from("skyrim-se");
    let files = |paths: &[&str]| paths.iter().map(|p| (*p).to_string()).collect::<Vec<_>>();

    db.save_applied_files(&game, "reshade", &files(&["dxgi.dll", "dxgi.dll"]))
        .await
        .unwrap();
    db.save_applied_files(&game, "optiscaler", &files(&["b.ini", "a.dll"]))
        .await
        .unwrap();
    db.save_applied_files(&other_game, "reshade", &files(&["other.dll"]))
        .await
        .unwrap();
    assert_eq!(
        db.load_applied_files(&game, "reshade").await.unwrap(),
        ["dxgi.dll"]
    );
    assert_eq!(
        db.load_all_applied_files(&game).await.unwrap(),
        ["a.dll", "b.ini", "dxgi.dll"]
    );

    db.clear_applied_files(&game, "reshade").await.unwrap();
    let rows: Vec<String> = db
        .load_all_applied_file_rows(&game)
        .await
        .unwrap()
        .into_iter()
        .map(|row| format!("{}:{}", row.tool_id, row.rel_path))
        .collect();
    assert_eq!(rows, ["optiscaler:a.dll", "optiscaler:b.ini"]);
    assert_eq!(
        db.load_applied_files(&other_game, "reshade").await.unwrap(),
        ["other.dll"]
    );
}

#[test]
fn source_encoding_round_trips_control_characters() {
    let source = ProfileSource::NexusCollection {
        slug: "a\u{7f}\"b\\c".to_string(),
        version: "1\t2\0".to_string(),
    };
    let (kind, data) = encode_source(&source);
    assert_eq!(decode_source(kind, data.as_deref()).unwrap(), source);
}

#[test]
fn source_decoding_rejects_unknown_kinds_and_defaults_missing_data() {
    assert!(decode_source("bogus", None).is_err());
    assert_eq!(
        decode_source("nexus_collection", None).unwrap(),
        ProfileSource::NexusCollection {
            slug: String::new(),
            version: String::new(),
        }
    );
}

async fn profile_with_mods(db: &ModdeDb, name: &str) -> i64 {
    db.create_profile(&sample_profile(name, "skyrim-se"))
        .await
        .unwrap()
}

async fn loaded_mod(db: &ModdeDb, name: &str, mod_id: &str) -> EnabledMod {
    db.load_profile(name, &GameId::from("skyrim-se"))
        .await
        .unwrap()
        .mods
        .into_iter()
        .find(|m| m.mod_id == mod_id)
        .unwrap()
}

#[tokio::test]
async fn hidden_files_are_idempotent_and_listed_per_mod() {
    let db = test_db().await;
    let id = profile_with_mods(&db, "p").await;
    let (mod_a, mod_b) = (ModId::from("mod_a"), ModId::from("mod_b"));

    db.hide_file(id, &mod_a, "a.esp").await.unwrap();
    db.hide_file(id, &mod_a, "a.esp").await.unwrap();
    db.hide_file(id, &mod_b, "b.esp").await.unwrap();
    assert_eq!(db.list_hidden_files(id).await.unwrap().len(), 2);
    assert_eq!(
        db.list_hidden_files_for_mod(id, &mod_a).await.unwrap(),
        ["a.esp"]
    );

    db.unhide_file(id, &mod_a, "a.esp").await.unwrap();
    assert_eq!(
        db.list_hidden_files_for_mod(id, &mod_a).await.unwrap(),
        [] as [&str; 0]
    );
}

#[tokio::test]
async fn categories_reorder_and_detach_mods_on_delete() {
    let db = test_db().await;
    let id = profile_with_mods(&db, "p").await;
    let category = |name: &str, sort_index| ModCategory {
        id: None,
        name: name.to_string(),
        color: None,
        sort_index,
    };
    let ui = db.create_category(id, &category("UI", 1)).await.unwrap();
    let gameplay = db
        .create_category(id, &category("Gameplay", 2))
        .await
        .unwrap();
    db.update_category(gameplay, "Gameplay", Some("#ff0000"), 0)
        .await
        .unwrap();
    let listed = db.list_categories(id).await.unwrap();
    let names: Vec<&str> = listed.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["Gameplay", "UI"]);
    assert_eq!(listed[0].color.as_deref(), Some("#ff0000"));

    db.set_mod_category(id, &ModId::from("mod_a"), Some(ui))
        .await
        .unwrap();
    assert_eq!(loaded_mod(&db, "p", "mod_a").await.category_id, Some(ui));
    db.delete_category(ui).await.unwrap();
    assert_eq!(loaded_mod(&db, "p", "mod_a").await.category_id, None);
    assert_eq!(db.list_categories(id).await.unwrap().len(), 1);
}

#[tokio::test]
async fn mod_notes_tags_and_nexus_metadata_persist() {
    let db = test_db().await;
    let id = profile_with_mods(&db, "p").await;
    let mod_a = ModId::from("mod_a");

    db.set_mod_notes(id, &mod_a, Some("keep last"))
        .await
        .unwrap();
    db.set_mod_tags(id, &mod_a, &["ui".to_string(), "core".to_string()])
        .await
        .unwrap();
    db.set_mod_nexus_meta(
        id,
        &mod_a,
        NexusModId::from(42),
        NexusFileId::from(7),
        "skyrimspecialedition",
        1_700_000_000,
    )
    .await
    .unwrap();

    let loaded = loaded_mod(&db, "p", "mod_a").await;
    assert_eq!(loaded.notes.as_deref(), Some("keep last"));
    assert_eq!(loaded.tags, ["ui", "core"]);
    assert_eq!(loaded.nexus_mod_id, Some(NexusModId::from(42)));
    assert_eq!(loaded.nexus_file_id, Some(NexusFileId::from(7)));
    assert_eq!(
        loaded.nexus_game_domain.as_deref(),
        Some("skyrimspecialedition")
    );
    assert_eq!(loaded.installed_timestamp, Some(1_700_000_000));

    db.set_mod_notes(id, &mod_a, None).await.unwrap();
    assert_eq!(loaded_mod(&db, "p", "mod_a").await.notes, None);
}

#[tokio::test]
async fn auxiliary_state_copies_to_another_profile() {
    let db = test_db().await;
    let source = profile_with_mods(&db, "source").await;
    let target = profile_with_mods(&db, "target").await;
    let mod_a = ModId::from("mod_a");
    db.hide_file(source, &mod_a, "hidden.esp").await.unwrap();
    db.set_plugin_order(source, &[plugin("a.esp", 0)])
        .await
        .unwrap();
    db.record_install(
        source,
        &mod_a,
        &plan(vec![staged("a.esp", None)]),
        InstallStatus::Installed,
    )
    .await
    .unwrap();

    db.copy_profile_auxiliary_state(source, target)
        .await
        .unwrap();
    assert_eq!(
        db.list_hidden_files_for_mod(target, &mod_a).await.unwrap(),
        ["hidden.esp"]
    );
    assert_eq!(
        plugin_names(&db.get_plugin_order(target).await.unwrap()),
        ["a.esp"]
    );
    assert_eq!(
        rel_paths(db.installed_files_for_mod(target, &mod_a).await.unwrap()),
        ["a.esp"]
    );
    assert_eq!(
        db.installed_files_for_profile(target).await.unwrap().len(),
        1
    );
}
