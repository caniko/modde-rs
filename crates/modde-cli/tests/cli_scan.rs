//! End-to-end tests for `modde scan` against fake Skyrim and Cyberpunk
//! installs, including `plugins.txt` load order and Wabbajack manifests.

mod common;

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Output;

use common::Fixture;

fn write(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b"x").unwrap();
}

fn scan(fx: &Fixture, args: &[&str]) -> Output {
    fx.cmd()
        .arg("scan")
        .args(args)
        .output()
        .expect("spawn modde scan")
}

fn ok(output: &Output) -> String {
    assert!(
        output.status.success(),
        "scan failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn mod_count(fx: &Fixture, profile: &str) -> String {
    let list = ok(&fx.cmd().args(["profile", "list"]).output().unwrap());
    list.lines()
        .find(|line| line.trim_start().starts_with(&format!("{profile} ")))
        .unwrap_or_else(|| panic!("{profile} missing:\n{list}"))
        .to_string()
}

#[cfg(feature = "bethesda")]
#[test]
fn bethesda_scan_prefers_plugins_txt_and_pairs_archives() {
    let fx = Fixture::new();
    let install = fx.root().join("skyrim");
    for file in [
        "Listed.esp",
        "Listed.bsa",
        "Listed - Textures.bsa",
        "Loose.esm",
        "Loose.ba2",
        "readme.txt",
    ] {
        write(&install.join("Data").join(file));
    }
    let plugins_txt = fx.home().join(
        ".local/share/Steam/steamapps/compatdata/489830/pfx/drive_c/users/steamuser/AppData/Local/Skyrim Special Edition/plugins.txt",
    );
    std::fs::create_dir_all(plugins_txt.parent().unwrap()).unwrap();
    std::fs::write(&plugins_txt, "# load order\n*Listed.esp\nMissing.esp\n").unwrap();
    let game_dir = install.to_str().unwrap();

    let out = ok(&scan(
        &fx,
        &["--game", "skyrim-se", "--game-dir", game_dir, "--dry-run"],
    ));
    assert!(out.contains("Filesystem scan: 2 mods discovered"), "{out}");
    assert!(out.contains("3     95%  Data          Listed.esp"), "{out}");
    assert!(out.contains("2     80%  Data          Loose.esm"), "{out}");

    let imported = ok(&scan(
        &fx,
        &[
            "--game",
            "skyrim-se",
            "--game-dir",
            game_dir,
            "--import-to",
            "scanned",
        ],
    ));
    assert!(
        imported.contains("Creating new profile 'scanned'"),
        "{imported}"
    );
    assert!(mod_count(&fx, "scanned").contains("2 mods"));

    let again = ok(&scan(
        &fx,
        &[
            "--game",
            "skyrim-se",
            "--game-dir",
            game_dir,
            "--import-to",
            "scanned",
        ],
    ));
    assert!(again.contains("had 2 tracked before"), "{again}");
    assert!(mod_count(&fx, "scanned").contains("2 mods"));
}

/// A `.wabbajack` whose single archive installs `archive/pc/mod/foo.archive`.
#[cfg(feature = "cyberpunk")]
fn cyberpunk_manifest(fx: &Fixture) -> PathBuf {
    let modlist = serde_json::json!({
        "Name": "Probe", "Author": "t", "Description": "t", "Game": "Cyberpunk2077", "Version": "1",
        "Archives": [{ "Hash": 1, "Name": "Foo-1-0.7z", "Size": 1 }],
        "Directives": [{
            "$type": "FromArchive, Wabbajack.Lib",
            "ArchiveHashPath": [1, "foo.archive"],
            "To": "mods\\Foo\\archive\\pc\\mod\\foo.archive"
        }]
    });
    let path = fx.root().join("list.wabbajack");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
    zip.start_file("modlist", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(modlist.to_string().as_bytes()).unwrap();
    zip.finish().unwrap();
    path
}

#[cfg(feature = "cyberpunk")]
#[test]
fn cyberpunk_manifest_import_prunes_duplicates_and_locks_the_profile() {
    let fx = Fixture::new();
    let install = fx.root().join("cyberpunk");
    for file in [
        "archive/pc/mod/foo.archive",
        "archive/pc/mod/other.archive",
        "bin/x64/plugins/cyber_engine_tweaks/mods/Healing/init.lua",
    ] {
        write(&install.join(file));
    }
    let game_dir = install.to_str().unwrap();
    let base = ["--game", "cyberpunk2077", "--game-dir", game_dir];

    let first = ok(&scan(&fx, &[&base[..], &["--import-to", "list"]].concat()));
    assert!(
        first.contains("Filesystem scan: 3 mods discovered"),
        "{first}"
    );
    assert!(first.contains("Imported 3 new mods"), "{first}");

    let manifest = cyberpunk_manifest(&fx);
    let with_manifest = ok(&scan(
        &fx,
        &[
            &base[..],
            &[
                "--manifest",
                manifest.to_str().unwrap(),
                "--import-to",
                "list",
                "--prune-duplicates",
            ],
        ]
        .concat(),
    ));
    assert!(
        with_manifest.contains("1/1 archives detected on disk"),
        "{with_manifest}"
    );
    assert!(
        with_manifest.contains("1 filesystem mods skipped — already covered by manifest"),
        "{with_manifest}"
    );
    assert!(
        with_manifest.contains("Pruned 1 leaked duplicate(s)"),
        "{with_manifest}"
    );
    assert!(
        with_manifest.contains("Applied Wabbajack load order lock"),
        "{with_manifest}"
    );
    assert!(mod_count(&fx, "list").contains("3 mods"));

    let lock = ok(&fx
        .cmd()
        .args(["profile", "lock-info", "list", "--game", "cyberpunk2077"])
        .output()
        .unwrap());
    assert!(lock.contains("Reason:    Wabbajack"), "{lock}");
}

#[cfg(feature = "cyberpunk")]
#[test]
fn manifest_threshold_excludes_partial_archives() {
    let fx = Fixture::new();
    let install = fx.root().join("cyberpunk");
    write(&install.join("archive/pc/mod/unrelated.archive"));
    let manifest = cyberpunk_manifest(&fx);

    let out = ok(&scan(
        &fx,
        &[
            "--game",
            "cyberpunk2077",
            "--game-dir",
            install.to_str().unwrap(),
            "--manifest",
            manifest.to_str().unwrap(),
            "--dry-run",
        ],
    ));
    assert!(out.contains("0/1 archives detected on disk"), "{out}");

    let bad = fx.root().join("bad.wabbajack");
    std::fs::write(&bad, b"not a zip").unwrap();
    let failed = scan(
        &fx,
        &[
            "--game",
            "cyberpunk2077",
            "--game-dir",
            install.to_str().unwrap(),
            "--manifest",
            bad.to_str().unwrap(),
        ],
    );
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("failed to parse manifest"));
}
