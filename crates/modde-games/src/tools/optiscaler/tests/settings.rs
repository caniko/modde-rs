use super::*;

#[test]
fn goverlay_inspired_settings_expose_friendly_raw_value_selects() {
    let specs = OptiScaler.settings_schema();
    let shortcut = specs
        .iter()
        .find(|spec| spec.key == "ini_overrides.Menu.ShortcutKey")
        .expect("shortcut spec");
    assert_eq!(shortcut.section, "Menu");
    assert!(!shortcut.advanced);
    let ToolSettingKind::Select { options } = &shortcut.kind else {
        panic!("shortcut should be a select");
    };
    assert!(
        options
            .iter()
            .any(|option| option.value == "INSERT" && option.label == "Insert")
    );

    let upscaler = specs
        .iter()
        .find(|spec| spec.key == "ini_overrides.FSR.UpscalerIndex")
        .expect("FSR upscaler spec");
    assert_eq!(upscaler.section, "Basic");
    let ToolSettingKind::Select { options } = &upscaler.kind else {
        panic!("FSR upscaler should be a select");
    };
    assert_eq!(options[0].value, "auto");
    assert_eq!(options[0].label, "Auto");
    assert!(
        options
            .iter()
            .any(|option| option.value == "0" && option.label == "0 - FSR 4.0.2")
    );
    assert!(
        options
            .iter()
            .any(|option| option.value == "2" && option.label == "2 - FSR 2.3.4")
    );

    let fg = specs
        .iter()
        .find(|spec| spec.key == "ini_overrides.FSR.FGIndex")
        .expect("FSR FG spec");
    let ToolSettingKind::Select { options } = &fg.kind else {
        panic!("FSR FG should be a select");
    };
    assert!(
        options
            .iter()
            .any(|option| option.value == "1" && option.label == "1 - FSR 3.1.6")
    );
}

#[test]
fn goverlay_inspired_settings_mark_only_risky_entries_advanced() {
    let specs = OptiScaler.settings_schema();
    let spoof = specs
        .iter()
        .find(|spec| spec.key == "spoof_dlss")
        .expect("spoof fallback spec");
    assert_eq!(spoof.label, "Spoof DLSS fallback");
    assert_eq!(spoof.section, "Basic");
    assert!(!spoof.advanced);

    let optipatcher = specs
        .iter()
        .find(|spec| spec.key == "enable_optipatcher")
        .expect("OptiPatcher spec");
    assert_eq!(optipatcher.section, "Basic");
    assert!(!optipatcher.advanced);

    assert!(
        !specs
            .iter()
            .any(|spec| spec.key == "ini_overrides.Plugins.LoadAsiPlugins")
    );
}

#[test]
fn emulate_fp8_schema_is_read_only_for_int8_variant() {
    let mut config = OptiScaler.default_config();
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));

    let specs = OptiScaler.settings_schema_for(None, &config);
    let emulate = specs
        .iter()
        .find(|spec| spec.key == "emulate_fp8")
        .expect("emulate fp8 spec");

    assert_eq!(emulate.section, "Basic");
    assert!(matches!(emulate.kind, ToolSettingKind::ReadOnly));
}

#[test]
fn goverlay_channel_is_only_exposed_for_goverlay_build_source() {
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("github_release"));
    let specs = OptiScaler.settings_schema_for(None, &config);
    assert!(!specs.iter().any(|spec| spec.key == "goverlay_channel"));

    config.set("source_mode", serde_json::json!("goverlay_builds"));
    let specs = OptiScaler.settings_schema_for(None, &config);
    assert!(specs.iter().any(|spec| spec.key == "goverlay_channel"));
}

#[test]
fn fsr_backend_overrides_write_raw_ini_values() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("OptiScaler.ini");
    let dest = tmp.path().join("applied.ini");
    std::fs::write(
        &src,
        "[FSR]\nUpscalerIndex=auto\nFGIndex=auto\n[Menu]\nShortcutKey=auto\n",
    )
    .expect("source ini");
    let mut config = OptiScaler.default_config();
    config.set(
        "ini_overrides",
        serde_json::json!({
            "FSR": {
                "UpscalerIndex": "0",
                "FGIndex": "1"
            },
            "Menu": {
                "ShortcutKey": "INSERT"
            }
        }),
    );

    apply_ini_overrides_with_existing(&src, None, &dest, &config).expect("apply overrides");

    let parsed = parse_optiscaler_ini(&std::fs::read_to_string(dest).expect("dest ini"));
    assert_eq!(parsed.get("FSR.UpscalerIndex"), Some(&"0".to_string()));
    assert_eq!(parsed.get("FSR.FGIndex"), Some(&"1".to_string()));
    assert_eq!(parsed.get("Menu.ShortcutKey"), Some(&"INSERT".to_string()));
}

#[test]
fn optiscaler_high_level_controls_write_ini_values() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let src = tmp.path().join("OptiScaler.ini");
    let dest = tmp.path().join("applied.ini");
    std::fs::write(
        &src,
        "[FSR]\nFsr4Update=auto\n[Spoofing]\nDxgi=auto\n[Plugins]\nLoadAsiPlugins=auto\n",
    )
    .expect("source ini");
    let mut config = OptiScaler.default_config();
    config.set("enable_optipatcher", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));

    apply_ini_overrides_with_existing(&src, None, &dest, &config).expect("apply overrides");

    let parsed = parse_optiscaler_ini(&std::fs::read_to_string(dest).expect("dest ini"));
    assert_eq!(parsed.get("FSR.Fsr4Update"), Some(&"True".to_string()));
    assert_eq!(parsed.get("Spoofing.Dxgi"), Some(&"false".to_string()));
    assert_eq!(
        parsed.get("Plugins.LoadAsiPlugins"),
        Some(&"true".to_string())
    );
}

#[test]
fn inferred_ini_specs_use_the_key_name_not_its_section() {
    let kind = |label: &'static str| infer_optiscaler_ini_spec(format!("ini_overrides.{label}"), label).kind;
    assert!(matches!(kind("Spoofing.OverrideNvapiDll"), ToolSettingKind::TriStateBool));
    assert!(matches!(kind("Upscalers.Sharpness"), ToolSettingKind::Number { .. }));
    assert!(matches!(kind("Framegen.MotionScale"), ToolSettingKind::Number { .. }));
    assert!(matches!(kind("Foo.Bar"), ToolSettingKind::Text));
}
