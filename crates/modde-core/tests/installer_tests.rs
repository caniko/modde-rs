use std::path::PathBuf;

use modde_core::installer::{InstallMethod, InstallPlan};

fn touch(path: &std::path::Path, body: &[u8]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

fn plan(method: InstallMethod) -> InstallPlan {
    InstallPlan {
        method,
        strip_prefix: None,
        source_archive_hash: "fixture".into(),
        staged_files: Vec::new(),
    }
}

#[test]
fn strip_content_root_stages_data_contents_without_data_prefix() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store");
    touch(&extracted.join("Data/Foo.esp"), b"plugin");

    let mut plan = plan(InstallMethod::StripContentRoot {
        root: "Data".into(),
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert!(store.join("Foo.esp").exists());
    assert_eq!(files[0].rel_path, "Foo.esp");
    assert_eq!(files[0].origin_rel_path, "Data/Foo.esp");
}

#[test]
fn directory_mod_stages_root_under_store_mod_name() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/BetterRanching");
    touch(&extracted.join("manifest.json"), b"{}");

    let mut plan = plan(InstallMethod::DirectoryMod {
        directory_name: None,
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert!(store.join("BetterRanching/manifest.json").exists());
    assert_eq!(files[0].rel_path, "BetterRanching/manifest.json");
}

#[test]
fn directory_mod_from_xml_uses_marker_id() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/bannerlord_mod");
    touch(
        &extracted.join("SubModule.xml"),
        br#"<Module><Name value="Visible"/><Id value="StableId"/></Module>"#,
    );

    let mut plan = plan(InstallMethod::DirectoryModFromXml {
        marker: PathBuf::from("SubModule.xml"),
        id_attr: "Id.value".into(),
        fallback_name: None,
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert!(store.join("StableId/SubModule.xml").exists());
    assert_eq!(files[0].rel_path, "StableId/SubModule.xml");
}

#[test]
fn multi_root_overlay_preserves_selected_roots() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/witcher_mod");
    touch(
        &extracted.join("mods/modFoo/content/scripts/a.ws"),
        b"script",
    );
    touch(&extracted.join("dlc/dlcFoo/content/blob.bundle"), b"bundle");
    touch(&extracted.join("bin/config.xml"), b"xml");

    let mut plan = plan(InstallMethod::MultiRootOverlay {
        roots: vec!["mods".into(), "dlc".into(), "bin".into()],
    });
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();
    let rels = files
        .iter()
        .map(|file| file.rel_path.as_str())
        .collect::<Vec<_>>();

    assert!(store.join("mods/modFoo/content/scripts/a.ws").exists());
    assert!(rels.contains(&"mods/modFoo/content/scripts/a.ws"));
    assert!(rels.contains(&"dlc/dlcFoo/content/blob.bundle"));
    assert!(rels.contains(&"bin/config.xml"));
}

#[test]
fn single_file_set_stages_only_root_files() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/bg3_mod");
    touch(&extracted.join("Cool.pak"), b"pak");
    touch(&extracted.join("nested/Ignored.pak"), b"pak");

    let mut plan = plan(InstallMethod::SingleFileSet);
    let files = modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert_eq!(files.len(), 1);
    assert!(store.join("Cool.pak").exists());
    assert!(!store.join("Ignored.pak").exists());
}

const MODULE_CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <moduleName>Fixture Mod</moduleName>
    <requiredInstallFiles>
        <file source="core/base.esp" destination="base.esp"/>
        <folder source="shared" destination="shared"/>
    </requiredInstallFiles>
    <installSteps order="Explicit">
        <installStep name="Textures">
            <optionalFileGroups order="Explicit">
                <group name="Resolution" type="SelectExactlyOne">
                    <plugins order="Explicit">
                        <plugin name="High">
                            <description>High</description>
                            <files><file source="hi/tex.dds" destination="textures/tex.dds"/></files>
                            <typeDescriptor><type name="Recommended"/></typeDescriptor>
                        </plugin>
                        <plugin name="Low">
                            <description>Low</description>
                            <files><file source="lo/tex.dds" destination="textures/tex.dds"/></files>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
            </optionalFileGroups>
        </installStep>
    </installSteps>
</config>
"#;

#[test]
fn fomod_with_a_config_stages_only_the_selected_files() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    let store = tmp.path().join("store/fomod_mod");
    touch(
        &extracted.join("fomod/ModuleConfig.xml"),
        MODULE_CONFIG.as_bytes(),
    );
    for file in ["core/base.esp", "shared/a.ini", "hi/tex.dds", "lo/tex.dds"] {
        touch(&extracted.join(file), file.as_bytes());
    }
    let config = fomod_oxide::ModuleConfig::parse(MODULE_CONFIG).unwrap();
    let decl = fomod_oxide::DeclarativeConfig::from_defaults(MODULE_CONFIG, "Fixture Mod", &config);

    let mut plan = plan(InstallMethod::Fomod {
        module_config: PathBuf::from("fomod/ModuleConfig.xml"),
        config_toml: Some(toml::to_string(&decl).unwrap()),
    });
    modde_core::installer::execute(&mut plan, &extracted, &store).unwrap();

    assert_eq!(
        std::fs::read(store.join("base.esp")).unwrap(),
        b"core/base.esp"
    );
    assert_eq!(
        std::fs::read(store.join("shared/a.ini")).unwrap(),
        b"shared/a.ini"
    );
    assert_eq!(
        std::fs::read(store.join("textures/tex.dds")).unwrap(),
        b"hi/tex.dds"
    );
    assert!(
        plan.staged_files
            .iter()
            .any(|f| f.origin_rel_path == "core/base.esp")
    );

    let mut broken = fomod_plan_with_config("not = [toml");
    let err =
        modde_core::installer::execute(&mut broken, &extracted, &tmp.path().join("store/broken"))
            .unwrap_err();
    assert!(err.to_string().contains("invalid config TOML"), "{err}");
}

fn fomod_plan_with_config(config: &str) -> InstallPlan {
    plan(InstallMethod::Fomod {
        module_config: PathBuf::from("fomod/ModuleConfig.xml"),
        config_toml: Some(config.to_string()),
    })
}

#[test]
fn bain_stages_selected_options_and_rejects_missing_ones() {
    let tmp = tempfile::tempdir().unwrap();
    let extracted = tmp.path().join("extracted");
    touch(&extracted.join("00 Core/core.esp"), b"core");
    touch(&extracted.join("10 Option/option.esp"), b"option");
    touch(&extracted.join("20 Unused/unused.esp"), b"unused");

    let store = tmp.path().join("store/bain_mod");
    let mut chosen = plan(InstallMethod::Bain {
        selected_subdirs: vec!["00 Core".into(), "10 Option".into()],
    });
    let files = modde_core::installer::execute(&mut chosen, &extracted, &store).unwrap();
    assert_eq!(files.len(), 2);
    assert!(store.join("core.esp").exists() && store.join("option.esp").exists());
    assert!(!store.join("unused.esp").exists());

    let mut missing = plan(InstallMethod::Bain {
        selected_subdirs: vec!["99 Missing".into()],
    });
    assert!(
        modde_core::installer::execute(&mut missing, &extracted, &tmp.path().join("store/x"))
            .is_err()
    );
}

#[test]
fn redmod_and_user_config_overlays_stage_into_their_layouts() {
    let tmp = tempfile::tempdir().unwrap();
    let redmod_src = tmp.path().join("redmod");
    touch(&redmod_src.join("info.json"), b"{}");
    touch(&redmod_src.join("archives/a.archive"), b"a");
    let redmod_store = tmp.path().join("store/CoolMod");
    let mut redmod = plan(InstallMethod::REDmod {
        manifest: PathBuf::from("info.json"),
    });
    modde_core::installer::execute(&mut redmod, &redmod_src, &redmod_store).unwrap();
    assert!(
        redmod_store
            .join("mods/CoolMod/archives/a.archive")
            .exists()
    );
    assert!(redmod_store.join("mods/CoolMod/info.json").exists());

    let config_src = tmp.path().join("config");
    touch(&config_src.join("Engine.ini"), b"[Core]");
    let config_store = tmp.path().join("store/tweaks");
    let mut overlay = plan(InstallMethod::UserConfigOverlay {
        target_id: "user-config".into(),
    });
    let files = modde_core::installer::execute(&mut overlay, &config_src, &config_store).unwrap();
    assert_eq!(files.len(), 1);
    assert!(config_store.join("Engine.ini").exists());
}
