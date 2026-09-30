#![cfg(feature = "bethesda")]

use std::path::PathBuf;

use modde_core::diagnostics::{DiagContext, DiagnosticRule, Severity};
use modde_core::profile::{EnabledMod, Profile, ProfileSource};
use modde_core::resolver::{ConflictMap, GameId};
use smallvec::smallvec;

fn make_profile(game_id: &str, mods: Vec<EnabledMod>, overrides: PathBuf) -> Profile {
    Profile {
        id: None,
        name: "test-profile".to_string(),
        game_id: GameId::from(game_id),
        source: ProfileSource::Manual,
        mods,
        overrides,
        load_order_rules: smallvec![],
        load_order_lock: None,
    }
}

fn enabled_mod(id: &str) -> EnabledMod {
    EnabledMod {
        mod_id: id.to_string(),
        enabled: true,
        version: None,
        fomod_config: None,
        ..Default::default()
    }
}

#[test]
fn test_empty_mod_rule() {
    use modde_games::bethesda::diagnostics::EmptyModRule;

    let store = tempfile::tempdir().unwrap();
    let staging = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    // Create one mod directory with files, one empty, one missing
    let mod_with_files = store.path().join("mod-with-files");
    std::fs::create_dir_all(&mod_with_files).unwrap();
    std::fs::write(mod_with_files.join("texture.dds"), b"fake texture").unwrap();

    let mod_empty = store.path().join("mod-empty");
    std::fs::create_dir_all(&mod_empty).unwrap();
    // mod-missing doesn't get a directory at all

    let profile = make_profile(
        "skyrim-se",
        vec![
            enabled_mod("mod-with-files"),
            enabled_mod("mod-empty"),
            enabled_mod("mod-missing"),
        ],
        overrides.path().to_path_buf(),
    );
    let conflict_map = ConflictMap::default();

    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let rule = EmptyModRule;
    let diagnostics = rule.check(&ctx);

    // mod-empty and mod-missing should produce diagnostics
    assert_eq!(
        diagnostics.len(),
        2,
        "expected 2 empty mod diagnostics, got {}",
        diagnostics.len()
    );
    assert!(diagnostics.iter().all(|d| d.severity == Severity::Warning));
    assert!(
        diagnostics
            .iter()
            .any(|d| d.affected_mod.as_deref() == Some("mod-empty"))
    );
    assert!(
        diagnostics
            .iter()
            .any(|d| d.affected_mod.as_deref() == Some("mod-missing"))
    );
}

/// Build a minimal TES4 record for testing (same pattern as `plugin_header.rs` tests).
fn build_test_plugin(version: f32, masters: &[&str], record_flags: u32) -> Vec<u8> {
    let mut data = Vec::new();
    let mut sub_records = Vec::new();

    // HEDR sub-record: version(f32) + numRecords(u32) + nextObjectId(u32) = 12 bytes
    sub_records.extend_from_slice(b"HEDR");
    sub_records.extend_from_slice(&12u16.to_le_bytes());
    sub_records.extend_from_slice(&version.to_le_bytes());
    sub_records.extend_from_slice(&100u32.to_le_bytes());
    sub_records.extend_from_slice(&0x800u32.to_le_bytes());

    // MAST sub-records
    for master in masters {
        let name_bytes = master.as_bytes();
        let sub_size = (name_bytes.len() + 1) as u16;
        sub_records.extend_from_slice(b"MAST");
        sub_records.extend_from_slice(&sub_size.to_le_bytes());
        sub_records.extend_from_slice(name_bytes);
        sub_records.push(0); // null terminator

        // DATA sub-record (8 bytes, required after each MAST)
        sub_records.extend_from_slice(b"DATA");
        sub_records.extend_from_slice(&8u16.to_le_bytes());
        sub_records.extend_from_slice(&0u64.to_le_bytes());
    }

    // TES4 record header
    data.extend_from_slice(b"TES4");
    data.extend_from_slice(&(sub_records.len() as u32).to_le_bytes());
    data.extend_from_slice(&record_flags.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes()); // form ID
    data.extend_from_slice(&0u32.to_le_bytes()); // revision
    data.extend_from_slice(&44u16.to_le_bytes()); // version
    data.extend_from_slice(&0u16.to_le_bytes()); // unknown
    data.extend_from_slice(&sub_records);
    data
}

fn build_record(sig: &[u8; 4], form_id: u32, subrecords: Vec<u8>) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(sig);
    data.extend_from_slice(&(subrecords.len() as u32).to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&form_id.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&44u16.to_le_bytes());
    data.extend_from_slice(&0u16.to_le_bytes());
    data.extend_from_slice(&subrecords);
    data
}

fn build_formid_subrecord(sig: &[u8; 4], form_id: u32) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(sig);
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&form_id.to_le_bytes());
    data
}

fn build_test_plugin_with_records(
    masters: &[&str],
    record_flags: u32,
    records: Vec<Vec<u8>>,
) -> Vec<u8> {
    let mut data = build_test_plugin(1.70, masters, record_flags);
    for record in records {
        data.extend(record);
    }
    data
}

#[test]
fn test_form43_and_missing_master() {
    use modde_games::bethesda::diagnostics::{Form43Rule, MissingMasterRule};

    let staging = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    // Create a Form 43 plugin that depends on a missing master
    let form43_data = build_test_plugin(0.94, &["Skyrim.esm", "MissingMod.esp"], 0);
    std::fs::write(staging.path().join("OldMod.esp"), &form43_data).unwrap();

    // Create Skyrim.esm so it doesn't count as missing
    let esm_data = build_test_plugin(1.70, &[], 0x0000_0001); // ESM flag
    std::fs::write(staging.path().join("Skyrim.esm"), &esm_data).unwrap();

    // Profile with plugins as mod IDs (the collect_active_plugins helper filters by extension)
    let profile = make_profile(
        "skyrim-se",
        vec![enabled_mod("Skyrim.esm"), enabled_mod("OldMod.esp")],
        overrides.path().to_path_buf(),
    );
    let conflict_map = ConflictMap::default();
    let active_plugins = vec!["Skyrim.esm".to_string(), "OldMod.esp".to_string()];

    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &active_plugins,
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    // Test Form43Rule
    let form43_rule = Form43Rule;
    let form43_diags = form43_rule.check(&ctx);
    assert_eq!(
        form43_diags.len(),
        1,
        "expected 1 Form 43 diagnostic, got {}",
        form43_diags.len()
    );
    assert_eq!(form43_diags[0].severity, Severity::Warning);
    assert!(form43_diags[0].title.contains("OldMod.esp"));

    // Test MissingMasterRule
    let master_rule = MissingMasterRule;
    let master_diags = master_rule.check(&ctx);
    assert_eq!(
        master_diags.len(),
        1,
        "expected 1 missing master diagnostic, got {}",
        master_diags.len()
    );
    assert_eq!(master_diags[0].severity, Severity::Error);
    assert!(master_diags[0].title.contains("MissingMod.esp"));
}

#[test]
fn test_record_reference_rule_reports_unresolved_formid() {
    use modde_games::bethesda::diagnostics::RecordReferenceRule;

    let staging = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    std::fs::write(
        staging.path().join("Skyrim.esm"),
        build_test_plugin(1.70, &[], 0x0000_0001),
    )
    .unwrap();
    let broken_ref = build_record(
        b"REFR",
        0x0100_0800,
        build_formid_subrecord(b"NAME", 0x0000_1234),
    );
    std::fs::write(
        staging.path().join("Broken.esp"),
        build_test_plugin_with_records(&["Skyrim.esm"], 0, vec![broken_ref]),
    )
    .unwrap();

    let profile = make_profile(
        "skyrim-se",
        vec![enabled_mod("Skyrim.esm"), enabled_mod("Broken.esp")],
        overrides.path().to_path_buf(),
    );
    let conflict_map = ConflictMap::default();
    let active_plugins = vec!["Skyrim.esm".to_string(), "Broken.esp".to_string()];
    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &active_plugins,
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let diags = RecordReferenceRule.check(&ctx);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Error);
    assert!(diags[0].title.contains("00001234"));
    assert!(diags[0].detail.contains("Broken.esp"));
    assert!(diags[0].detail.contains("REFR"));
    assert!(diags[0].detail.contains("Skyrim.esm"));
}

#[test]
fn test_bethesda_engine_includes_record_reference_rule() {
    use modde_games::bethesda::diagnostics::bethesda_diagnostics;

    let staging = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    std::fs::write(
        staging.path().join("Skyrim.esm"),
        build_test_plugin(1.70, &[], 0x0000_0001),
    )
    .unwrap();
    let broken_ref = build_record(
        b"REFR",
        0x0100_0800,
        build_formid_subrecord(b"NAME", 0x0000_4321),
    );
    std::fs::write(
        staging.path().join("Broken.esp"),
        build_test_plugin_with_records(&["Skyrim.esm"], 0, vec![broken_ref]),
    )
    .unwrap();

    let profile = make_profile(
        "skyrim-se",
        vec![enabled_mod("Skyrim.esm"), enabled_mod("Broken.esp")],
        overrides.path().to_path_buf(),
    );
    let conflict_map = ConflictMap::default();
    let active_plugins = vec!["Skyrim.esm".to_string(), "Broken.esp".to_string()];
    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &active_plugins,
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let diagnostics = bethesda_diagnostics().run_all(&ctx);
    assert!(
        diagnostics
            .iter()
            .any(|diag| diag.title.contains("00004321")),
        "expected unresolved FormID diagnostic, got {diagnostics:?}"
    );
}

#[test]
fn test_record_reference_rule_has_no_findings_without_plugins_for_ce_games() {
    use modde_games::bethesda::diagnostics::RecordReferenceRule;

    for game_id in [
        "skyrim-se",
        "skyrim-ae",
        "fallout4",
        "fallout76",
        "starfield",
    ] {
        let staging = tempfile::tempdir().unwrap();
        let store = tempfile::tempdir().unwrap();
        let overrides = tempfile::tempdir().unwrap();
        let profile = make_profile(game_id, vec![], overrides.path().to_path_buf());
        let conflict_map = ConflictMap::default();
        let ctx = DiagContext {
            game_id,
            profile: &profile,
            active_plugins: &[],
            conflict_map: &conflict_map,
            collision_report: None,
            store_dir: store.path(),
            staging_dir: staging.path(),
        };
        assert!(RecordReferenceRule.check(&ctx).is_empty(), "{game_id}");
    }
}

#[test]
fn test_form43_rule_skips_non_skyrim() {
    use modde_games::bethesda::diagnostics::Form43Rule;

    let staging = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    let form43_data = build_test_plugin(0.94, &[], 0);
    std::fs::write(staging.path().join("SomeMod.esp"), &form43_data).unwrap();

    let profile = make_profile(
        "fallout4",
        vec![enabled_mod("SomeMod.esp")],
        overrides.path().to_path_buf(),
    );
    let conflict_map = ConflictMap::default();
    let active_plugins = vec!["SomeMod.esp".to_string()];

    let ctx = DiagContext {
        game_id: "fallout4",
        profile: &profile,
        active_plugins: &active_plugins,
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let rule = Form43Rule;
    let diags = rule.check(&ctx);
    assert!(
        diags.is_empty(),
        "Form43Rule should not produce diagnostics for non-Skyrim games"
    );
}

#[test]
fn test_orphaned_overrides_rule() {
    use modde_games::bethesda::diagnostics::OrphanedOverridesRule;

    let staging = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    // Put a file in overrides
    std::fs::write(overrides.path().join("test.ini"), b"override content").unwrap();

    let profile = make_profile("skyrim-se", vec![], overrides.path().to_path_buf());
    let conflict_map = ConflictMap::default();

    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let rule = OrphanedOverridesRule;
    let diags = rule.check(&ctx);
    assert_eq!(diags.len(), 1);
    assert_eq!(diags[0].severity, Severity::Info);
}

#[test]
fn test_bethesda_diagnostics_engine() {
    use modde_games::bethesda::diagnostics::bethesda_diagnostics;

    let staging = tempfile::tempdir().unwrap();
    let store = tempfile::tempdir().unwrap();
    let overrides = tempfile::tempdir().unwrap();

    let profile = make_profile(
        "skyrim-se",
        vec![enabled_mod("empty-mod")],
        overrides.path().to_path_buf(),
    );
    let conflict_map = ConflictMap::default();

    let ctx = DiagContext {
        game_id: "skyrim-se",
        profile: &profile,
        active_plugins: &[],
        conflict_map: &conflict_map,
        collision_report: None,
        store_dir: store.path(),
        staging_dir: staging.path(),
    };

    let engine = bethesda_diagnostics();
    let results = engine.run_all(&ctx);

    // At minimum, the empty mod rule should fire
    assert!(
        results
            .iter()
            .any(|d| d.affected_mod.as_deref() == Some("empty-mod"))
    );
}
