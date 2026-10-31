//! vkBasalt — Vulkan post-processing layer.
//!
//! Enabled via `ENABLE_VKBASALT=1` env var. Per-game config written to
//! `~/.local/share/modde/tools/{game_id}/vkBasalt.conf` and pointed to
//! via `VKBASALT_CONFIG_FILE`.

use smallvec::{SmallVec, smallvec};

use super::{
    GameTool, GeneratedConfig, ToolAvailability, ToolCategory, ToolConfig, tool_config_dir, which,
};

pub static VKBASALT: VkBasalt = VkBasalt;

pub struct VkBasalt;

/// Implicit-layer manifests the Vulkan loader would read, given
/// `$XDG_DATA_DIRS`: fixed system paths, NixOS's driver dir, then each data dir.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn layer_paths(xdg_data_dirs: &str) -> Vec<std::path::PathBuf> {
    const LAYER: &str = "vulkan/implicit_layer.d/vkBasalt.json";
    let mut paths: Vec<std::path::PathBuf> = [
        "/usr/share/vulkan/implicit_layer.d/vkBasalt.json",
        "/etc/vulkan/implicit_layer.d/vkBasalt.json",
        "/run/opengl-driver/share/vulkan/implicit_layer.d/vkBasalt.json",
    ]
    .map(Into::into)
    .into();
    paths.extend(
        xdg_data_dirs
            .split(':')
            .filter(|dir| !dir.is_empty())
            .map(|dir| std::path::Path::new(dir).join(LAYER)),
    );
    paths
}

impl GameTool for VkBasalt {
    fn tool_id(&self) -> &'static str {
        "vkbasalt"
    }

    fn display_name(&self) -> &'static str {
        "vkBasalt"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::PostProcess
    }

    fn description(&self) -> &'static str {
        "Vulkan post-processing layer for CAS sharpening and shader effects."
    }

    fn settings_schema(&self) -> Vec<super::ToolSettingSpec> {
        vec![
            super::ToolSettingSpec::text("toggleKey", "Toggle key", "Key used to toggle vkBasalt.")
                .section("Activation"),
            super::ToolSettingSpec::bool(
                "enableOnLaunch",
                "Enable on launch",
                "Start with vkBasalt effects enabled.",
            )
            .section("Activation"),
            super::ToolSettingSpec::text(
                "effects",
                "Effects",
                "Colon or comma separated effect list, such as cas or cas:fxaa.",
            )
            .section("Effects"),
            super::ToolSettingSpec::number(
                "casSharpness",
                "CAS sharpness",
                "Contrast Adaptive Sharpening amount.",
                0.0,
                1.0,
                0.05,
            )
            .section("Effects"),
            super::ToolSettingSpec::path(
                "reshadeTexturePath",
                "ReShade texture path",
                "Optional ReShade texture directory for vkBasalt.",
            )
            .section("ReShade Paths"),
            super::ToolSettingSpec::path(
                "reshadeIncludePath",
                "ReShade shader path",
                "Optional ReShade shader include directory for vkBasalt.",
            )
            .section("ReShade Paths"),
        ]
    }

    fn detect_available(&self) -> ToolAvailability {
        #[cfg(not(target_os = "linux"))]
        {
            return ToolAvailability::NotInstalled {
                install_hint: "vkBasalt is only available on Linux".into(),
            };
        }

        // vkBasalt doesn't have a CLI binary; it's a Vulkan layer.
        // Check for the layer JSON or the library.
        #[cfg(target_os = "linux")]
        {
            let xdg = std::env::var("XDG_DATA_DIRS").unwrap_or_default();
            if layer_paths(&xdg).iter().any(|p| p.exists()) || which("vkbasalt").is_some() {
                ToolAvailability::Available { version: None }
            } else {
                // Also check via nix profile paths
                if let Ok(paths) = std::env::var("VK_LAYER_PATH") {
                    for dir in std::env::split_paths(&std::ffi::OsString::from(&paths)) {
                        if dir.join("vkBasalt.json").exists() {
                            return ToolAvailability::Available { version: None };
                        }
                    }
                }

                ToolAvailability::NotInstalled {
                    install_hint: "Install vkbasalt from your package manager".into(),
                }
            }
        }
    }

    fn env_vars(&self, config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        let mut vars = smallvec![("ENABLE_VKBASALT".into(), "1".into())];

        if let Some(game_id) = config.get_str("_game_id") {
            let conf_path = tool_config_dir(game_id).join("vkBasalt.conf");
            vars.push((
                "VKBASALT_CONFIG_FILE".into(),
                conf_path.to_string_lossy().into(),
            ));
        }

        vars
    }

    fn generate_config(&self, config: &ToolConfig) -> Option<GeneratedConfig> {
        let game_id = config.get_str("_game_id")?;

        let mut lines = Vec::new();
        lines.push("# Generated by modde — do not edit manually".to_string());

        // Toggle key
        let toggle_key = config.get_str("toggleKey").unwrap_or("Home");
        lines.push(format!("toggleKey = {toggle_key}"));

        // Enable on launch
        let enable_on_launch = config.get_bool("enableOnLaunch");
        lines.push(format!(
            "enableOnLaunch = {}",
            if enable_on_launch { "True" } else { "False" }
        ));

        // Effects
        if let Some(effects) = config.settings.get("effects").and_then(|v| v.as_array()) {
            let effect_names: Vec<&str> = effects.iter().filter_map(|v| v.as_str()).collect();
            if !effect_names.is_empty() {
                lines.push(format!("effects = {}", effect_names.join(":")));
            }
        }

        // CAS sharpness
        if let Some(sharpness) = config.settings.get("casSharpness") {
            lines.push(format!("casSharpness = {sharpness}"));
        }

        // Reshade shader/texture paths
        if let Some(path) = config.get_str("reshadeTexturePath") {
            lines.push(format!("reshadeTexturePath = {path}"));
        }
        if let Some(path) = config.get_str("reshadeIncludePath") {
            lines.push(format!("reshadeIncludePath = {path}"));
        }

        let dir = tool_config_dir(game_id);
        Some(GeneratedConfig {
            path: dir.join("vkBasalt.conf"),
            content: lines.join("\n") + "\n",
        })
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("vkbasalt");
        config.set("toggleKey", serde_json::json!("Home"));
        config.set("enableOnLaunch", serde_json::json!(true));
        config.set("effects", serde_json::json!(["cas"]));
        config.set("casSharpness", serde_json::json!(0.4));
        config
    }
}

#[cfg(test)]
mod tests {
    use super::layer_paths;
    use std::path::Path;

    #[test]
    fn layer_paths_cover_nixos_and_xdg_dirs() {
        let paths = layer_paths("/a/share::/b/share");
        for want in [
            "/usr/share/vulkan/implicit_layer.d/vkBasalt.json",
            "/etc/vulkan/implicit_layer.d/vkBasalt.json",
            "/run/opengl-driver/share/vulkan/implicit_layer.d/vkBasalt.json",
            "/a/share/vulkan/implicit_layer.d/vkBasalt.json",
            "/b/share/vulkan/implicit_layer.d/vkBasalt.json",
        ] {
            assert!(paths.iter().any(|p| p == Path::new(want)), "{want}");
        }
        assert_eq!(paths.len(), 5, "empty XDG entries are skipped");
    }
}
