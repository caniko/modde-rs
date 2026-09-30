use super::*;

#[test]
fn scanner_reports_absent_install() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let state = scan_optiscaler_install_in_dir(tmp.path(), &BTreeSet::new()).expect("scan");
    assert_eq!(state.status, OptiScalerInstallStatus::Absent);
    assert!(state.recognized_files.is_empty());
}

#[test]
fn scanner_reports_unmanaged_install_with_config_and_companions() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    std::fs::write(
        tmp.path().join("OptiScaler.ini"),
        "[OptiScaler]\nDxgi=false\n",
    )
    .expect("ini");
    std::fs::write(tmp.path().join("fakenvapi.dll"), b"fake").expect("companion");

    let state = scan_optiscaler_install_in_dir(tmp.path(), &BTreeSet::new()).expect("scan");
    assert_eq!(state.status, OptiScalerInstallStatus::Unmanaged);
    assert_eq!(state.proxy_dlls, vec!["dxgi.dll".to_string()]);
    assert_eq!(state.wine_dll_overrides, vec!["dxgi".to_string()]);
    assert_eq!(
        state.ini_settings.get("OptiScaler.Dxgi"),
        Some(&"false".to_string())
    );
    assert_eq!(state.recognized_files.len(), 3);
}

#[test]
fn scanner_distinguishes_managed_and_conflicted_installs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    let mut managed = BTreeSet::new();
    managed.insert("dxgi.dll".to_string());
    let state = scan_optiscaler_install_in_dir(tmp.path(), &managed).expect("scan");
    assert_eq!(state.status, OptiScalerInstallStatus::Managed);

    std::fs::write(tmp.path().join("winmm.dll"), b"optiscaler").expect("proxy");
    let state = scan_optiscaler_install_in_dir(tmp.path(), &managed).expect("scan");
    assert_eq!(state.status, OptiScalerInstallStatus::Conflicted);
}

#[test]
#[cfg(feature = "ue4")]
fn scanner_matches_stellar_blade_root_relative_managed_manifest() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let exe_dir = tmp.path().join("SB/Binaries/Win64");
    std::fs::create_dir_all(&exe_dir).expect("exe dir");
    std::fs::write(exe_dir.join("dxgi.dll"), b"optiscaler").expect("proxy");
    std::fs::write(exe_dir.join("OptiScaler.ini"), "[OptiScaler]\nDxgi=auto\n").expect("ini");
    std::fs::write(exe_dir.join("version.txt"), "v0.9.1\n").expect("version");

    let unmanaged =
        scan_optiscaler_install("stellar-blade", tmp.path(), &BTreeSet::new()).expect("scan");
    assert_eq!(unmanaged.status, OptiScalerInstallStatus::Unmanaged);
    assert_eq!(
        unmanaged.summary(),
        "unmanaged; version v0.9.1; proxy dxgi.dll"
    );
    assert_eq!(unmanaged.wine_dll_overrides, vec!["dxgi".to_string()]);

    let mut managed = BTreeSet::new();
    managed.insert("sb/binaries/win64/dxgi.dll".to_string());
    managed.insert("sb/binaries/win64/optiscaler.ini".to_string());
    let managed_state =
        scan_optiscaler_install("stellar-blade", tmp.path(), &managed).expect("scan");
    assert_eq!(managed_state.status, OptiScalerInstallStatus::Managed);
    assert_eq!(
        managed_state.summary(),
        "managed; version v0.9.1; proxy dxgi.dll"
    );
}

#[test]
fn scanner_ignores_backup_files_that_look_like_fidelityfx_dlls() {
    let tmp = tempfile::tempdir().expect("tempdir");
    std::fs::write(tmp.path().join("dxgi.dll"), b"optiscaler").expect("proxy");
    std::fs::write(tmp.path().join("amd_fidelityfx_vk.dll"), b"managed").expect("dll");
    std::fs::write(tmp.path().join("amd_fidelityfx_vk.dll.b"), b"backup").expect("backup");

    let mut managed = BTreeSet::new();
    managed.insert("dxgi.dll".to_string());
    managed.insert("amd_fidelityfx_vk.dll".to_string());

    let state = scan_optiscaler_install_in_dir(tmp.path(), &managed).expect("scan");

    assert_eq!(state.status, OptiScalerInstallStatus::Managed);
    assert_eq!(state.recognized_files.len(), 2);
    assert!(state.recognized_files.iter().all(|file| {
        file.rel_path
            .extension()
            .is_some_and(|extension| extension == "dll")
    }));
}
