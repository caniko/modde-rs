//! End-to-end tests for `modde instance`: create, list and switch named
//! data directories recorded in the fixture's config directory.

mod common;

use common::Fixture;

#[test]
fn instances_are_created_listed_and_switched() {
    let fx = Fixture::new();
    let run = |args: &[&str]| {
        fx.cmd()
            .arg("instance")
            .args(args)
            .output()
            .expect("spawn modde instance")
    };
    let stdout = |output: std::process::Output| {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    };

    assert!(stdout(run(&["list"])).contains("No instances configured"));

    let main = fx.root().join("main-data");
    let alt = fx.root().join("alt-data");
    for (name, dir) in [("main", &main), ("alt", &alt)] {
        let created = stdout(run(&["create", name, "--data-dir", dir.to_str().unwrap()]));
        assert!(created.contains(&format!("Created instance '{name}'")));
        assert!(dir.is_dir());
    }
    let duplicate = run(&["create", "main", "--data-dir", main.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&duplicate.stderr).contains("instance 'main' already exists"));

    // The first instance becomes active.
    let listed = stdout(run(&["list"]));
    assert!(listed.contains("  main *  "), "{listed}");
    assert!(listed.contains("  alt  "), "{listed}");

    assert!(stdout(run(&["switch", "alt"])).contains("Switched to instance 'alt'"));
    assert!(stdout(run(&["list"])).contains("  alt *  "));
    let missing = run(&["switch", "ghost"]);
    assert!(String::from_utf8_lossy(&missing.stderr).contains("instance 'ghost' not found"));
    assert!(fx.home().join(".config/modde/instances.toml").is_file());
}
