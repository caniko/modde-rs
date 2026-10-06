use super::*;

#[test]
fn parse_optiscaler_ini_preserves_section_paths() {
    let parsed = parse_optiscaler_ini(
        r"
        ; comment
        [OptiScaler]
        Dxgi=auto
        LoadAsiPlugins=true
        [Menu]
        Scale=1.25
        ",
    );
    assert_eq!(parsed.get("OptiScaler.Dxgi"), Some(&"auto".to_string()));
    assert_eq!(
        parsed.get("OptiScaler.LoadAsiPlugins"),
        Some(&"true".to_string())
    );
    assert_eq!(parsed.get("Menu.Scale"), Some(&"1.25".to_string()));
}

#[test]
fn preview_reports_changed_when_proxy_or_ini_differ() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"new dll").expect("source dll");
    std::fs::write(
        source.path().join("OptiScaler.ini"),
        "[FSR]\nFGIndex=auto\n",
    )
    .expect("source ini");
    std::fs::write(game.path().join("dxgi.dll"), b"old dll").expect("dest dll");
    std::fs::write(game.path().join("OptiScaler.ini"), "[FSR]\nFGIndex=1\n").expect("dest ini");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set(
        "ini_overrides",
        serde_json::json!({ "FSR": { "FGIndex": "2" } }),
    );

    let preview = OptiScaler
        .preview_apply_for(game.path(), None, &config)
        .expect("preview");

    assert!(preview.changed_files.contains(&PathBuf::from("dxgi.dll")));
    assert!(
        preview
            .changed_files
            .contains(&PathBuf::from("OptiScaler.ini"))
    );
    assert_eq!(preview.missing_inputs, [] as [std::string::String; 0]);
}

#[test]
fn preview_reports_unchanged_and_does_not_create_target_dirs() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"same dll").expect("source dll");
    std::fs::write(
        source.path().join("OptiScaler.ini"),
        "[FSR]\nFGIndex=auto\n",
    )
    .expect("source ini");
    let target = game.path().join("Bin");
    std::fs::create_dir(&target).expect("target");
    std::fs::write(target.join("dxgi.dll"), b"same dll").expect("dest dll");
    std::fs::write(
        target.join("OptiScaler.ini"),
        "[FSR]\nFGIndex=auto\nFsr4Update=True\n[Spoofing]\nDxgi=false\n[Plugins]\nLoadAsiPlugins=auto\n",
    )
    .expect("dest ini");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("exe_subdir", serde_json::json!("Bin"));

    let preview = OptiScaler
        .preview_apply_for(game.path(), None, &config)
        .expect("preview");

    assert_eq!(preview.changed_files, [] as [std::path::PathBuf; 0]);
    assert!(
        preview
            .unchanged_files
            .contains(&PathBuf::from("Bin/dxgi.dll"))
    );
    assert!(
        preview
            .unchanged_files
            .contains(&PathBuf::from("Bin/OptiScaler.ini"))
    );
    assert!(!game.path().join("Other").exists());
}

#[test]
fn preview_does_not_create_missing_target_directory() {
    let source = tempfile::tempdir().expect("source");
    let game = tempfile::tempdir().expect("game");
    std::fs::write(source.path().join("OptiScaler.dll"), b"dll").expect("source dll");
    std::fs::write(source.path().join("OptiScaler.ini"), "[FSR]\n").expect("source ini");
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("local_dir"));
    config.set(
        "local_source_dir",
        serde_json::json!(source.path().display().to_string()),
    );
    config.set("exe_subdir", serde_json::json!("MissingBin"));

    let preview = OptiScaler
        .preview_apply_for(game.path(), None, &config)
        .expect("preview");

    assert!(preview.has_changes());
    assert!(!game.path().join("MissingBin").exists());
}
