use super::*;
use flate2::Compression;
use flate2::write::ZlibEncoder;
use std::io::Write;

fn plugin(record_flags: u32, masters: &[&str], records: Vec<Vec<u8>>) -> Vec<u8> {
    let mut tes4 = Vec::new();
    tes4.extend(subrecord(b"HEDR", &[0; 12]));
    for master in masters {
        let mut body = master.as_bytes().to_vec();
        body.push(0);
        tes4.extend(subrecord(b"MAST", &body));
        tes4.extend(subrecord(b"DATA", &[0; 8]));
    }

    let mut bytes = record(b"TES4", record_flags, 0, tes4);
    for record in records {
        bytes.extend(record);
    }
    bytes
}

fn record(sig: &[u8; 4], flags: u32, form_id: u32, data: Vec<u8>) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(sig);
    bytes.extend(&(data.len() as u32).to_le_bytes());
    bytes.extend(&flags.to_le_bytes());
    bytes.extend(&form_id.to_le_bytes());
    bytes.extend(&0u32.to_le_bytes());
    bytes.extend(&44u16.to_le_bytes());
    bytes.extend(&0u16.to_le_bytes());
    bytes.extend(data);
    bytes
}

fn compressed_record(sig: &[u8; 4], form_id: u32, data: Vec<u8>) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&data).unwrap();
    let compressed = encoder.finish().unwrap();
    let mut payload = Vec::new();
    payload.extend(&(data.len() as u32).to_le_bytes());
    payload.extend(compressed);
    record(sig, COMPRESSED_RECORD_FLAG, form_id, payload)
}

fn group(records: Vec<Vec<u8>>) -> Vec<u8> {
    let data_len: usize = records.iter().map(Vec::len).sum();
    let size = 24 + data_len;
    let mut bytes = Vec::new();
    bytes.extend(b"GRUP");
    bytes.extend(&(size as u32).to_le_bytes());
    bytes.extend(b"WRLD");
    bytes.extend(&0u32.to_le_bytes());
    bytes.extend(&0u32.to_le_bytes());
    bytes.extend(&0u32.to_le_bytes());
    for record in records {
        bytes.extend(record);
    }
    bytes
}

fn subrecord(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(sig);
    bytes.extend(&(body.len() as u16).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn extended_subrecord(sig: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend(b"XXXX");
    bytes.extend(&4u16.to_le_bytes());
    bytes.extend(&(body.len() as u32).to_le_bytes());
    bytes.extend(sig);
    bytes.extend(&0u16.to_le_bytes());
    bytes.extend(body);
    bytes
}

fn name_ref(form_id: u32) -> Vec<u8> {
    subrecord(b"NAME", &form_id.to_le_bytes())
}

fn cnto_ref(form_id: u32, count: u32) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(&form_id.to_le_bytes());
    body.extend(&count.to_le_bytes());
    subrecord(b"CNTO", &body)
}

fn cnam_ref(form_id: u32) -> Vec<u8> {
    subrecord(b"CNAM", &form_id.to_le_bytes())
}

#[test]
fn parses_tes4_header_and_masters() {
    let bytes = plugin(0, &["Skyrim.esm", "Update.esm"], Vec::new());
    let doc = parse_plugin_bytes("Test.esp", &bytes).unwrap();
    assert_eq!(doc.masters, vec!["Skyrim.esm", "Update.esm"]);
    assert!(doc.records.is_empty());
}

#[test]
fn resolves_reference_to_same_plugin() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"CONT", 0, 0x0100_0800, Vec::new());
    let reference = record(b"REFR", 0, 0x0100_0801, name_ref(0x0100_0800));
    std::fs::write(
        tmp.path().join("Self.esp"),
        plugin(0, &["Skyrim.esm"], vec![target, reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Self.esp"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn reports_unresolved_reference_in_present_master() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Skyrim.esm"),
        plugin(plugin_header::flags::ESM, &[], Vec::new()),
    )
    .unwrap();
    let reference = record(b"REFR", 0, 0x0100_0800, name_ref(0x0000_1234));
    std::fs::write(
        tmp.path().join("Broken.esp"),
        plugin(0, &["Skyrim.esm"], vec![reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Skyrim.esm", "Broken.esp"], "skyrim-se");
    assert_eq!(report.unresolved.len(), 1);
    assert_eq!(
        report.unresolved[0].expected_plugin.as_deref(),
        Some("Skyrim.esm")
    );
    assert_eq!(report.unresolved[0].subrecord, "NAME");
}

#[test]
fn disabled_plugin_is_not_a_valid_target() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"CONT", 0, 0x0000_0800, Vec::new());
    std::fs::write(
        tmp.path().join("Disabled.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![target]),
    )
    .unwrap();
    let reference = record(b"REFR", 0, 0x0100_0800, name_ref(0x0000_0800));
    std::fs::write(
        tmp.path().join("Broken.esp"),
        plugin(0, &["Disabled.esm"], vec![reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Broken.esp"], "skyrim-se");
    assert_eq!(report.unresolved.len(), 1);
    assert_eq!(
        report.unresolved[0].expected_plugin.as_deref(),
        Some("Disabled.esm")
    );
}

#[test]
fn parses_nested_group_records() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"CONT", 0, 0x0000_0800, Vec::new());
    let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0000_0800));
    std::fs::write(
        tmp.path().join("Grouped.esm"),
        plugin(
            plugin_header::flags::ESM,
            &[],
            vec![group(vec![group(vec![target, reference])])],
        ),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Grouped.esm"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn parses_compressed_record_references() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"CONT", 0, 0x0000_0800, Vec::new());
    let reference = compressed_record(b"REFR", 0x0000_0801, name_ref(0x0000_0800));
    std::fs::write(
        tmp.path().join("Compressed.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![target, reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Compressed.esm"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn parses_extended_subrecord_sizes() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"MISC", 0, 0x0000_0800, Vec::new());
    let mut body = Vec::new();
    body.extend(&0x0000_0800u32.to_le_bytes());
    body.extend(&1u32.to_le_bytes());
    let reference = record(b"CONT", 0, 0x0000_0801, extended_subrecord(b"CNTO", &body));
    std::fs::write(
        tmp.path().join("Extended.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![target, reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Extended.esm"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn malformed_plugin_reports_parse_failure() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("Bad.esp"), b"TES4").unwrap();
    let report = validate_record_references(tmp.path(), &["Bad.esp"], "skyrim-se");
    assert_eq!(report.parse_errors.len(), 1);
}

#[test]
fn validates_light_plugin_compact_ids_conservatively() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"MISC", 0, 0x0000_0800, Vec::new());
    let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0000_0800));
    std::fs::write(
        tmp.path().join("Light.esl"),
        plugin(plugin_header::flags::ESL, &[], vec![target, reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Light.esl"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn validates_esl_flagged_esp_compact_ids_conservatively() {
    let tmp = tempfile::tempdir().unwrap();
    let target = record(b"MISC", 0, 0x0000_0800, Vec::new());
    let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0000_0800));
    std::fs::write(
        tmp.path().join("LightFlagged.esp"),
        plugin(plugin_header::flags::ESL, &[], vec![target, reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["LightFlagged.esp"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn container_item_reference_uses_cnto_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let item = record(b"MISC", 0, 0x0000_0800, Vec::new());
    let container = record(b"CONT", 0, 0x0000_0801, cnto_ref(0x0000_0800, 3));
    std::fs::write(
        tmp.path().join("Container.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![item, container]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Container.esm"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn weapon_reference_uses_cnam_schema() {
    let tmp = tempfile::tempdir().unwrap();
    let enchantment = record(b"ENCH", 0, 0x0000_0800, Vec::new());
    let weapon = record(b"WEAP", 0, 0x0000_0801, cnam_ref(0x0000_0800));
    std::fs::write(
        tmp.path().join("Weapon.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![enchantment, weapon]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Weapon.esm"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
}

#[test]
fn out_of_range_mod_index_has_specific_diagnostic() {
    let tmp = tempfile::tempdir().unwrap();
    let reference = record(b"REFR", 0, 0x0000_0801, name_ref(0x0200_0800));
    std::fs::write(
        tmp.path().join("Broken.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![reference]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Broken.esm"], "skyrim-se");
    assert_eq!(report.unresolved.len(), 1);
    assert!(report.unresolved[0].mod_index_out_of_range);
    assert!(
        report.unresolved[0]
            .to_string()
            .contains("mod index beyond the master list")
    );
}

#[test]
fn unsupported_record_types_are_reported_without_failing_clean_validation() {
    let tmp = tempfile::tempdir().unwrap();
    let armor = record(b"ARMO", 0, 0x0000_0800, Vec::new());
    std::fs::write(
        tmp.path().join("Armor.esm"),
        plugin(plugin_header::flags::ESM, &[], vec![armor]),
    )
    .unwrap();

    let report = validate_record_references(tmp.path(), &["Armor.esm"], "skyrim-se");
    assert!(report.is_empty(), "{report:?}");
    assert_eq!(report.unsupported_record_types, ["ARMO"]);
}

#[test]
fn parser_rejects_malformed_headers_groups_and_payloads() {
    let err = |result: Result<PluginDocument>| result.unwrap_err().to_string();
    assert!(
        err(parse_plugin_bytes("A.esp", &record(b"WEAP", 0, 1, Vec::new())))
            .contains("expected TES4")
    );

    for (size, message) in [(23u32, "invalid GRUP size"), (100, "extends past")] {
        let mut bytes = plugin(0, &[], Vec::new());
        let mut bad_group = group(Vec::new());
        bad_group[4..8].copy_from_slice(&size.to_le_bytes());
        bytes.extend(bad_group);
        assert!(err(parse_plugin_bytes("A.esp", &bytes)).contains(message));
    }

    assert!(decompress_record(&[1, 0, 0]).is_err());
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&[1, 2, 3]).unwrap();
    let mut payload = 99u32.to_le_bytes().to_vec();
    payload.extend(encoder.finish().unwrap());
    assert!(
        decompress_record(&payload)
            .unwrap_err()
            .to_string()
            .contains("size mismatch")
    );

    assert!(
        parse_masters(&subrecord(b"XXXX", &[0; 2]))
            .unwrap_err()
            .to_string()
            .contains("invalid XXXX size")
    );
}

#[test]
fn esl_extension_or_flag_marks_plugin_light() {
    let light = |name: &str, flags: u32| {
        parse_plugin_bytes(name, &plugin(flags, &[], Vec::new()))
            .unwrap()
            .is_light
    };
    assert!(light("A.ESL", 0));
    assert!(light("A.esp", ESL_FLAG));
    assert!(!light("A.esp", 0));
}

#[test]
fn bethesda_plugin_extensions_are_case_insensitive() {
    for name in ["A.ESP", "b.esm", "c.Esl"] {
        assert!(is_bethesda_plugin(name), "{name}");
    }
    for name in ["a.esp.bak", "esp", "a.bsa"] {
        assert!(!is_bethesda_plugin(name), "{name}");
    }
}

#[test]
fn reference_compactness_comes_from_light_map_then_extension() {
    let source = PluginDocument {
        filename: "Mod.esp".to_string(),
        masters: vec!["Light.esm".to_string(), "Dlc.esl".to_string()],
        is_light: false,
        records: Vec::new(),
    };
    let active: HashMap<String, String> = ["Light.esm", "Dlc.esl", "Mod.esp"]
        .into_iter()
        .map(|name| (name.to_lowercase(), name.to_string()))
        .collect();
    let light = HashMap::from([("light.esm".to_string(), true)]);
    let key = |form_id| {
        resolve_reference(&source, form_id, &active, &light)
            .key
            .unwrap()
    };

    let from_map = key(0x0012_3456);
    assert!(from_map.compact);
    assert_eq!(from_map.object_id, 0x456);
    assert!(key(0x0100_0abc).compact);
    let own = key(0x0212_3456);
    assert!(!own.compact);
    assert_eq!((own.plugin.as_str(), own.object_id), ("mod.esp", 0x12_3456));

    let missing = resolve_reference(&source, 0x0300_0001, &active, &light);
    assert!(missing.mod_index_out_of_range && missing.key.is_none());
}
