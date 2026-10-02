#![allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn detect_available() -> ToolAvailability {
    // Check common locations for OptiScaler.
    let candidates = [
        dirs::data_dir()
            .unwrap_or_default()
            .join("goverlay/fgmod/OptiScaler.dll"),
        dirs::home_dir()
            .unwrap_or_default()
            .join(".local/share/goverlay/fgmod/OptiScaler.dll"),
    ];

    for path in &candidates {
        if path.exists() {
            return ToolAvailability::Available {
                version: Some("fgmod".into()),
            };
        }
    }

    // User can provide a custom path via settings.
    ToolAvailability::Available {
        version: Some("user-provided".into()),
    }
}

pub(super) fn env_vars(config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
    let config = effective_config(config);
    let mut vars: SmallVec<[(String, String); 4]> = SmallVec::new();

    if config.get_bool("emulate_fp8")
        && config.get_str("fsr4_variant") == Some(FSR4_VARIANT_LATEST_FP8)
    {
        vars.push((
            FP8_EMULATION_ENV_KEY.to_string(),
            FP8_EMULATION_ENV_VALUE.to_string(),
        ));
    }

    // PROTON_FSR4_UPGRADE is required on Proton/Linux for FSR4 to work
    // on both RDNA3 and RDNA4 GPUs. Harmless on Windows.
    if config.get_str("fsr4_variant").is_some() {
        vars.push((
            PROTON_FSR4_ENV_KEY.to_string(),
            PROTON_FSR4_ENV_VALUE.to_string(),
        ));
    }

    vars
}

pub(super) fn wine_dll_overrides(config: &ToolConfig) -> SmallVec<[String; 4]> {
    let primary = config
        .get_str("proxy_dll")
        .or_else(|| config.get_str("dll_name"))
        .unwrap_or("dxgi.dll")
        .trim_end_matches(".dll")
        .to_string();
    let mut overrides: SmallVec<[String; 4]> = smallvec![primary];

    if config.get_bool("needs_winmm") && !overrides.iter().any(|value| value == "winmm") {
        overrides.push("winmm".into());
    }
    if let Some(raw) = config.get_str("dll_overrides") {
        for part in raw.split([',', ';', ' ', '\n', '\t']) {
            let value = part.trim().trim_end_matches(".dll");
            if !value.is_empty() && !overrides.iter().any(|existing| existing == value) {
                overrides.push(value.to_string());
            }
        }
    }

    overrides
}

pub(super) fn default_config() -> ToolConfig {
    let mut config = ToolConfig::new("optiscaler");
    config.set(
        "source_mode",
        serde_json::json!(OPTISCALER_SOURCE_GOVERLAY_FGMOD),
    );
    config.set("goverlay_channel", serde_json::json!("edge"));
    config.set("release_tag", serde_json::json!("latest"));
    config.set("release_asset", serde_json::json!(""));
    config.set("local_source_dir", serde_json::json!(""));
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));
    config.set("dll_overrides", serde_json::json!(""));
    config.set("copy_companion_files", serde_json::json!(true));
    config.set("enable_optipatcher", serde_json::json!(false));
    config.set("hardware_tuning", serde_json::json!(HARDWARE_TUNING_AUTO));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
    config.set("emulate_fp8", serde_json::json!(false));
    config.set("spoof_dlss", serde_json::json!(false));
    config.set("ini_overrides", serde_json::json!({}));
    config
}

pub(super) fn default_config_for(context: Option<&ToolGameContext>) -> ToolConfig {
    default_config_for_arch(context, crate::gpu::detect_gpu_arch())
}

pub(super) fn default_config_for_arch(
    context: Option<&ToolGameContext>,
    arch: crate::gpu::GpuArch,
) -> ToolConfig {
    let mut config = default_config();
    apply_game_defaults(&mut config, context);
    super::hardware::apply_hardware_defaults_for_arch(&mut config, arch);
    config
}

/// Shim for `dirs::data_dir()` / `dirs::home_dir()` - we use a minimal
/// vendored version to avoid adding the full `dirs` crate.
pub(super) mod dirs {
    use std::path::PathBuf;

    pub fn data_dir() -> Option<PathBuf> {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| home_dir().map(|h| h.join(".local/share")))
    }

    pub fn home_dir() -> Option<PathBuf> {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}
