//! End-to-end tests for `modde export`, the profile mod list as CSV.

mod common;

use std::path::PathBuf;

use common::Fixture;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::resolver::ModId;
use modde_core::{GameId, ModdeDb, NexusFileId, NexusModId};

const GAME: &str = "stardew-valley";

#[tokio::test]
async fn exports_quoted_csv_to_stdout_or_a_file() {
    let fx = Fixture::new();
    let db = ModdeDb::open_at(&fx.data_dir().join("modde.db"))
        .await
        .unwrap();
    let id = db
        .create_profile(&Profile {
            id: None,
            name: "main".to_string(),
            game_id: GameId::from(GAME),
            source: ProfileSource::Manual,
            mods: [("a", true), ("b", false)]
                .into_iter()
                .map(|(mod_id, enabled)| EnabledMod {
                    mod_id: mod_id.to_string(),
                    enabled,
                    ..Default::default()
                })
                .collect(),
            overrides: PathBuf::from("/tmp/overrides"),
            load_order_rules: smallvec::SmallVec::new(),
            load_order_lock: None,
        })
        .await
        .unwrap();
    let (a, b) = (ModId::from("a"), ModId::from("b"));
    db.set_mod_notes(id, &a, Some("Says \"hi\", twice\nok"))
        .await
        .unwrap();
    db.set_mod_tags(id, &a, &["x".to_string(), "y".to_string()])
        .await
        .unwrap();
    db.set_mod_nexus_meta(
        id,
        &b,
        NexusModId::from(42),
        NexusFileId::from(7),
        "stardewvalley",
        0,
    )
    .await
    .unwrap();
    drop(db);
    let export = |extra: &[&str]| {
        fx.cmd()
            .args(["export", "--profile", "main", "--game", GAME])
            .args(extra)
            .output()
            .expect("spawn modde export")
    };

    let all = export(&[]);
    assert!(all.status.success());
    let csv = String::from_utf8_lossy(&all.stdout);
    let mut lines = csv.lines();
    assert_eq!(
        lines.next(),
        Some("mod_id,enabled,version,category,notes,tags,nexus_mod_id")
    );
    assert!(
        csv.contains("a,true,,,\"Says \"\"hi\"\", twice\nok\",\"[\"\"x\"\",\"\"y\"\"]\",\n"),
        "{csv}"
    );
    assert!(csv.ends_with("b,false,,,,,42\n"), "{csv}");

    let path = fx.root().join("mods.csv");
    let to_file = export(&[
        "--columns",
        "mod_id, nexus_mod_id,bogus",
        "--output",
        path.to_str().unwrap(),
    ]);
    assert!(to_file.status.success());
    assert!(String::from_utf8_lossy(&to_file.stdout).contains("Exported 2 mods to "));
    assert!(String::from_utf8_lossy(&to_file.stderr).contains("Unknown column: bogus"));
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "mod_id,nexus_mod_id\na,\nb,42\n"
    );
}
