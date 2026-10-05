#![allow(clippy::wildcard_imports)]
use super::*;
use std::cell::Cell;
use std::sync::{Mutex, MutexGuard, OnceLock};

static ISOLATED_DATA_DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
thread_local! {
    static DB_LOCK_HELD: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn test_db() -> modde_core::db::ModdeDb {
    if DB_LOCK_HELD.with(Cell::get) {
        isolated_data_dir();
        crate::app::block_on(modde_core::db::ModdeDb::open_at(
            &modde_core::paths::db_path(),
        ))
        .expect("isolated SQLite db opens")
    } else {
        crate::app::block_on(modde_core::db::ModdeDb::open_memory()).expect("memory db opens")
    }
}

pub(crate) fn test_profile_manager() -> ProfileManager {
    assert!(
        DB_LOCK_HELD.with(Cell::get),
        "file-backed fixture requires db_lock"
    );
    ProfileManager::with_db(test_db())
}

/// Process-wide test mutex. All lock-refusal tests share one
/// file-backed `SQLite` DB (via the `ISOLATED_DATA_DIR` `OnceLock`).
/// These tests still serialize DB access because the UI handlers call
/// into the app-owned DB handle internally, so they need a predictable
/// process-wide data directory while each fixture is seeded and asserted.
static DB_LOCK: Mutex<()> = Mutex::new(());

pub(crate) struct DbLockGuard {
    _guard: MutexGuard<'static, ()>,
}

impl Drop for DbLockGuard {
    fn drop(&mut self) {
        DB_LOCK_HELD.with(|held| held.set(false));
    }
}

/// Acquire the serial lock. Discards poisoning (a prior panicking
/// test shouldn't block the rest of the suite).
pub(crate) fn db_lock() -> DbLockGuard {
    let guard = DB_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    DB_LOCK_HELD.with(|held| held.set(true));
    DbLockGuard { _guard: guard }
}

/// Redirect `modde_core::paths::modde_data_dir` to a per-process
/// tempdir so lock-refusal tests don't touch `~/.local/share/modde`.
/// `OnceLock::get_or_init` makes this safe (idempotent) on every call.
pub(crate) fn isolated_data_dir() {
    let dir = ISOLATED_DATA_DIR.get_or_init(|| {
        let dir = tempfile::tempdir().expect("create isolated modde data dir");
        modde_core::paths::set_data_dir(dir.path().to_path_buf());
        modde_core::paths::set_config_dir(dir.path().join("config"));
        dir
    });
    assert_eq!(modde_core::paths::modde_data_dir(), dir.path());
    assert_eq!(modde_core::paths::config_dir(), dir.path().join("config"));
}

pub(crate) fn reset_isolated_db() {
    isolated_data_dir();
    let pm = test_profile_manager();
    crate::app::block_on(pm.db().clear_ui_test_state()).expect("clear UI test state during reset");
    let profiles = crate::app::block_on(pm.list()).expect("list isolated profiles for reset");
    for profile in profiles {
        crate::app::block_on(pm.db().clear_active_profile(&profile.game_id))
            .expect("clear active profile during reset");
        crate::app::block_on(pm.db().delete_profile(&profile.name, &profile.game_id))
            .expect("delete profile during reset");
    }
}

#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn optiscaler_release(
    tag: &str,
    asset: &str,
    published_at: Option<&str>,
) -> modde_games::tools::ToolReleaseSummary {
    modde_games::tools::ToolReleaseSummary {
        tag: tag.to_string(),
        name: None,
        published_at: published_at.map(str::to_string),
        assets: vec![modde_games::tools::ToolReleaseAsset {
            name: asset.to_string(),
            download_url: format!("https://example.test/{asset}"),
            size: 10,
        }],
    }
}

#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn complete_optiscaler_release_selection(app: &mut Modde) {
    let game_id = app.current_game_id().expect("game selected").to_string();
    let result = crate::app::block_on(
        crate::app::tool_settings::save_optiscaler_release_selection_for_game(
            app.db.clone(),
            game_id,
            app.tool_state.optiscaler_releases.clone(),
            app.tool_state.tool_option_catalog.clone(),
            app.current_tool_game_context(),
        ),
    );
    let _ = app.update(Message::ToolSettingWritten {
        tool_id: "optiscaler".to_string(),
        result,
    });
}

#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn complete_optiscaler_setting_write(app: &mut Modde, key: &str, value: serde_json::Value) {
    let game_id = app.current_game_id().expect("game selected").to_string();
    let result = crate::app::block_on(crate::app::tool_settings::save_tool_setting_for_game(
        app.db.clone(),
        game_id,
        "optiscaler".to_string(),
        key.to_string(),
        value,
        app.current_tool_game_context(),
        app.tool_state.optiscaler_releases.clone(),
        app.tool_state.tool_option_catalog.clone(),
    ));
    let _ = app.update(Message::ToolSettingWritten {
        tool_id: "optiscaler".to_string(),
        result,
    });
}

#[test]
#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn optiscaler_release_loaded_resets_stale_asset() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let db = test_db();
    let settings = serde_json::json!({
        "release_tag": "official:v0.9.1",
        "release_asset": "stale.zip"
    });
    crate::app::block_on(db.save_tool_config(
        &GameId::from("skyrim-se"),
        "optiscaler",
        false,
        &settings.to_string(),
    ))
    .expect("save stale settings");

    let releases = vec![modde_games::tools::ToolReleaseSummary {
        tag: "official:v0.9.1".to_string(),
        name: None,
        published_at: None,
        assets: vec![modde_games::tools::ToolReleaseAsset {
            name: "Optiscaler_0.9.1-final.7z".to_string(),
            download_url: "https://example.test/optiscaler.7z".to_string(),
            size: 10,
        }],
    }];
    let _ = app.update(Message::OptiScalerReleasesLoaded(Ok(releases)));
    complete_optiscaler_release_selection(&mut app);

    let row = crate::app::block_on(db.load_tool_config(&GameId::from("skyrim-se"), "optiscaler"))
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["release_tag"], "official:v0.9.1");
    assert_eq!(saved["release_asset"], "Optiscaler_0.9.1-final.7z");
    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("optiscaler.release_asset"),
        Some(&vec!["Optiscaler_0.9.1-final.7z".to_string()])
    );
}

#[test]
#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn optiscaler_release_tag_update_resets_asset() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.optiscaler_releases = vec![
        modde_games::tools::ToolReleaseSummary {
            tag: "official:v0.9.0".to_string(),
            name: None,
            published_at: None,
            assets: vec![modde_games::tools::ToolReleaseAsset {
                name: "Optiscaler_0.9.0.7z".to_string(),
                download_url: "https://example.test/0.9.0.7z".to_string(),
                size: 10,
            }],
        },
        modde_games::tools::ToolReleaseSummary {
            tag: "official:v0.9.1".to_string(),
            name: None,
            published_at: None,
            assets: vec![modde_games::tools::ToolReleaseAsset {
                name: "Optiscaler_0.9.1.7z".to_string(),
                download_url: "https://example.test/0.9.1.7z".to_string(),
                size: 11,
            }],
        },
    ];

    let _ = app.update(Message::UpdateToolSetting {
        tool_id: "optiscaler".to_string(),
        key: "release_tag".to_string(),
        value: serde_json::json!("official:v0.9.1"),
    });
    complete_optiscaler_setting_write(
        &mut app,
        "release_tag",
        serde_json::json!("official:v0.9.1"),
    );

    let db = test_db();
    let row = crate::app::block_on(db.load_tool_config(&GameId::from("skyrim-se"), "optiscaler"))
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["release_tag"], "official:v0.9.1");
    assert_eq!(saved["release_asset"], "Optiscaler_0.9.1.7z");
}

#[test]
#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn optiscaler_official_source_filters_out_goverlay_releases() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let db = test_db();
    let settings = serde_json::json!({
        "source_mode": "github_release",
        "release_tag": "official:v0.9.1",
        "release_asset": "Optiscaler_0.9.1.7z"
    });
    crate::app::block_on(db.save_tool_config(
        &GameId::from("skyrim-se"),
        "optiscaler",
        false,
        &settings.to_string(),
    ))
    .expect("save settings");

    let _ = app.update(Message::OptiScalerReleasesLoaded(Ok(vec![
        optiscaler_release("official:v0.9.1", "Optiscaler_0.9.1.7z", None),
        optiscaler_release(
            "goverlay-edge:edge-0.9.12.0323",
            "optiscaler-edge.7z",
            Some("2026-03-24T00:18:25Z"),
        ),
    ])));
    complete_optiscaler_release_selection(&mut app);

    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("optiscaler.release_tag"),
        Some(&vec!["official:v0.9.1".to_string()])
    );
}

#[test]
#[cfg(any(
    all(target_os = "linux", feature = "linux-integrations"),
    all(target_os = "windows", feature = "windows-integrations")
))]
fn optiscaler_goverlay_source_filters_by_channel_and_resets_selection() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());
    app.tool_state.optiscaler_releases = vec![
        optiscaler_release(
            "goverlay-edge:edge-0.9.12.0323",
            "optiscaler-edge.7z",
            Some("2026-03-24T00:18:25Z"),
        ),
        optiscaler_release(
            "goverlay-master:master-3ce61922",
            "OptiScaler_master_3ce61922.7z",
            Some("2026-05-01T04:43:25Z"),
        ),
    ];

    let _ = app.update(Message::UpdateToolSetting {
        tool_id: "optiscaler".to_string(),
        key: "source_mode".to_string(),
        value: serde_json::json!("goverlay_builds"),
    });
    complete_optiscaler_setting_write(
        &mut app,
        "source_mode",
        serde_json::json!("goverlay_builds"),
    );

    let db = test_db();
    let row = crate::app::block_on(db.load_tool_config(&GameId::from("skyrim-se"), "optiscaler"))
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["source_mode"], "goverlay_builds");
    assert_eq!(saved["goverlay_channel"], "edge");
    assert_eq!(saved["release_tag"], "goverlay-edge:edge-0.9.12.0323");
    assert_eq!(saved["release_asset"], "optiscaler-edge.7z");
    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("optiscaler.release_tag"),
        Some(&vec!["goverlay-edge:edge-0.9.12.0323".to_string()])
    );

    let _ = app.update(Message::UpdateToolSetting {
        tool_id: "optiscaler".to_string(),
        key: "goverlay_channel".to_string(),
        value: serde_json::json!("master"),
    });
    complete_optiscaler_setting_write(&mut app, "goverlay_channel", serde_json::json!("master"));
    let row = crate::app::block_on(db.load_tool_config(&GameId::from("skyrim-se"), "optiscaler"))
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["goverlay_channel"], "master");
    assert_eq!(saved["release_tag"], "goverlay-master:master-3ce61922");
    assert_eq!(saved["release_asset"], "OptiScaler_master_3ce61922.7z");
}

#[test]
fn proton_versions_loaded_populates_selected_version_options() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let _ = app.update(Message::ProtonVersionsLoaded(Ok(vec![
        "latest".to_string(),
        "GE-Proton10-34".to_string(),
    ])));

    assert_eq!(
        app.tool_state
            .tool_option_catalog
            .get("proton.selected_version"),
        Some(&vec!["latest".to_string(), "GE-Proton10-34".to_string()])
    );
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn proton_versions_loaded_resets_stale_selected_version() {
    let _guard = db_lock();
    reset_isolated_db();
    let mut app = test_app();
    app.selected_game = Some("skyrim-se".to_string());

    let db = test_db();
    let settings = serde_json::json!({ "selected_version": "GE-Proton9-stale" });
    crate::app::block_on(db.save_tool_config(
        &GameId::from("skyrim-se"),
        "proton",
        false,
        &settings.to_string(),
    ))
    .expect("save stale settings");

    let _ = app.update(Message::ProtonVersionsLoaded(Ok(vec![
        "latest".to_string(),
        "GE-Proton10-34".to_string(),
    ])));
    let result = crate::app::block_on(
        crate::app::tool_settings::save_proton_selected_version_for_game(
            app.db.clone(),
            "skyrim-se".to_string(),
            vec!["latest".to_string(), "GE-Proton10-34".to_string()],
            app.tool_state.tool_option_catalog.clone(),
        ),
    );
    let _ = app.update(Message::ToolSettingWritten {
        tool_id: "proton".to_string(),
        result,
    });

    let row = crate::app::block_on(db.load_tool_config(&GameId::from("skyrim-se"), "proton"))
        .expect("load tool config")
        .expect("tool config exists");
    let saved: serde_json::Value = serde_json::from_str(&row.settings_json).expect("settings json");
    assert_eq!(saved["selected_version"], "latest");
}

#[test]
fn optiscaler_zip_extraction_flattens_payload() {
    let temp = tempfile::tempdir().expect("tempdir");
    let archive_path = temp.path().join("optiscaler.zip");
    let dest = temp.path().join("dest");
    std::fs::create_dir_all(&dest).expect("dest");
    {
        let file = std::fs::File::create(&archive_path).expect("zip file");
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("nested/OptiScaler.dll", options)
            .expect("start dll");
        std::io::Write::write_all(&mut zip, b"dll").expect("write dll");
        zip.start_file("nested/OptiScaler.ini", options)
            .expect("start ini");
        std::io::Write::write_all(&mut zip, b"ini").expect("write ini");
        zip.start_file(
            "nested/FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll",
            options,
        )
        .expect("start fp8");
        std::io::Write::write_all(&mut zip, b"fp8").expect("write fp8");
        zip.start_file("nested/FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll", options)
            .expect("start int8");
        std::io::Write::write_all(&mut zip, b"int8").expect("write int8");
        zip.start_file("nested/readme.txt", options)
            .expect("start txt");
        std::io::Write::write_all(&mut zip, b"txt").expect("write txt");
        zip.finish().expect("finish zip");
    }

    modde_games::tools::optiscaler::extract_optiscaler_archive_flat(&archive_path, &dest)
        .expect("extract zip");
    assert!(dest.join("OptiScaler.dll").exists());
    assert!(dest.join("OptiScaler.ini").exists());
    assert_eq!(
        std::fs::read(dest.join("FSR4_LATEST/amd_fidelityfx_upscaler_dx12.dll")).expect("fp8"),
        b"fp8"
    );
    assert_eq!(
        std::fs::read(dest.join("FSR4_INT8/amd_fidelityfx_upscaler_dx12.dll")).expect("int8"),
        b"int8"
    );
    assert!(!dest.join("readme.txt").exists());
}

#[test]
fn optiscaler_7z_extraction_flattens_payload_when_7z_available() {
    let sevenz = ["7zz", "7z"].into_iter().find(|bin| {
        std::process::Command::new(bin)
            .arg("--help")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    });
    let Some(sevenz) = sevenz else {
        return;
    };

    let _guard = db_lock();
    isolated_data_dir();
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("source");
    let dest = temp.path().join("dest");
    let archive_path = temp.path().join("optiscaler.7z");
    std::fs::create_dir_all(source.join("nested")).expect("source");
    std::fs::create_dir_all(&dest).expect("dest");
    std::fs::write(source.join("nested/OptiScaler.dll"), b"dll").expect("dll");
    std::fs::write(source.join("nested/OptiScaler.ini"), b"ini").expect("ini");

    let status = std::process::Command::new(sevenz)
        .arg("a")
        .arg("-y")
        .arg(&archive_path)
        .arg(source.join("nested"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("run 7z");
    assert!(status.success());

    modde_games::tools::optiscaler::extract_optiscaler_archive_flat(&archive_path, &dest)
        .expect("extract 7z");
    assert!(dest.join("OptiScaler.dll").exists());
    assert!(dest.join("OptiScaler.ini").exists());
}
