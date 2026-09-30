use std::fs;
use std::io::Write;
use std::sync::OnceLock;

#[test]
fn plain_select_options_use_value_as_label() {
    let spec = super::ToolSettingSpec::select("mode", "Mode", "", &["0", "1"]);

    let super::ToolSettingKind::Select { options } = spec.kind else {
        panic!("expected select");
    };
    assert_eq!(options[0].value, "0");
    assert_eq!(options[0].label, "0");
    assert_eq!(options[1].to_string(), "1");
}

#[test]
fn labeled_select_options_keep_distinct_value_and_label() {
    let spec = super::ToolSettingSpec::labeled_select(
        "mode",
        "Mode",
        "",
        &[("0", "0 - Conservative"), ("1", "1 - Aggressive")],
    );

    let super::ToolSettingKind::Select { options } = spec.kind else {
        panic!("expected select");
    };
    assert_eq!(options[0].value, "0");
    assert_eq!(options[0].label, "0 - Conservative");
    assert_eq!(options[1].to_string(), "1 - Aggressive");
}

#[test]
fn setting_specs_are_not_advanced_by_default() {
    let plain = super::ToolSettingSpec::text("mode", "Mode", "");
    let advanced = plain.clone().advanced();

    assert!(!plain.advanced);
    assert!(advanced.advanced);
}

#[test]
fn every_tool_has_ui_metadata_and_serializable_defaults() {
    for tool in super::all_tools() {
        assert!(!tool.description().trim().is_empty(), "{}", tool.tool_id());
        assert!(
            !tool.settings_schema().is_empty(),
            "{} should expose UI settings",
            tool.tool_id()
        );
        serde_json::to_string(&tool.default_config()).expect("default config serializes");
    }
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn tool_registry_linux_includes_linux_and_proton_tools() {
    let ids: Vec<_> = super::all_tools()
        .iter()
        .map(|tool| tool.tool_id())
        .collect();
    assert_eq!(
        ids,
        vec![
            "mangohud",
            "vkbasalt",
            "gamemode",
            "reshade",
            "optiscaler",
            "proton"
        ]
    );
}

#[test]
#[cfg(all(target_os = "windows", feature = "windows-integrations"))]
fn tool_registry_windows_hides_linux_only_tools() {
    let ids: Vec<_> = super::all_tools()
        .iter()
        .map(|tool| tool.tool_id())
        .collect();
    assert_eq!(ids, vec!["reshade", "optiscaler"]);
    assert!(super::resolve_tool("mangohud").is_none());
    assert!(super::resolve_tool("vkbasalt").is_none());
    assert!(super::resolve_tool("gamemode").is_none());
    assert!(super::resolve_tool("proton").is_none());
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn proton_is_registered() {
    let tool = super::resolve_tool("proton").expect("proton tool should resolve");
    assert_eq!(tool.display_name(), "Proton");
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn proton_launch_integration_is_exposed() {
    let tool = super::resolve_tool("proton").expect("proton tool should resolve");
    let mut config = tool.default_config();
    config.enabled = true;
    config.set("extra_env", serde_json::json!("DXVK_ASYNC=1\nPROTON_LOG=1"));
    config.set("proton_enable_hdr", serde_json::json!(true));
    config.set("radv_perftest_rt", serde_json::json!(true));
    config.set("dll_override_mode", serde_json::json!("forced"));
    config.set("forced_dll_overrides", serde_json::json!("dxgi,winmm"));

    let env = tool.env_vars(&config);
    assert!(
        env.iter()
            .any(|(key, value)| key == "DXVK_ASYNC" && value == "1")
    );
    assert!(
        env.iter()
            .any(|(key, value)| key == "PROTON_LOG" && value == "1")
    );
    assert!(
        env.iter()
            .any(|(key, value)| key == "PROTON_ENABLE_HDR" && value == "1")
    );
    assert!(
        env.iter()
            .any(|(key, value)| key == "RADV_PERFTEST" && value == "rt,emulate_rt")
    );

    let overrides = tool.wine_dll_overrides(&config);
    assert!(overrides.iter().any(|value| value == "dxgi"));
    assert!(overrides.iter().any(|value| value == "winmm"));
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn mangohud_exposes_goverlay_config_keys() {
    let tool = super::resolve_tool("mangohud").expect("mangohud tool should resolve");
    let specs = tool.settings_schema();
    for key in [
        "custom_text_center",
        "background_alpha",
        "fps_limit_method",
        "gpu_junction_temp",
        "winesync",
        "media_player",
        "upload_logs",
        "display_server",
    ] {
        assert!(
            specs.iter().any(|spec| spec.key == key),
            "missing MangoHud key {key}"
        );
    }

    let mut config = tool.default_config();
    config.set("_game_id", serde_json::json!("skyrim-se"));
    config.set("custom_text_center", serde_json::json!("modde"));
    config.set("gpu_junction_temp", serde_json::json!(true));
    let generated = tool.generate_config(&config).expect("generated config");
    assert!(generated.content.contains("custom_text_center=modde"));
    assert!(generated.content.contains("gpu_junction_temp"));
}

#[test]
fn optiscaler_release_provider_filters_installable_assets() {
    let tool: &dyn super::GameTool = &super::optiscaler::OPTISCALER;
    assert!(tool.supports_releases());
    let release = super::ToolReleaseSummary {
        tag: "v1".to_string(),
        name: None,
        published_at: None,
        assets: vec![
            super::ToolReleaseAsset {
                name: "OptiScaler.7z".to_string(),
                download_url: "https://example.test/OptiScaler.7z".to_string(),
                size: 1,
            },
            super::ToolReleaseAsset {
                name: "notes.txt".to_string(),
                download_url: "https://example.test/notes.txt".to_string(),
                size: 1,
            },
        ],
    };
    assert_eq!(
        tool.installable_release_assets(&release),
        vec!["OptiScaler.7z".to_string()]
    );
}

#[test]
#[cfg(feature = "cyberpunk")]
fn tool_context_derives_executable_dir_from_game_plugin() {
    let root = std::path::PathBuf::from("/tmp/modde-test-cyberpunk");
    let context =
        super::ToolGameContext::from_parts("cyberpunk2077", "Cyberpunk 2077", Some(root), None);
    assert!(
        context
            .executable_dir
            .expect("executable dir")
            .ends_with("bin/x64")
    );
}

#[test]
fn optiscaler_restore_commands_use_supplied_executable_dir() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let game_dir = tmp.path().join("game");
    let staging = tmp.path().join("staging");
    let mod_bin = staging.join("mods/test/bin/x64");
    fs::create_dir_all(&mod_bin).expect("mod bin");
    fs::write(mod_bin.join("winmm.dll"), b"dll").expect("dll");
    let exe_dir = game_dir.join("Binaries/Win64");
    let commands =
        super::optiscaler::fgmod_restore_commands_for_executable_dir(&game_dir, &staging, &exe_dir);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].1.ends_with("Binaries/Win64/winmm.dll"));
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn protonup_rs_install_args_are_non_interactive() {
    let args = super::proton::protonup_rs_install_args("GE-Proton10-34", "steam");
    assert_eq!(
        args,
        vec![
            "--tool",
            "GEProton",
            "--version",
            "GE-Proton10-34",
            "--for",
            "steam"
        ]
    );
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn ge_proton_release_filter_accepts_real_tags() {
    assert!(super::proton::is_ge_proton_version("GE-Proton10-34"));
    assert!(super::proton::is_ge_proton_version("Proton-GE-Proton8-32"));
    assert!(!super::proton::is_ge_proton_version(""));
    assert!(!super::proton::is_ge_proton_version("notes"));
    assert!(!super::proton::is_ge_proton_version("Proton-Experimental"));
}

#[test]
#[cfg(all(target_os = "linux", feature = "linux-integrations"))]
fn proton_version_options_preserve_catalog_order_and_dedup() {
    let options = super::proton::merge_proton_version_options(
        vec![
            "GE-Proton10-34".to_string(),
            "GE-Proton10-33".to_string(),
            "GE-Proton10-34".to_string(),
        ],
        vec!["GE-Proton10-32".to_string(), "GE-Proton10-33".to_string()],
    );
    assert_eq!(
        options,
        vec![
            "latest".to_string(),
            "GE-Proton10-34".to_string(),
            "GE-Proton10-33".to_string(),
            "GE-Proton10-32".to_string(),
        ]
    );
}

fn ensure_test_data_dir() -> std::path::PathBuf {
    static DATA_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DATA_DIR
        .get_or_init(|| {
            let default_dir = modde_core::paths::data_dir().join("modde");
            if modde_core::paths::modde_data_dir() != default_dir {
                return modde_core::paths::modde_data_dir();
            }

            let tempdir = tempfile::TempDir::new().expect("create tempdir");
            let data_dir = tempdir.path().join("data");
            std::fs::create_dir_all(&data_dir).expect("create data dir");
            // set_data_dir is idempotent (first caller wins); return whatever
            // actually became the override — ours, or a parallel test's.
            modde_core::paths::set_data_dir(data_dir);
            std::mem::forget(tempdir);
            modde_core::paths::modde_data_dir()
        })
        .clone()
}

#[tokio::test]
async fn optiscaler_install_release_from_path_extracts_local_archive() {
    let _ = ensure_test_data_dir();
    let tempdir = tempfile::TempDir::new().expect("create tempdir");
    let archive_path = tempdir.path().join("OptiScaler.zip");
    let file = std::fs::File::create(&archive_path).expect("create archive");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("OptiScaler.dll", zip::write::FileOptions::<()>::default())
        .expect("start file");
    zip.write_all(b"dll-bytes").expect("write dll");
    zip.finish().expect("finish archive");

    let tool: &dyn super::GameTool = &super::optiscaler::OPTISCALER;
    let config = tool.default_config();
    let updated = tool
        .install_release_from_path(
            "stellar-blade",
            config,
            "v1.0",
            "OptiScaler.zip",
            archive_path,
        )
        .await
        .expect("install from path");

    assert_eq!(updated.get_str("release_tag"), Some("official:v1.0"));
    assert_eq!(updated.get_str("release_asset"), Some("OptiScaler.zip"));
    assert!(
        super::optiscaler::cached_release_dir("official:v1.0")
            .join("OptiScaler.dll")
            .exists()
    );
}
