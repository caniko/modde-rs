#![cfg(feature = "ue4")]

use std::sync::OnceLock;

use modde_games::resolve_optiscaler_profiles;
use tracing_test::traced_test;

fn shared_data_dir() -> &'static std::path::PathBuf {
    static DATA_DIR: OnceLock<std::path::PathBuf> = OnceLock::new();
    DATA_DIR.get_or_init(|| {
        let tempdir = tempfile::TempDir::new().expect("create tempdir");
        let data_dir = tempdir.path().join("data");
        std::fs::create_dir_all(data_dir.join("games")).expect("create games dir");

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
            "#,
        )
        .expect("write stellar blade optiscaler sidecar");

        modde_core::paths::set_data_dir(data_dir.clone());
        std::mem::forget(tempdir);
        data_dir
    })
}

#[test]
#[traced_test]
fn optiscaler_user_profile_override_emits_warning() {
    let _ = shared_data_dir();

    let profiles = resolve_optiscaler_profiles("stellar-blade");
    assert_eq!(profiles.len(), 1);
    assert_eq!(
        profiles[0].name,
        "Community tested dxgi.dll (local override)"
    );
    assert!(logs_contain("overrides built-in profile"));
}
