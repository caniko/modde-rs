use smallvec::smallvec;
use std::path::PathBuf;

use modde_core::GameId;
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::resolver::{ConflictMap, LoadOrderRule, ModId, resolve};

fn make_profile(mods: Vec<(&str, bool)>, rules: smallvec::SmallVec<[LoadOrderRule; 4]>) -> Profile {
    Profile {
        id: None,
        name: "test".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: mods
            .into_iter()
            .map(|(id, enabled)| EnabledMod {
                mod_id: id.to_string(),
                enabled,
                version: None,
                fomod_config: None,
                ..Default::default()
            })
            .collect(),
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: rules,
        load_order_lock: None,
    }
}

#[test]
fn test_resolve_empty_profile() {
    let profile = make_profile(vec![], smallvec![]);
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order, [] as [modde_core::ModId; 0]);
}

#[test]
fn test_resolve_single_mod_no_rules() {
    let profile = make_profile(vec![("only_mod", true)], smallvec![]);
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "only_mod");
}

#[test]
fn test_resolve_rules_reference_nonexistent_mods() {
    let profile = make_profile(
        vec![("mod_a", true)],
        smallvec![LoadOrderRule::LoadAfter {
            mod_id: ModId::from("mod_a"),
            after: ModId::from("ghost_mod"),
        }],
    );
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "mod_a");
}

#[test]
fn test_resolve_duplicate_rules_same_pair() {
    let profile = make_profile(
        vec![("mod_a", true), ("mod_b", true)],
        smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            },
        ],
    );
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 2);
    let pos_a = result.order.iter().position(|m| m == "mod_a").unwrap();
    let pos_b = result.order.iter().position(|m| m == "mod_b").unwrap();
    assert!(pos_a < pos_b);
}

#[test]
fn test_resolve_both_load_after_and_load_before() {
    // mod_b after mod_a (LoadAfter), mod_b before mod_c (LoadBefore) -> A, B, C
    let profile = make_profile(
        vec![("mod_a", true), ("mod_b", true), ("mod_c", true)],
        smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            },
            LoadOrderRule::LoadBefore {
                mod_id: ModId::from("mod_b"),
                before: ModId::from("mod_c"),
            },
        ],
    );
    let result = resolve(&profile).unwrap();
    let pos_a = result.order.iter().position(|m| m == "mod_a").unwrap();
    let pos_b = result.order.iter().position(|m| m == "mod_b").unwrap();
    let pos_c = result.order.iter().position(|m| m == "mod_c").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_b < pos_c);
}

#[test]
fn test_resolve_transitive_ordering() {
    // A after B, B after C -> order: C, B, A
    let profile = make_profile(
        vec![("mod_a", true), ("mod_b", true), ("mod_c", true)],
        smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_a"),
                after: ModId::from("mod_b"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_c"),
            },
        ],
    );
    let result = resolve(&profile).unwrap();
    let pos_a = result.order.iter().position(|m| m == "mod_a").unwrap();
    let pos_b = result.order.iter().position(|m| m == "mod_b").unwrap();
    let pos_c = result.order.iter().position(|m| m == "mod_c").unwrap();
    assert!(pos_c < pos_b);
    assert!(pos_b < pos_a);
}

#[test]
fn test_resolve_complex_diamond() {
    // Diamond: D depends on B and C, B and C depend on A
    //   A -> B -> D
    //   A -> C -> D
    let profile = make_profile(
        vec![
            ("mod_a", true),
            ("mod_b", true),
            ("mod_c", true),
            ("mod_d", true),
        ],
        smallvec![
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_b"),
                after: ModId::from("mod_a"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_c"),
                after: ModId::from("mod_a"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_d"),
                after: ModId::from("mod_b"),
            },
            LoadOrderRule::LoadAfter {
                mod_id: ModId::from("mod_d"),
                after: ModId::from("mod_c"),
            },
        ],
    );
    let result = resolve(&profile).unwrap();
    let pos_a = result.order.iter().position(|m| m == "mod_a").unwrap();
    let pos_b = result.order.iter().position(|m| m == "mod_b").unwrap();
    let pos_c = result.order.iter().position(|m| m == "mod_c").unwrap();
    let pos_d = result.order.iter().position(|m| m == "mod_d").unwrap();
    assert!(pos_a < pos_b);
    assert!(pos_a < pos_c);
    assert!(pos_b < pos_d);
    assert!(pos_c < pos_d);
}

#[test]
fn test_resolve_incompatible_one_disabled() {
    // mod_a and mod_b are incompatible, but mod_b is disabled -> should succeed
    let profile = make_profile(
        vec![("mod_a", true), ("mod_b", false)],
        smallvec![LoadOrderRule::Incompatible {
            mod_a: ModId::from("mod_a"),
            mod_b: ModId::from("mod_b"),
        }],
    );
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 1);
    assert_eq!(result.order[0], "mod_a");
}

#[test]
fn test_resolve_many_mods_no_rules() {
    let mods: Vec<(&str, bool)> = (0..100)
        .map(|i| {
            // Leak the string so we get &'static str
            let s: &str = Box::leak(format!("mod_{i}").into_boxed_str());
            (s, true)
        })
        .collect();
    let profile = make_profile(mods, smallvec![]);
    let result = resolve(&profile).unwrap();
    assert_eq!(result.order.len(), 100);
    // All mods should appear
    for i in 0..100 {
        let name = ModId::from(format!("mod_{i}").as_str());
        assert!(result.order.contains(&name), "mod_{i} not found in order");
    }
}

#[test]
fn test_conflict_map_no_conflicts() {
    let mut cm = ConflictMap::default();
    cm.register("textures/sky.dds".to_string(), ModId::from("mod_a"));
    cm.register("meshes/tree.nif".to_string(), ModId::from("mod_b"));
    assert_eq!(
        cm.conflicts(),
        [] as [(&str, &std::collections::HashSet<modde_core::ModId>); 0]
    );
}

#[test]
fn test_conflict_map_many_providers() {
    let mut cm = ConflictMap::default();
    for i in 0..5 {
        cm.register(
            "shared_file.dds".to_string(),
            ModId::from(format!("mod_{i}").as_str()),
        );
    }
    let conflicts = cm.conflicts();
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].1.len(), 5);
}

#[test]
fn test_conflict_map_empty() {
    let cm = ConflictMap::default();
    assert_eq!(
        cm.conflicts(),
        [] as [(&str, &std::collections::HashSet<modde_core::ModId>); 0]
    );
}
