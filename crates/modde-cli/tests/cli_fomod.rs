//! End-to-end tests for `modde fomod` against a small fixture installer.

mod common;

use std::path::{Path, PathBuf};
use std::process::Output;

use common::Fixture;

const MODULE_CONFIG: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <moduleName>Fixture Mod</moduleName>
    <requiredInstallFiles>
        <file source="core/base.esp" destination="base.esp"/>
    </requiredInstallFiles>
    <installSteps order="Explicit">
        <installStep name="Textures">
            <optionalFileGroups order="Explicit">
                <group name="Resolution" type="SelectExactlyOne">
                    <plugins order="Explicit">
                        <plugin name="High">
                            <description>High resolution.</description>
                            <files><file source="hi/tex.dds" destination="textures/tex.dds"/></files>
                            <typeDescriptor><type name="Recommended"/></typeDescriptor>
                        </plugin>
                        <plugin name="Low">
                            <description>Low resolution.</description>
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

fn fomod_mod(fx: &Fixture) -> PathBuf {
    let dir = fx.root().join("mod");
    for (path, body) in [
        ("fomod/ModuleConfig.xml", MODULE_CONFIG),
        ("core/base.esp", "core"),
        ("hi/tex.dds", "hi"),
        ("lo/tex.dds", "lo"),
    ] {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    dir
}

fn fomod(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("fomod")
        .args(args)
        .output()
        .expect("spawn modde fomod")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "command failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn inspect_lists_steps_groups_and_plugins() {
    let fx = Fixture::new();
    let dir = fomod_mod(&fx);

    let out = ok(&fomod(&fx, &["inspect", path(&dir)]));
    assert!(out.contains("Module: Fixture Mod"), "{out}");
    assert!(out.contains("Required files: 1 items"), "{out}");
    assert!(
        out.contains("Group 0: Resolution (SelectExactlyOne)"),
        "{out}"
    );
    assert!(out.contains("[0] High (Recommended, 1 files)"), "{out}");
    assert!(out.contains("[1] Low (Optional, 1 files)"), "{out}");
}

#[test]
fn generate_emits_default_or_all_selections() {
    let fx = Fixture::new();
    let dir = fomod_mod(&fx);

    let defaults: toml::Table =
        toml::from_str(&ok(&fomod(&fx, &["generate", path(&dir)]))).unwrap();
    assert_eq!(defaults["rev"].as_str(), Some("Fixture Mod"));
    assert_eq!(
        defaults["selections"]["Textures"]["Resolution"],
        toml::Value::Array(vec!["High".into()])
    );

    let all: serde_json::Value = serde_json::from_str(&ok(&fomod(
        &fx,
        &["generate", path(&dir), "--format", "json", "--all"],
    )))
    .unwrap();
    assert_eq!(
        all["selections"]["Textures"]["Resolution"],
        serde_json::json!(["High", "Low"])
    );

    let nix = fomod(&fx, &["generate", path(&dir), "--format", "nix"]);
    assert!(!nix.status.success());
    let unknown = fomod(&fx, &["generate", path(&dir), "--format", "yaml"]);
    assert!(!unknown.status.success());
}

#[test]
fn apply_installs_the_selected_files() {
    let fx = Fixture::new();
    let dir = fomod_mod(&fx);
    let config = fx.root().join("selection.toml");
    std::fs::write(&config, ok(&fomod(&fx, &["generate", path(&dir)]))).unwrap();
    let dest = fx.root().join("out");

    let out = ok(&fomod(
        &fx,
        &[
            "apply",
            path(&dir),
            "--config",
            path(&config),
            "--dest",
            path(&dest),
        ],
    ));
    assert!(out.contains("2 file operations"), "{out}");
    assert_eq!(
        std::fs::read_to_string(dest.join("base.esp")).unwrap(),
        "core"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("textures/tex.dds")).unwrap(),
        "hi"
    );
}

#[test]
fn commands_fail_without_a_module_config() {
    let fx = Fixture::new();
    let empty = fx.root().join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(!fomod(&fx, &["inspect", path(&empty)]).status.success());
    assert!(!fomod(&fx, &["generate", path(&empty)]).status.success());
}
