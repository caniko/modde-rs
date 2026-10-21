use super::*;

#[test]
fn goverlay_release_classification_maps_supported_channels() {
    assert_eq!(
        goverlay_release_channel("edge-0.9.12.0323"),
        Some("goverlay-edge")
    );
    assert_eq!(
        goverlay_release_channel("master-3ce61922"),
        Some("goverlay-master")
    );
    assert_eq!(
        goverlay_release_channel("any-release-0.9-2083b274"),
        Some("goverlay-any")
    );
    assert_eq!(goverlay_release_channel("0.9.1-0"), Some("goverlay-stable"));
    assert_eq!(goverlay_release_channel("fsr-int8"), None);
}

#[test]
fn goverlay_release_summary_requires_full_install_archive() {
    let edge = release_fixture("edge-0.9.12.0323", &["notes.json", "optiscaler-edge.7z"]);
    let edge = goverlay_release_summary(edge).expect("edge release");
    assert_eq!(edge.tag, "goverlay-edge:edge-0.9.12.0323");
    assert_eq!(edge.assets.len(), 1);
    assert_eq!(edge.assets[0].name, "optiscaler-edge.7z");

    let master = release_fixture(
        "master-3ce61922",
        &["master-3ce61922.json", "OptiScaler_master_3ce61922.7z"],
    );
    let master = goverlay_release_summary(master).expect("master release");
    assert_eq!(master.tag, "goverlay-master:master-3ce61922");
    assert_eq!(master.assets[0].name, "OptiScaler_master_3ce61922.7z");

    let fsr_int8 = release_fixture("fsr-int8", &["amd_fidelityfx_upscaler_dx12.dll"]);
    assert!(goverlay_release_summary(fsr_int8).is_none());
}

#[test]
fn optiscaler_release_tags_are_encoded_by_source() {
    assert_eq!(
        encode_optiscaler_release_tag("official", "v0.9.1"),
        "official:v0.9.1"
    );
    assert_eq!(
        normalize_optiscaler_release_tag("v0.9.1"),
        "official:v0.9.1"
    );
    assert_eq!(
        normalize_optiscaler_release_tag("goverlay-edge:edge-0.9.12.0323"),
        "goverlay-edge:edge-0.9.12.0323"
    );
}

#[test]
fn optiscaler_release_asset_selection_uses_encoded_source_keys() {
    let releases = vec![
        release_fixture("official:v0.9.1", &["Optiscaler_0.9.1-final.7z"]),
        release_fixture("goverlay-edge:edge-0.9.12.0323", &["optiscaler-edge.7z"]),
    ];

    let (tag, asset) =
        select_optiscaler_release_asset(&releases, "v0.9.1", "Optiscaler_0.9.1-final.7z")
            .expect("legacy official tag resolves");
    assert_eq!(tag, "official:v0.9.1");
    assert_eq!(asset.name, "Optiscaler_0.9.1-final.7z");

    let (tag, asset) = select_optiscaler_release_asset(
        &releases,
        "goverlay-edge:edge-0.9.12.0323",
        "optiscaler-edge.7z",
    )
    .expect("goverlay edge tag resolves");
    assert_eq!(tag, "goverlay-edge:edge-0.9.12.0323");
    assert_eq!(asset.name, "optiscaler-edge.7z");

    assert!(
        select_optiscaler_release_asset(&releases, "edge-0.9.12.0323", "optiscaler-edge.7z")
            .is_err()
    );
}

#[test]
fn optiscaler_release_config_moves_legacy_goverlay_tag_to_goverlay_source() {
    let mut config = OptiScaler.default_config();
    config.set("source_mode", serde_json::json!("github_release"));
    config.set(
        "release_tag",
        serde_json::json!("goverlay-edge:edge-0.9.12.0323"),
    );

    assert!(normalize_optiscaler_release_config(&mut config));

    assert_eq!(config.get_str("source_mode"), Some("goverlay_builds"));
    assert_eq!(config.get_str("goverlay_channel"), Some("edge"));
    assert_eq!(
        config.get_str("release_tag"),
        Some("goverlay-edge:edge-0.9.12.0323")
    );
}

#[test]
fn optiscaler_release_matching_respects_source_and_channel() {
    let official = release_fixture("official:v0.9.1", &["Optiscaler.7z"]);
    let edge = release_fixture("goverlay-edge:edge-0.9.12.0323", &["optiscaler-edge.7z"]);
    let master = release_fixture(
        "goverlay-master:master-3ce61922",
        &["OptiScaler_master_3ce61922.7z"],
    );
    let mut config = OptiScaler.default_config();

    config.set("source_mode", serde_json::json!("github_release"));
    assert!(optiscaler_release_matches_config(&official, &config));
    assert!(!optiscaler_release_matches_config(&edge, &config));

    config.set("source_mode", serde_json::json!("goverlay_builds"));
    config.set("goverlay_channel", serde_json::json!("edge"));
    assert!(optiscaler_release_matches_config(&edge, &config));
    assert!(!optiscaler_release_matches_config(&master, &config));

    config.set("goverlay_channel", serde_json::json!("master"));
    assert!(!optiscaler_release_matches_config(&edge, &config));
    assert!(optiscaler_release_matches_config(&master, &config));
}

#[test]
fn goverlay_release_sorts_newest_first_by_publish_date() {
    let mut releases = [
        release_fixture_with_date(
            "goverlay-edge:edge-old",
            &["optiscaler-edge.7z"],
            "2026-03-20T01:08:18Z",
        ),
        release_fixture_with_date(
            "goverlay-edge:edge-new",
            &["optiscaler-edge.7z"],
            "2026-03-24T00:18:25Z",
        ),
    ];

    releases.sort_by(|left, right| right.published_at.cmp(&left.published_at));

    assert_eq!(releases[0].tag, "goverlay-edge:edge-new");
    assert_eq!(releases[1].tag, "goverlay-edge:edge-old");
}

#[test]
fn goverlay_stable_tags_are_plain_three_part_versions() {
    for tag in ["0.9.1", "0.9.1-12"] {
        assert!(is_goverlay_stable_tag(tag), "{tag} should be stable");
    }
    for tag in ["", "0.9", "0.9.1.2", "0.9.1-", "0.9.1-rc1", "v0.9.1", "0..1"] {
        assert!(!is_goverlay_stable_tag(tag), "{tag} should not be stable");
    }
}

#[test]
fn installable_release_assets_are_archives() {
    assert!(is_installable_release_asset("OptiScaler.ZIP"));
    assert!(is_installable_release_asset("a.7z"));
    assert!(!is_installable_release_asset("a.json"));
    assert!(!is_installable_release_asset("zip"));
}

#[test]
fn bare_release_tag_normalizes_to_official_once() {
    let mut config = ToolConfig::new("optiscaler");
    config.set("release_tag", serde_json::json!("v0.9.1"));

    assert!(normalize_optiscaler_release_config(&mut config));
    assert_eq!(config.get_str("release_tag"), Some("official:v0.9.1"));
    assert_eq!(config.get_str("source_mode"), Some(OPTISCALER_SOURCE_OFFICIAL));
    assert!(!normalize_optiscaler_release_config(&mut config));
}
