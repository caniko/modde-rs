//! Proton / launcher integration exposed as a per-game tool.
//!
//! This does not install Proton. It stores game-specific launch integration
//! preferences that participate in modde's existing env-var, DLL override, and
//! wrapper collection pipeline.

use std::path::PathBuf;
use std::process::Command;

use smallvec::SmallVec;

use super::{GameTool, ToolAvailability, ToolCategory, ToolConfig, ToolGameContext, which};

pub static PROTON: Proton = Proton;
const GE_PROTON_REPO: &str = "GloriousEggroll/proton-ge-custom";

pub struct Proton;

impl GameTool for Proton {
    fn tool_id(&self) -> &'static str {
        "proton"
    }

    fn display_name(&self) -> &'static str {
        "Proton"
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Performance
    }

    fn description(&self) -> &'static str {
        "Per-game Proton and launcher compatibility settings used by modde launch wrappers."
    }

    fn settings_schema(&self) -> Vec<super::ToolSettingSpec> {
        let config = self.default_config();
        self.settings_schema_for(None, &config)
    }

    fn settings_schema_for(
        &self,
        _context: Option<&ToolGameContext>,
        _config: &ToolConfig,
    ) -> Vec<super::ToolSettingSpec> {
        let mut specs = vec![
            super::ToolSettingSpec::select(
                "version_mode",
                "Version mode",
                "How modde should choose the Proton runner for this game.",
                &[
                    "launcher_default",
                    "installed_version",
                    "install_with_protonup_rs",
                ],
            ),
            super::ToolSettingSpec::select(
                "selected_version",
                "Proton version",
                "Installed or requested GEProton version.",
                &proton_version_options()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
            ),
            super::ToolSettingSpec::select(
                "install_target",
                "Install target",
                "Target application passed to protonup-rs.",
                &["steam"],
            ),
            super::ToolSettingSpec::read_only(
                "derived_launcher",
                "Launcher",
                "Derived from the selected game.",
            ),
            super::ToolSettingSpec::read_only(
                "derived_steam_app_id",
                "Steam app id",
                "Detected from launcher metadata when available.",
            ),
            super::ToolSettingSpec::path(
                "prefix_path_override",
                "Prefix override",
                "Optional Proton/Wine prefix override. Leave blank to use launcher detection.",
            ),
            super::ToolSettingSpec::text(
                "extra_env",
                "Extra environment",
                "Additional KEY=VALUE lines exported when launching the game.",
            ),
            super::ToolSettingSpec::bool("steamdeck", "Steam Deck mode", "Export SteamDeck=1."),
            super::ToolSettingSpec::bool(
                "proton_enable_hdr",
                "Proton HDR",
                "Export PROTON_ENABLE_HDR=1.",
            ),
            super::ToolSettingSpec::bool("enable_hdr_wsi", "HDR WSI", "Export ENABLE_HDR_WSI=1."),
            super::ToolSettingSpec::bool(
                "proton_enable_wayland",
                "Proton Wayland",
                "Export PROTON_ENABLE_WAYLAND=1.",
            ),
            super::ToolSettingSpec::bool("proton_log", "Proton log", "Export PROTON_LOG=1."),
            super::ToolSettingSpec::bool(
                "proton_use_sdl",
                "Proton SDL",
                "Export PROTON_USE_SDL=1.",
            ),
            super::ToolSettingSpec::bool(
                "radv_perftest_rt",
                "RADV RT",
                "Export RADV_PERFTEST=rt,emulate_rt.",
            ),
            super::ToolSettingSpec::bool(
                "proton_hide_nvidia_gpu",
                "Hide NVIDIA GPU",
                "Export PROTON_HIDE_NVIDIA_GPU=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_enable_nvapi",
                "Enable NVAPI",
                "Export PROTON_ENABLE_NVAPI=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_use_wined3d",
                "Use WINED3D",
                "Export PROTON_USE_WINED3D=1.",
            ),
            super::ToolSettingSpec::bool(
                "mesa_loader_zink",
                "Mesa Zink",
                "Export MESA_LOADER_DRIVER_OVERRIDE=zink.",
            ),
            super::ToolSettingSpec::bool(
                "glx_vendor_mesa",
                "GLX Mesa",
                "Export __GLX_VENDOR_LIBRARY_NAME=mesa.",
            ),
            super::ToolSettingSpec::bool(
                "radv_debug_nofastclears",
                "RADV no fast clears",
                "Export RADV_DEBUG=nofastclears.",
            ),
            super::ToolSettingSpec::bool(
                "proton_fsr4_upgrade",
                "FSR4 upgrade",
                "Export PROTON_FSR4_UPGRADE=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_dlss_upgrade",
                "DLSS upgrade",
                "Export PROTON_DLSS_UPGRADE=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_xess_upgrade",
                "XeSS upgrade",
                "Export PROTON_XESS_UPGRADE=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_priority_high",
                "High priority",
                "Export PROTON_PRIORITY_HIGH=1.",
            ),
            super::ToolSettingSpec::bool("proton_use_wow64", "WOW64", "Export PROTON_USE_WOW64=1."),
            super::ToolSettingSpec::bool(
                "proton_force_large_address_aware",
                "Large address aware",
                "Export PROTON_FORCE_LARGE_ADDRESS_AWARE=1.",
            ),
            super::ToolSettingSpec::bool(
                "staging_shared_memory",
                "Shared memory",
                "Export STAGING_SHARED_MEMORY=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_no_ntsync",
                "Disable NTSYNC",
                "Export PROTON_NO_NTSYNC=1.",
            ),
            super::ToolSettingSpec::bool(
                "proton_heap_delay_free",
                "Heap delay free",
                "Export PROTON_HEAP_DELAY_FREE=1.",
            ),
            super::ToolSettingSpec::bool(
                "enable_mesa_antilag",
                "Mesa Anti-Lag",
                "Export ENABLE_LAYER_MESA_ANTI_LAG=1.",
            ),
            super::ToolSettingSpec::select(
                "dll_override_mode",
                "DLL override mode",
                "How Proton should contribute forced DLL overrides.",
                &["auto", "forced", "off"],
            ),
            super::ToolSettingSpec::text(
                "forced_dll_overrides",
                "Forced DLL overrides",
                "Comma or whitespace separated DLL base names, such as dxgi, winmm.",
            ),
            super::ToolSettingSpec::select(
                "wrapper_order",
                "Wrapper order",
                "Where Proton-specific wrapper integration should appear in the launch chain.",
                &["after-modde", "before-tools"],
            ),
        ];
        for spec in &mut specs {
            spec.section = proton_setting_section(&spec.key);
        }
        specs
    }

    fn detect_available(&self) -> ToolAvailability {
        if let Some(path) = detect_protonup_rs() {
            let count = installed_ge_proton_versions().len();
            ToolAvailability::Available {
                version: Some(format!(
                    "protonup-rs at {}; {count} GEProton install(s)",
                    path.display()
                )),
            }
        } else {
            ToolAvailability::NotInstalled {
                install_hint: "Install protonup-rs to manage GEProton versions; launcher/default settings still work.".into(),
            }
        }
    }

    fn env_vars(&self, config: &ToolConfig) -> SmallVec<[(String, String); 4]> {
        let mut vars = SmallVec::new();

        // `default_config` stores an empty override, which must not mask `prefix_path`.
        if let Some(prefix) = config
            .get_str("prefix_path_override")
            .filter(|prefix| !prefix.trim().is_empty())
            .or_else(|| config.get_str("prefix_path"))
            && !prefix.trim().is_empty()
        {
            vars.push(("WINEPREFIX".into(), prefix.trim().into()));
        }

        if let Some(extra) = config.get_str("extra_env") {
            for line in extra.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    if !key.is_empty() {
                        vars.push((key.into(), value.trim().into()));
                    }
                }
            }
        }

        for (setting, key, value) in GOVERLAY_ENV_TOGGLES {
            if config.get_bool(setting) {
                vars.push(((*key).into(), (*value).into()));
            }
        }

        vars
    }

    fn wine_dll_overrides(&self, config: &ToolConfig) -> SmallVec<[String; 4]> {
        if config.get_str("dll_override_mode") == Some("off") {
            return SmallVec::new();
        }
        let Some(raw) = config.get_str("forced_dll_overrides") else {
            return SmallVec::new();
        };

        raw.split([',', ';', ' ', '\n', '\t'])
            .filter_map(|part| {
                let value = part.trim().trim_end_matches(".dll");
                (!value.is_empty()).then(|| value.to_string())
            })
            .collect()
    }

    fn default_config(&self) -> ToolConfig {
        let mut config = ToolConfig::new("proton");
        config.set("version_mode", serde_json::json!("launcher_default"));
        config.set("selected_version", serde_json::json!("latest"));
        config.set("install_target", serde_json::json!("steam"));
        config.set("prefix_path_override", serde_json::json!(""));
        config.set("extra_env", serde_json::json!(""));
        for (setting, _, _) in GOVERLAY_ENV_TOGGLES {
            config.set(*setting, serde_json::json!(false));
        }
        config.set("dll_override_mode", serde_json::json!("auto"));
        config.set("forced_dll_overrides", serde_json::json!(""));
        config.set("wrapper_order", serde_json::json!("after-modde"));
        config
    }
}

const GOVERLAY_ENV_TOGGLES: &[(&str, &str, &str)] = &[
    ("steamdeck", "SteamDeck", "1"),
    ("proton_enable_hdr", "PROTON_ENABLE_HDR", "1"),
    ("enable_hdr_wsi", "ENABLE_HDR_WSI", "1"),
    ("proton_enable_wayland", "PROTON_ENABLE_WAYLAND", "1"),
    ("proton_log", "PROTON_LOG", "1"),
    ("proton_use_sdl", "PROTON_USE_SDL", "1"),
    ("radv_perftest_rt", "RADV_PERFTEST", "rt,emulate_rt"),
    ("proton_hide_nvidia_gpu", "PROTON_HIDE_NVIDIA_GPU", "1"),
    ("proton_enable_nvapi", "PROTON_ENABLE_NVAPI", "1"),
    ("proton_use_wined3d", "PROTON_USE_WINED3D", "1"),
    ("mesa_loader_zink", "MESA_LOADER_DRIVER_OVERRIDE", "zink"),
    ("glx_vendor_mesa", "__GLX_VENDOR_LIBRARY_NAME", "mesa"),
    ("radv_debug_nofastclears", "RADV_DEBUG", "nofastclears"),
    ("proton_fsr4_upgrade", "PROTON_FSR4_UPGRADE", "1"),
    ("proton_dlss_upgrade", "PROTON_DLSS_UPGRADE", "1"),
    ("proton_xess_upgrade", "PROTON_XESS_UPGRADE", "1"),
    ("proton_priority_high", "PROTON_PRIORITY_HIGH", "1"),
    ("proton_use_wow64", "PROTON_USE_WOW64", "1"),
    (
        "proton_force_large_address_aware",
        "PROTON_FORCE_LARGE_ADDRESS_AWARE",
        "1",
    ),
    ("staging_shared_memory", "STAGING_SHARED_MEMORY", "1"),
    ("proton_no_ntsync", "PROTON_NO_NTSYNC", "1"),
    ("proton_heap_delay_free", "PROTON_HEAP_DELAY_FREE", "1"),
    ("enable_mesa_antilag", "ENABLE_LAYER_MESA_ANTI_LAG", "1"),
];

fn proton_setting_section(key: &str) -> &'static str {
    match key {
        "version_mode" | "selected_version" | "install_target" => "Runner",
        "derived_launcher" | "derived_steam_app_id" => "Detected Game",
        "prefix_path_override" | "extra_env" => "Environment",
        "dll_override_mode" | "forced_dll_overrides" => "DLL Overrides",
        "wrapper_order" => "Wrapper",
        _ => "Compatibility Toggles",
    }
}

#[must_use]
pub fn detect_protonup_rs() -> Option<PathBuf> {
    which("protonup-rs")
}

#[must_use]
pub fn compatibilitytools_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut dirs = Vec::new();
    if let Some(home) = &home {
        dirs.push(home.join(".steam/root/compatibilitytools.d"));
        dirs.push(home.join(".local/share/Steam/compatibilitytools.d"));
        dirs.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam/compatibilitytools.d"));
    }
    dirs.retain(|d| d.is_dir());
    dirs
}

#[must_use]
pub fn installed_ge_proton_versions() -> Vec<String> {
    let mut versions = Vec::new();
    for dir in compatibilitytools_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("GE-Proton") || name.starts_with("Proton-GE") {
                versions.push(name);
            }
        }
    }
    versions.sort();
    versions.dedup();
    versions
}

#[must_use]
pub fn proton_version_options() -> Vec<String> {
    merge_proton_version_options(std::iter::empty(), installed_ge_proton_versions())
}

#[must_use]
pub fn is_ge_proton_version(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && (trimmed.starts_with("GE-Proton") || trimmed.starts_with("Proton-GE"))
}

#[must_use]
pub fn merge_proton_version_options<C, I>(catalog_versions: C, installed_versions: I) -> Vec<String>
where
    C: IntoIterator<Item = String>,
    I: IntoIterator<Item = String>,
{
    super::release::prepend_latest_dedup(catalog_versions.into_iter().chain(installed_versions))
}

pub async fn list_ge_proton_versions() -> anyhow::Result<Vec<String>> {
    let releases = super::release::list_github_releases(GE_PROTON_REPO).await?;
    let catalog_versions = releases
        .into_iter()
        .map(|release| release.tag)
        .filter(|tag| is_ge_proton_version(tag))
        .collect::<Vec<_>>();
    Ok(merge_proton_version_options(
        catalog_versions,
        installed_ge_proton_versions(),
    ))
}

#[must_use]
pub fn protonup_rs_install_args(version: &str, target: &str) -> Vec<String> {
    vec![
        "--tool".to_string(),
        "GEProton".to_string(),
        "--version".to_string(),
        version.to_string(),
        "--for".to_string(),
        target.to_string(),
    ]
}

pub fn install_ge_proton_with_protonup_rs(version: &str, target: &str) -> anyhow::Result<()> {
    let Some(binary) = detect_protonup_rs() else {
        anyhow::bail!("protonup-rs is not installed or not on PATH");
    };
    let status = Command::new(binary)
        .args(protonup_rs_install_args(version, target))
        .status()?;
    if !status.success() {
        anyhow::bail!("protonup-rs failed with status {status}");
    }
    Ok(())
}
