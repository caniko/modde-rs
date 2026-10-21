//! Serde spec types for the `OptiScaler` profile TOML format, covering both the
//! owned deserialization shapes used when reading profiles and the borrowed
//! serialization shapes used by [`serialize`] when exporting them.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct OptiScalerProfileSpec {
    pub id: String,
    pub name: String,
    pub source_url: String,
    pub tested_optiscaler_version: String,
    pub source_mode: Option<String>,
    pub goverlay_channel: Option<String>,
    pub proxy_dll: String,
    pub release_tag: Option<String>,
    pub release_asset: Option<String>,
    #[serde(default)]
    pub wine_dll_overrides: Vec<String>,
    #[serde(default)]
    pub copy_companion_files: bool,
    #[serde(default)]
    pub enable_optipatcher: bool,
    pub fsr4_variant: Option<String>,
    #[serde(default)]
    pub emulate_fp8: bool,
    #[serde(default)]
    pub spoof_dlss: bool,
    #[serde(default)]
    pub ini_overrides: Vec<IniOverrideSpec>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct IniOverrideSpec {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct OptiScalerProfilesSpec {
    #[serde(default)]
    pub profile: Vec<OptiScalerProfileSpec>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OptiScalerImportToml {
    pub optiscaler: OptiScalerProfilesSpec,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct IniOverrideToml<'a> {
    pub key: &'a str,
    pub value: &'a str,
}

#[derive(Debug, Clone, Serialize)]
#[allow(clippy::struct_excessive_bools)]
pub(crate) struct OptiScalerProfileToml<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub source_url: &'a str,
    pub tested_optiscaler_version: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_mode: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub goverlay_channel: Option<&'a str>,
    pub proxy_dll: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_tag: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_asset: Option<&'a str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wine_dll_overrides: Vec<&'a str>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub copy_companion_files: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub enable_optipatcher: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fsr4_variant: Option<&'a str>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub emulate_fp8: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub spoof_dlss: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ini_overrides: Vec<IniOverrideToml<'a>>,
    #[serde(default, skip_serializing_if = "str::is_empty")]
    pub notes: &'a str,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OptiScalerProfilesToml<'a> {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<OptiScalerProfileToml<'a>>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct OptiScalerExportToml<'a> {
    pub optiscaler: OptiScalerProfilesToml<'a>,
}

pub fn serialize(profiles: &[crate::optiscaler::OptiScalerProfile]) -> anyhow::Result<String> {
    let toml = OptiScalerExportToml {
        optiscaler: OptiScalerProfilesToml {
            profile: profiles
                .iter()
                .map(|profile| OptiScalerProfileToml {
                    id: profile.id,
                    name: profile.name,
                    source_url: profile.source_url,
                    tested_optiscaler_version: profile.tested_optiscaler_version,
                    source_mode: profile.source_mode,
                    goverlay_channel: profile.goverlay_channel,
                    proxy_dll: profile.proxy_dll,
                    release_tag: profile.release_tag,
                    release_asset: profile.release_asset,
                    wine_dll_overrides: profile.wine_dll_overrides.to_vec(),
                    copy_companion_files: profile.copy_companion_files,
                    enable_optipatcher: profile.enable_optipatcher,
                    fsr4_variant: profile.fsr4_variant,
                    emulate_fp8: profile.emulate_fp8,
                    spoof_dlss: profile.spoof_dlss,
                    ini_overrides: profile
                        .ini_overrides
                        .iter()
                        .map(|override_| IniOverrideToml {
                            key: override_.key,
                            value: override_.value,
                        })
                        .collect(),
                    notes: profile.notes,
                })
                .collect(),
        },
    };
    let body = toml::to_string_pretty(&toml)?;
    // Keep an explicit header; toml only emits one itself for an empty profile list.
    if body.starts_with("[optiscaler]") {
        Ok(body)
    } else {
        Ok(format!("[optiscaler]\n{body}"))
    }
}
