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
            "not valid toml",
        )
        .expect("write bad stellar blade sidecar");

        modde_core::paths::set_data_dir(data_dir.clone());
        std::mem::forget(tempdir);
        data_dir
    })
}

#[test]
#[traced_test]
fn optiscaler_bad_sidecar_logs_and_skips() {
    let _ = shared_data_dir();

    let profiles = resolve_optiscaler_profiles("stellar-blade");
    assert_eq!(profiles.len(), usize::from(cfg!(feature = "ue4")));
    assert!(logs_contain("skipping user OptiScaler profile spec"));
}
