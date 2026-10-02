use super::*;

/// Verifies that `default_config_for` applies the default community profile and
/// leaves GPU-specific FSR4 selection to the hardware tuning layer.
#[test]
#[cfg(feature = "ue4")]
fn stellar_blade_default_config_adds_default_profile_and_hardware_tuning() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let config =
        super::super::runtime::default_config_for_arch(Some(&context), crate::gpu::GpuArch::RDNA3);

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));

    // Common profile fields
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://github.com/optiscaler/OptiScaler/wiki/Stellar-Blade")
    );

    // Operational settings applied from the selected profile
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert!(config.get_bool("copy_companion_files"));
    assert!(config.get_bool("enable_optipatcher"));
    assert!(!config.get_bool("spoof_dlss"));

    // Hardware tuning is applied after profile defaults. Inject the architecture
    // so the test also runs on headless builders and inside Nix sandboxes.
    assert_eq!(config.get_str("hardware_tuning"), Some("auto"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(!config.get_bool("emulate_fp8"));

    for arch in [crate::gpu::GpuArch::RDNA4, crate::gpu::GpuArch::Unknown] {
        let config = super::super::runtime::default_config_for_arch(Some(&context), arch);
        assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
        assert_eq!(config.get_str("hardware_tuning"), Some("auto"));
        assert_eq!(
            config.get_str("fsr4_variant"),
            Some(FSR4_VARIANT_LATEST_FP8)
        );
        assert!(!config.get_bool("emulate_fp8"));
    }

    // Release tag and ini overrides
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.1"));
    assert_eq!(
        config.settings.get("ini_overrides"),
        Some(&serde_json::json!({}))
    );
}

#[test]
fn games_without_optiscaler_profiles_keep_generic_defaults() {
    let context = ToolGameContext::from_parts("skyrim-se", "Skyrim Special Edition", None, None);
    let config = OptiScaler.default_config_for(Some(&context));

    assert_eq!(config.get_str("source_mode"), Some("goverlay_fgmod"));
    assert_eq!(config.get_str("release_tag"), Some("latest"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some(""));
    assert_eq!(config.get_str("optiscaler_profile"), None);
}

#[test]
fn custom_profile_opt_out_prevents_community_defaults() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();
    config.set("optiscaler_profile", serde_json::json!("custom"));
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set("release_tag", serde_json::json!("latest"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));

    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(config.get_str("optiscaler_profile"), Some("custom"));
    assert_eq!(config.get_str("source_mode"), Some("local_dir"));
    assert_eq!(config.get_str("release_tag"), Some("latest"));
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("tested_optiscaler_version"), None);
}

#[test]
#[cfg(feature = "ue4")]
fn stale_stellar_blade_rdna3_profile_marker_falls_back_to_default_profile() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();
    config.set(
        "optiscaler_profile",
        serde_json::json!("community-dxgi-rdna3"),
    );

    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
}

#[test]
#[cfg(feature = "ue4")]
fn stellar_blade_customize_after_profile_application_preserves_manual_settings() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();

    // First, apply profile defaults
    apply_game_defaults(&mut config, Some(&context));

    let profile_id = config
        .get_str("optiscaler_profile")
        .unwrap_or("")
        .to_string();
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));

    // Now manually override on top of profile settings
    config.set("source_mode", serde_json::json!("goverlay_builds"));
    config.set("goverlay_channel", serde_json::json!("edge"));
    config.set(
        "release_tag",
        serde_json::json!("goverlay-edge:edge-0.9.12.0323"),
    );
    config.set("release_asset", serde_json::json!("optiscaler-edge.7z"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));
    config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
    config.set("copy_companion_files", serde_json::json!(false));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(true));
    config.set(
        "ini_overrides",
        serde_json::json!({"Spoofing": {"Dxgi": "false"}}),
    );

    // Re-applying defaults should preserve manual overrides (profile already selected)
    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(
        config.get_str("optiscaler_profile"),
        Some(profile_id.as_str())
    );
    assert_eq!(config.get_str("source_mode"), Some("goverlay_builds"));
    assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
    assert_eq!(
        config.get_str("release_tag"),
        Some("goverlay-edge:edge-0.9.12.0323")
    );
    assert_eq!(config.get_str("release_asset"), Some("optiscaler-edge.7z"));
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
    assert!(!config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(config.get_bool("emulate_fp8"));
    assert!(config.get_bool("spoof_dlss"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/Spoofing/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("false")
    );
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
}

#[test]
#[cfg(feature = "ue4")]
fn stellar_blade_with_explicit_profile_marker_can_preserve_custom_release() {
    let context = ToolGameContext::from_parts(
        "stellar-blade",
        "Stellar Blade",
        Some(PathBuf::from("/fake/StellarBlade")),
        None,
    );
    let mut config = OptiScaler.default_config();

    // First call applies the default community profile.
    apply_game_defaults(&mut config, Some(&context));
    let profile_id = config
        .get_str("optiscaler_profile")
        .unwrap_or("")
        .to_string();

    // Profile's release settings are applied
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert!(config.get_bool("enable_optipatcher"));

    // Now apply a custom release on top — the profile is already selected,
    // so re-applying defaults should preserve our manual release settings
    config.set("source_mode", serde_json::json!("github_release"));
    config.set("release_tag", serde_json::json!("official:v0.9.11"));
    config.set("release_asset", serde_json::json!("OptiScaler_0.9.11.7z"));
    config.set("enable_optipatcher", serde_json::json!(false));

    apply_game_defaults(&mut config, Some(&context));

    assert_eq!(
        config.get_str("optiscaler_profile"),
        Some(profile_id.as_str())
    );
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.11"));
    assert_eq!(
        config.get_str("release_asset"),
        Some("OptiScaler_0.9.11.7z")
    );
    assert!(!config.get_bool("enable_optipatcher"));
}

#[test]
#[cfg(feature = "ue4")]
fn selecting_community_profile_applies_all_community_settings() {
    let mut config = OptiScaler.default_config();

    assert!(apply_profile_by_id(
        &mut config,
        "stellar-blade",
        "community-dxgi"
    ));

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.1"));
    assert!(config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    assert_eq!(
        config.get_str("fsr4_variant"),
        Some(FSR4_VARIANT_LATEST_FP8)
    );
    assert!(!config.get_bool("emulate_fp8"));
    assert!(!config.get_bool("spoof_dlss"));
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://github.com/optiscaler/OptiScaler/wiki/Stellar-Blade")
    );
}

#[test]
#[cfg(feature = "ue4")]
fn custom_profile_then_community_profile_overwrites_most_settings() {
    let mut config = OptiScaler.default_config();
    assert!(apply_profile_by_id(&mut config, "stellar-blade", "custom"));
    assert_eq!(config.get_str("optiscaler_profile"), Some("custom"));
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set("release_tag", serde_json::json!("custom-build"));
    config.set("release_asset", serde_json::json!("CustomOptiScaler.7z"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));
    config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
    config.set("copy_companion_files", serde_json::json!(false));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(true));

    assert!(apply_profile_by_id(
        &mut config,
        "stellar-blade",
        "community-dxgi"
    ));

    // Community profile settings overwrite profile-specified settings
    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.1"));
    assert_eq!(config.get_str("proxy_dll"), Some("dxgi.dll"));
    // dll_overrides not cleared by community profile (which has empty overrides)
    assert!(config.get_bool("copy_companion_files"));
    assert!(config.get_bool("enable_optipatcher"));
    assert_eq!(
        config.get_str("fsr4_variant"),
        Some(FSR4_VARIANT_LATEST_FP8)
    );
    assert!(!config.get_bool("emulate_fp8"));
    assert!(!config.get_bool("spoof_dlss"));
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("0.9"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://github.com/optiscaler/OptiScaler/wiki/Stellar-Blade")
    );
}

#[test]
fn optiscaler_profile_applies_metadata_and_operational_settings() {
    let profile = crate::optiscaler::OptiScalerProfile {
        id: "test-profile",
        name: "Test Profile",
        source_url: "https://example.test/profile",
        tested_optiscaler_version: "1.2.3",
        source_mode: Some("github_release"),
        goverlay_channel: None,
        proxy_dll: "winmm.dll",
        release_tag: Some("v1.2.3"),
        release_asset: Some("OptiScaler.7z"),
        wine_dll_overrides: &["winmm", "nvngx"],
        copy_companion_files: false,
        enable_optipatcher: true,
        fsr4_variant: Some(FSR4_VARIANT_INT8_402),
        emulate_fp8: true,
        spoof_dlss: true,
        ini_overrides: &[crate::optiscaler::OptiScalerIniOverride {
            key: "Spoofing.Dxgi",
            value: "false",
        }],
        notes: "Test notes",
    };
    let mut config = OptiScaler.default_config();
    // Apply profile — it should set both metadata AND operational settings
    apply_optiscaler_profile_metadata(&mut config, &profile);

    // Metadata fields
    assert_eq!(config.get_str("optiscaler_profile"), Some("test-profile"));
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("1.2.3"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://example.test/profile")
    );

    // Operational settings from profile
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("v1.2.3"));
    assert_eq!(config.get_str("release_asset"), Some("OptiScaler.7z"));
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert!(!config.get_bool("copy_companion_files"));
    assert!(config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(config.get_bool("emulate_fp8"));
    assert!(config.get_bool("spoof_dlss"));
    assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/Spoofing/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("false")
    );
}

#[test]
fn optiscaler_second_profile_application_preserves_manual_overrides() {
    let profile = crate::optiscaler::OptiScalerProfile {
        id: "test-profile",
        name: "Test Profile",
        source_url: "https://example.test/profile",
        tested_optiscaler_version: "1.2.3",
        source_mode: Some("github_release"),
        goverlay_channel: None,
        proxy_dll: "winmm.dll",
        release_tag: Some("v1.2.3"),
        release_asset: Some("OptiScaler.7z"),
        wine_dll_overrides: &["winmm", "nvngx"],
        copy_companion_files: false,
        enable_optipatcher: true,
        fsr4_variant: Some(FSR4_VARIANT_INT8_402),
        emulate_fp8: true,
        spoof_dlss: true,
        ini_overrides: &[crate::optiscaler::OptiScalerIniOverride {
            key: "Spoofing.Dxgi",
            value: "false",
        }],
        notes: "Test notes",
    };
    let mut config = OptiScaler.default_config();

    // First application: profile sets all operational settings
    apply_optiscaler_profile_metadata(&mut config, &profile);

    // User manually overrides on top of profile
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set("release_tag", serde_json::json!("custom-build"));
    config.set("release_asset", serde_json::json!("CustomOptiScaler.7z"));
    config.set("proxy_dll", serde_json::json!("version.dll"));
    config.set("dll_overrides", serde_json::json!("dxgi"));
    config.set("copy_companion_files", serde_json::json!(true));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
    config.set("emulate_fp8", serde_json::json!(false));
    config.set("spoof_dlss", serde_json::json!(false));
    config.set(
        "ini_overrides",
        serde_json::json!({"OptiScaler": {"Dxgi": "auto"}}),
    );

    // Second application with SAME profile: should preserve manual overrides
    apply_optiscaler_profile_metadata(&mut config, &profile);

    assert_eq!(config.get_str("optiscaler_profile"), Some("test-profile"));
    assert_eq!(config.get_str("source_mode"), Some("local_dir"));
    assert_eq!(config.get_str("release_tag"), Some("custom-build"));
    assert_eq!(config.get_str("release_asset"), Some("CustomOptiScaler.7z"));
    assert_eq!(config.get_str("proxy_dll"), Some("version.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("dxgi"));
    assert!(config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(
        config.get_str("fsr4_variant"),
        Some(FSR4_VARIANT_LATEST_FP8)
    );
    assert!(!config.get_bool("emulate_fp8"));
    assert!(!config.get_bool("spoof_dlss"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/OptiScaler/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("auto")
    );
    assert_eq!(config.get_str("tested_optiscaler_version"), Some("1.2.3"));
    assert_eq!(
        config.get_str("optiscaler_profile_source_url"),
        Some("https://example.test/profile")
    );
}

#[test]
fn optiscaler_release_selection_overwrites_old_release_settings() {
    let mut config = OptiScaler.default_config();
    config.set("optiscaler_profile", serde_json::json!("community-dxgi"));
    config.set("proxy_dll", serde_json::json!("winmm.dll"));
    config.set("dll_overrides", serde_json::json!("winmm,nvngx"));
    config.set("copy_companion_files", serde_json::json!(false));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));
    config.set("spoof_dlss", serde_json::json!(true));
    config.set(
        "ini_overrides",
        serde_json::json!({"Spoofing": {"Dxgi": "false"}}),
    );

    apply_optiscaler_release_selection(&mut config, "official:v0.9.11", "OptiScaler_0.9.11.7z");

    assert_eq!(config.get_str("optiscaler_profile"), Some("community-dxgi"));
    assert_eq!(config.get_str("source_mode"), Some("github_release"));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.11"));
    assert_eq!(
        config.get_str("release_asset"),
        Some("OptiScaler_0.9.11.7z")
    );
    // Operational settings are NOT part of release selection — release_selection
    // only changes source_mode, release_tag, and release_asset
    assert_eq!(config.get_str("proxy_dll"), Some("winmm.dll"));
    assert_eq!(config.get_str("dll_overrides"), Some("winmm,nvngx"));
    assert!(!config.get_bool("copy_companion_files"));
    assert!(!config.get_bool("enable_optipatcher"));
    assert_eq!(config.get_str("fsr4_variant"), Some(FSR4_VARIANT_INT8_402));
    assert!(config.get_bool("emulate_fp8"));
    assert!(config.get_bool("spoof_dlss"));
    assert_eq!(
        config
            .settings
            .pointer("/ini_overrides/Spoofing/Dxgi")
            .and_then(serde_json::Value::as_str),
        Some("false")
    );
}
