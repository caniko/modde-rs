use super::*;
use crate::tools::optiscaler::archive::{is_optiscaler_payload_file, optiscaler_payload_dest};

#[test]
fn optiscaler_fp8_variant_copies_selected_fsr4_dll() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
    std::fs::create_dir_all(source.path().join(FSR4_LATEST_DIR)).expect("latest dir");
    std::fs::write(
        source.path().join(FSR4_LATEST_DIR).join(FSR4_DLL_NAME),
        b"fp8",
    )
    .expect("fp8 dll");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));

    let applied = OptiScaler.apply(game.path(), &config).expect("apply");

    assert_eq!(
        std::fs::read(game.path().join(FSR4_DLL_NAME)).expect("deployed FSR4"),
        b"fp8"
    );
    assert!(applied.files.contains(&PathBuf::from(FSR4_DLL_NAME)));
}

#[test]
fn optiscaler_int8_variant_copies_int8_and_ignores_fp8_env() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
    std::fs::create_dir_all(source.path().join(FSR4_INT8_DIR)).expect("int8 dir");
    std::fs::write(
        source.path().join(FSR4_INT8_DIR).join(FSR4_DLL_NAME),
        b"int8",
    )
    .expect("int8 dll");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(true));

    OptiScaler.apply(game.path(), &config).expect("apply");

    assert_eq!(
        std::fs::read(game.path().join(FSR4_DLL_NAME)).expect("deployed FSR4"),
        b"int8"
    );
    // PROTON_FSR4_UPGRADE is emitted for any FSR4 variant
    assert_eq!(
        OptiScaler.env_vars(&config).as_slice(),
        [(
            PROTON_FSR4_ENV_KEY.to_string(),
            PROTON_FSR4_ENV_VALUE.to_string()
        )]
    );
}

#[test]
fn optiscaler_apply_removes_stale_alternate_proxy_dlls() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"new dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
    std::fs::write(game.path().join("d3d12.dll"), b"old proxy").expect("stale proxy");

    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("proxy_dll", serde_json::json!("dxgi.dll"));

    OptiScaler.apply(game.path(), &config).expect("apply");

    assert!(game.path().join("dxgi.dll").is_file());
    assert!(!game.path().join("d3d12.dll").exists());
}

#[test]
fn optiscaler_preview_reports_missing_selected_fsr4_variant_for_root_only_payload() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
    std::fs::write(source.path().join(FSR4_DLL_NAME), b"ambiguous").expect("root fsr4 dll");

    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));

    let preview = OptiScaler
        .preview_apply_for(game.path(), None, &config)
        .expect("preview");

    assert!(
        preview
            .missing_inputs
            .iter()
            .any(|input| input.contains(FSR4_INT8_DIR) && input.contains(FSR4_DLL_NAME))
    );
}

#[test]
fn optiscaler_missing_optipatcher_is_reported_in_preview() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[Plugins]\n").expect("source ini");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("enable_optipatcher", serde_json::json!(true));

    let preview = OptiScaler
        .preview_apply_for(game.path(), None, &config)
        .expect("preview");

    let optipatcher_rel = PathBuf::from("plugins").join(OPTIPATCHER_ASSET);
    assert!(
        preview
            .missing_inputs
            .iter()
            .any(|input| input.contains("OptiPatcher.asi"))
            || preview.changed_files.contains(&optipatcher_rel)
            || preview.unchanged_files.contains(&optipatcher_rel)
    );
}

#[test]
fn optiscaler_uses_bundled_optipatcher_from_source_plugins() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[Plugins]\n").expect("source ini");
    std::fs::create_dir_all(source.path().join("plugins")).expect("plugins dir");
    std::fs::write(
        source.path().join("plugins").join(OPTIPATCHER_ASSET),
        b"bundled asi",
    )
    .expect("source optipatcher");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("enable_optipatcher", serde_json::json!(true));

    OptiScaler.apply(game.path(), &config).expect("apply");

    assert_eq!(
        std::fs::read(game.path().join("plugins").join(OPTIPATCHER_ASSET))
            .expect("deployed optipatcher"),
        b"bundled asi"
    );
}

#[test]
fn optiscaler_archive_payload_keeps_optipatcher_in_plugins() {
    assert!(is_optiscaler_payload_file("optipatcher.asi"));
    assert_eq!(
        optiscaler_payload_dest(
            Path::new("/cache"),
            Path::new("plugins/OptiPatcher.asi"),
            std::ffi::OsStr::new("OptiPatcher.asi"),
        ),
        PathBuf::from("/cache/plugins/OptiPatcher.asi")
    );
}

#[test]
fn optiscaler_fp8_env_includes_proton_upgrade_and_emulation() {
    let mut config = OptiScaler.default_config();
    config.set("hardware_tuning", serde_json::json!(HARDWARE_TUNING_MANUAL));
    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_LATEST_FP8));
    config.set("emulate_fp8", serde_json::json!(true));

    let env = OptiScaler.env_vars(&config);
    assert!(
        env.iter()
            .any(|(k, v)| k == FP8_EMULATION_ENV_KEY && v == FP8_EMULATION_ENV_VALUE)
    );
    assert!(
        env.iter()
            .any(|(k, v)| k == PROTON_FSR4_ENV_KEY && v == PROTON_FSR4_ENV_VALUE)
    );

    config.set("fsr4_variant", serde_json::json!(FSR4_VARIANT_INT8_402));
    config.set("emulate_fp8", serde_json::json!(false));

    let env_int8 = OptiScaler.env_vars(&config);
    // PROTON_FSR4_UPGRADE still emitted for INT8 variant
    assert!(!env_int8.iter().any(|(k, _)| k == FP8_EMULATION_ENV_KEY));
    assert!(
        env_int8
            .iter()
            .any(|(k, v)| k == PROTON_FSR4_ENV_KEY && v == PROTON_FSR4_ENV_VALUE)
    );
}

#[test]
fn optiscaler_7z_scratch_dir_is_removed_on_failure_and_success() {
    use crate::tools::optiscaler::archive::extract_7z_flat_in;

    let tmp = tempfile::tempdir().expect("tmp");
    let base = tmp.path().join("tmp");
    let dest = tmp.path().join("dest");
    let leftovers = || std::fs::read_dir(&base).map_or(0, Iterator::count);

    // Failure: nothing to extract.
    assert!(extract_7z_flat_in(&base, &tmp.path().join("missing.7z"), &dest).is_err());
    assert_eq!(leftovers(), 0);

    // Success needs a real 7z binary to build the fixture; skip without one.
    let payload = tmp.path().join("payload");
    std::fs::create_dir_all(&payload).unwrap();
    std::fs::write(payload.join("OptiScaler.dll"), b"dll").unwrap();
    let archive = tmp.path().join("fixture.7z");
    let built = ["7zz", "7z"].iter().any(|bin| {
        std::process::Command::new(bin)
            .arg("a")
            .arg(&archive)
            .arg(payload.join("OptiScaler.dll"))
            .stdout(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    });
    if !built {
        return;
    }
    extract_7z_flat_in(&base, &archive, &dest).unwrap();
    assert!(dest.join("OptiScaler.dll").exists());
    assert_eq!(leftovers(), 0);
}
