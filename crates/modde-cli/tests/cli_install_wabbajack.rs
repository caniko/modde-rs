//! End-to-end `modde install wabbajack` from archives already in the store:
//! apply, validate, deploy and record the locked profile.

mod common;

use std::io::Write;

use common::Fixture;
use serde_json::{Value, json};
use xxhash_rust::xxh64::xxh64;

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    let mut zip = zip::ZipWriter::new(&mut cursor);
    for (name, data) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    cursor.into_inner()
}

fn write_wabbajack(path: &std::path::Path, manifest: &Value) {
    let mut writer = zip::ZipWriter::new(std::fs::File::create(path).unwrap());
    writer
        .start_file("modlist", zip::write::SimpleFileOptions::default())
        .unwrap();
    writer.write_all(manifest.to_string().as_bytes()).unwrap();
    writer.finish().unwrap();
}

fn from_archive(hash: u64, file: &str, to: &str, size: usize) -> Value {
    json!({
        "$type": "FromArchive, Wabbajack.Lib",
        "ArchiveHashPath": [hash, file],
        "To": to,
        "Size": size,
    })
}

#[test]
fn installs_a_modlist_from_archives_already_in_the_store() {
    let fx = Fixture::new();
    let archive = zip(&[("readme.txt", b"hello"), ("a.esp", b"plugin")]);
    let hash = xxh64(&archive, 0);
    let store = fx.data_dir().join("store");
    std::fs::create_dir_all(&store).unwrap();
    std::fs::write(store.join(format!("{hash:016x}.archive")), &archive).unwrap();
    let manifest = json!({
        "Name": "Tiny", "Author": "t", "Description": "d",
        "Game": "SkyrimSpecialEdition", "Version": "1.0",
        "Archives": [{
            "Hash": hash, "Name": "tiny.zip", "Size": archive.len(),
            "State": {
                "$type": "ManualDownloader, Wabbajack.Lib",
                "Url": "https://example.test/tiny.zip", "Prompt": "",
            },
        }],
        "Directives": [
            from_archive(hash, "readme.txt", "mods\\Tiny\\readme.txt", 5),
            from_archive(hash, "a.esp", "mods\\Tiny\\a.esp", 6),
        ],
    });
    let list = fx.root().join("tiny.wabbajack");
    write_wabbajack(&list, &manifest);
    let game = fx.root().join("game");
    std::fs::create_dir_all(&game).unwrap();
    let install = |extra: &[&str]| {
        fx.cmd()
            .args(["install", "wabbajack", list.to_str().unwrap()])
            .args(["--profile", "tiny", "--game-dir", game.to_str().unwrap()])
            .args(extra)
            .output()
            .expect("spawn modde install wabbajack")
    };

    let first = install(&[]);
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(std::fs::read(game.join("readme.txt")).unwrap(), b"hello");
    assert_eq!(std::fs::read(game.join("a.esp")).unwrap(), b"plugin");
    let lock = fx
        .cmd()
        .args(["profile", "lock-info", "tiny"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&lock.stdout).contains("Reason:    Wabbajack"));

    // A complete staging tree is reused; --no-deploy leaves the game alone.
    std::fs::remove_file(game.join("readme.txt")).unwrap();
    let again = install(&["--no-deploy", "--no-acquire-missing"]);
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(!game.join("readme.txt").exists());

    // A missing archive fails the default missing-archive policy.
    std::fs::remove_file(store.join(format!("{hash:016x}.archive"))).unwrap();
    let missing = install(&["--force", "--reset-staging", "--no-acquire-missing"]);
    assert!(!missing.status.success());
}
