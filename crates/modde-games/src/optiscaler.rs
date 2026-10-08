//! Per-game `OptiScaler` compatibility profiles.
//!
//! These profiles are intentionally game-owned metadata. They represent
//! community-tested configurations users may choose when enabling `OptiScaler`;
//! the presence of a profile must not imply `OptiScaler` is enabled by default.

pub(crate) mod loader;
pub(crate) mod spec;

use std::sync::OnceLock;

use dashmap::DashMap;
use tracing::warn;

use crate::generic::leak::slice as leak_slice;

pub use spec::{
    OptiScalerImportToml, OptiScalerProfilesSpec, serialize as serialize_optiscaler_profiles_toml,
};

/// One `OptiScaler` INI override expressed as a dotted section/key path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptiScalerIniOverride {
    pub key: &'static str,
    pub value: &'static str,
}

/// A community-tested `OptiScaler` configuration for a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
pub struct OptiScalerProfile {
    pub id: &'static str,
    pub name: &'static str,
    pub source_url: &'static str,
    pub tested_optiscaler_version: &'static str,
    pub source_mode: Option<&'static str>,
    pub goverlay_channel: Option<&'static str>,
    pub proxy_dll: &'static str,
    pub release_tag: Option<&'static str>,
    pub release_asset: Option<&'static str>,
    pub wine_dll_overrides: &'static [&'static str],
    pub copy_companion_files: bool,
    pub enable_optipatcher: bool,
    pub fsr4_variant: Option<&'static str>,
    pub emulate_fp8: bool,
    pub spoof_dlss: bool,
    pub ini_overrides: &'static [OptiScalerIniOverride],
    pub notes: &'static str,
}

static MERGED_PROFILE_CACHE: OnceLock<DashMap<String, &'static [OptiScalerProfile]>> =
    OnceLock::new();

fn merged_profile_cache() -> &'static DashMap<String, &'static [OptiScalerProfile]> {
    MERGED_PROFILE_CACHE.get_or_init(DashMap::new)
}

/// Resolve community-tested `OptiScaler` profiles for a supported game.
#[must_use]
pub fn resolve_optiscaler_profiles(game_id: &str) -> &'static [OptiScalerProfile] {
    let built_in = crate::registry::resolve_game(game_id)
        .map_or(&[] as &'static [OptiScalerProfile], |game| {
            game.optiscaler_profiles
        });
    let user = loader::load_user_profiles(game_id);

    if user.is_empty() {
        return built_in;
    }

    if built_in.is_empty() {
        return user;
    }

    if let Some(profiles) = merged_profile_cache().get(game_id) {
        return *profiles;
    }

    let mut merged = built_in.to_vec();
    for profile in user {
        if let Some(existing_index) = merged.iter().position(|existing| existing.id == profile.id) {
            warn!(
                game_id,
                profile_id = profile.id,
                "user OptiScaler profile overrides built-in profile"
            );
            merged[existing_index] = *profile;
        } else {
            merged.push(*profile);
        }
    }

    *merged_profile_cache()
        .entry(game_id.to_string())
        .or_insert_with(move || leak_slice(merged))
}

/// Return the default community-tested profile for a game, if any.
#[must_use]
pub fn default_optiscaler_profile(game_id: &str) -> Option<&'static OptiScalerProfile> {
    resolve_optiscaler_profiles(game_id).first()
}

#[cfg(test)]
mod tests {
    use super::{default_optiscaler_profile, resolve_optiscaler_profiles};
    use std::sync::OnceLock;

    fn shared_data_dir() -> &'static std::path::PathBuf {
        static DATA_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
        DATA_DIR.get_or_init(|| {
            let tempdir = tempfile::TempDir::new().expect("create tempdir");
            let data_dir = tempdir.path().join("data");
            std::fs::create_dir_all(data_dir.join("games")).expect("create games dir");
            modde_core::paths::set_data_dir(data_dir.clone());
            std::mem::forget(tempdir);
            data_dir
        })
    }

    #[test]
    #[cfg(feature = "ue4")]
    fn stellar_blade_resolves_community_optiscaler_profile() {
        let _ = shared_data_dir();
        let profiles = resolve_optiscaler_profiles("stellar-blade");

        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "community-dxgi");
        assert_eq!(profiles[0].proxy_dll, "dxgi.dll");
        assert_eq!(profiles[0].tested_optiscaler_version, "0.9");
        assert_eq!(profiles[0].source_mode, Some("github_release"));
        assert_eq!(profiles[0].goverlay_channel, None);
        assert!(profiles[0].enable_optipatcher);
        assert_eq!(profiles[0].fsr4_variant, Some("latest_fp8"));
        assert!(!profiles[0].emulate_fp8);
        assert!(!profiles[0].spoof_dlss);
        assert_eq!(
            default_optiscaler_profile("stellar-blade").map(|profile| profile.id),
            Some("community-dxgi")
        );
    }

    #[test]
    fn unsupported_optiscaler_profiles_resolve_empty() {
        let _ = shared_data_dir();
        assert_eq!(resolve_optiscaler_profiles("skyrim-se"), []);
        assert!(default_optiscaler_profile("skyrim-se").is_none());
    }

    #[test]
    fn missing_sidecar_returns_built_in_unchanged() {
        let _ = shared_data_dir();
        let first = resolve_optiscaler_profiles("stellar-blade");
        let second = resolve_optiscaler_profiles("stellar-blade");

        assert_eq!(first.len(), usize::from(cfg!(feature = "ue4")));
        assert!(std::ptr::eq(first, second));
    }
}
