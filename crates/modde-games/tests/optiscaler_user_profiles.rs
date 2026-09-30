use std::sync::OnceLock;

use modde_games::{default_optiscaler_profile, resolve_game_plugin, resolve_optiscaler_profiles};

fn shared_data_dir() -> &'static std::path::PathBuf {
    static DATA_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        let tempdir = tempfile::TempDir::new().expect("create tempdir");
        let data_dir = tempdir.path().join("data");
        std::fs::create_dir_all(data_dir.join("games")).expect("create games dir");

        std::fs::write(
            data_dir.join("games/custom-generic.toml"),
            r#"
                id = "custom-generic"
                display_name = "Custom Generic"
                steam_app_id = "1234567"
                install_dir_name = "Custom Generic"
                executable_dir = "bin/x64"
                mod_dir = "mods"
                nexus_domain = "customgeneric"
                proxy_dlls = ["dxgi"]
            "#,
        )
        .expect("write custom game spec");

        std::fs::write(
            data_dir.join("games/stellar-blade.optiscaler.toml"),
            r#"
                [[profile]]
                id = "community-dxgi"
                name = "Community tested dxgi.dll (local override)"
                source_url = "https://example.invalid/stellar-blade"
                tested_optiscaler_version = "0.9.1"
                source_mode = "github_release"
                proxy_dll = "dxgi.dll"
                release_tag = "official:v0.9.1"
                copy_companion_files = true
                enable_optipatcher = true
                fsr4_variant = "latest_fp8"
                emulate_fp8 = true
                spoof_dlss = false
                notes = "Local override for Stellar Blade."

                [[profile]]
                id = "community-dxgi-plus"
                name = "Additional user profile"
                source_url = "https://example.invalid/stellar-blade-plus"
                tested_optiscaler_version = "0.9.1"
                proxy_dll = "d3d11.dll"
                notes = "Secondary profile to prove multi-profile sidecars work."
            "#,
        )
        .expect("write stellar blade optiscaler sidecar");

        std::fs::write(
            data_dir.join("games/custom-generic.optiscaler.toml"),
            r#"
                [[profile]]
                id = "custom-community"
                name = "Community profile"
                source_url = "https://example.invalid/custom-generic"
                tested_optiscaler_version = "0.9.1"
                proxy_dll = "dxgi.dll"
                notes = "User-added game profile."
            "#,
        )
        .expect("write custom game optiscaler sidecar");

        modde_core::paths::set_data_dir(data_dir.clone());

        std::mem::forget(tempdir);
        data_dir
    })
}

#[test]
#[cfg(feature = "ue4")]
fn optiscaler_user_profile_appears_for_built_in_game() {
    let _ = shared_data_dir();

    let profiles = resolve_optiscaler_profiles("stellar-blade");
    assert_eq!(profiles.len(), 2);
    assert!(
        profiles
            .iter()
            .any(|profile| profile.id == "community-dxgi")
    );
    assert!(
        profiles
            .iter()
            .any(|profile| profile.id == "community-dxgi-plus")
    );
}

#[test]
fn optiscaler_user_profile_attaches_to_user_added_game() {
    let _ = shared_data_dir();

    let plugin = resolve_game_plugin("custom-generic").expect("custom game should resolve");
    assert_eq!(plugin.display_name(), "Custom Generic");

    let profiles = resolve_optiscaler_profiles("custom-generic");
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].id, "custom-community");
    assert_eq!(
        default_optiscaler_profile("custom-generic").map(|profile| profile.id),
        Some("custom-community")
    );
}

#[test]
fn optiscaler_user_profile_overrides_built_in_with_same_id() {
    let _ = shared_data_dir();

    let profiles = resolve_optiscaler_profiles("stellar-blade");
    let profile = profiles
        .iter()
        .find(|profile| profile.id == "community-dxgi")
        .expect("overridden profile should exist");

    assert_eq!(profile.name, "Community tested dxgi.dll (local override)");
    assert_eq!(profile.source_url, "https://example.invalid/stellar-blade");
    assert!(profile.copy_companion_files);
    assert!(profile.enable_optipatcher);
    assert!(profile.emulate_fp8);
    assert!(!profile.spoof_dlss);
}

#[test]
fn optiscaler_resolve_profiles_reuses_cached_slice() {
    let _ = shared_data_dir();

    let first = resolve_optiscaler_profiles("stellar-blade");
    let second = resolve_optiscaler_profiles("stellar-blade");

    assert!(std::ptr::eq(first, second));
}
