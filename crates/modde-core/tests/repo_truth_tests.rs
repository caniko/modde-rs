use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CapabilityMatrix {
    games: BTreeMap<String, GameCapability>,
    features: BTreeMap<String, FeatureCapability>,
}

#[derive(Debug, Deserialize)]
struct GameCapability {
    display_name: String,
    overall: String,
    save_tracking: String,
}

#[derive(Debug, Deserialize)]
struct FeatureCapability {
    label: String,
    status: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root should exist")
}

fn read_repo_file(relative: &str) -> String {
    fs::read_to_string(repo_root().join(relative))
        .unwrap_or_else(|err| panic!("failed to read {relative}: {err}"))
}

fn load_capability_matrix() -> CapabilityMatrix {
    toml::from_str(&read_repo_file("docs/capability-matrix.toml"))
        .expect("capability matrix should parse")
}

fn assert_contains(haystack: &str, needle: &str, context: &str) {
    assert!(
        haystack.contains(needle),
        "{context} should contain `{needle}`, but it did not"
    );
}

#[test]
fn capability_matrix_captures_the_expected_baseline() {
    let matrix = load_capability_matrix();

    let starfield = matrix
        .games
        .get("starfield")
        .expect("Starfield entry should exist");
    assert_eq!(starfield.overall, "Partial");
    assert_eq!(starfield.save_tracking, "Done");

    let downloads = matrix
        .features
        .get("downloads_ui")
        .expect("Downloads UI entry should exist");
    assert_eq!(downloads.status, "Partial");

    let generic = matrix
        .features
        .get("generic_game_support")
        .expect("Generic game entry should exist");
    assert_eq!(generic.status, "Partial");

    let executables = matrix
        .features
        .get("executable_management")
        .expect("Executable management entry should exist");
    assert_eq!(executables.status, "Done");

    let mod_info = matrix
        .features
        .get("mod_info_dialog")
        .expect("Mod information dialog entry should exist");
    assert_eq!(mod_info.status, "Partial");

    let hot_deploy = matrix
        .features
        .get("hot_deploy")
        .expect("Hot-deploy entry should exist");
    assert_eq!(hot_deploy.status, "Partial");

    for feature_id in ["owned_game_library", "sandboxed_launch"] {
        let feature = matrix
            .features
            .get(feature_id)
            .expect("launch capability entry should exist");
        assert_eq!(
            feature.status, "Partial",
            "{feature_id} still requires live qualification"
        );
    }
}

#[test]
fn public_docs_match_capability_matrix_for_critical_statuses() {
    let matrix = load_capability_matrix();
    let readme = read_repo_file("README.md");
    // The MO2 parity audit is published as an mdBook page (it used to live at
    // docs/mo2-coverage.md). The capability table in that page must agree with
    // the canonical matrix.
    let coverage = read_repo_file("docs/src/reference/parity.md");
    let supported_games = read_repo_file("docs/src/games/supported-games.md");
    let comparison = read_repo_file("website/plinth-project.toml");

    let starfield = matrix.games.get("starfield").unwrap();
    assert_contains(
        &readme,
        &format!("| {} | `{}`:", starfield.display_name, starfield.overall),
        "README supported games table",
    );
    assert_contains(
        &readme,
        "docs/capability-matrix.toml",
        "README capability note",
    );
    assert_contains(
        &supported_games,
        &format!(
            "| {} | `starfield` | `{}` | Yes | Yes | `{}`",
            starfield.display_name, starfield.overall, starfield.save_tracking
        ),
        "docs site supported games table",
    );
    assert_contains(
        &supported_games,
        "no contamination gate",
        "Starfield supported games row",
    );
    assert_contains(
        &coverage,
        "record-level save-contamination removal gate is not implemented",
        "MO2 coverage audit",
    );

    for feature_id in [
        "instance_switching",
        "bethesda_plugin_management",
        "diagnostics",
        "downloads_ui",
        "tool_management",
        "executable_management",
        "hot_deploy",
        "mod_info_dialog",
        "bain",
        "generic_game_support",
        "starfield_save_tracking",
        "owned_game_library",
        "sandboxed_launch",
    ] {
        let feature = matrix
            .features
            .get(feature_id)
            .unwrap_or_else(|| panic!("missing feature entry: {feature_id}"));
        assert_contains(
            &coverage,
            &format!("| {} | `{}` |", feature.label, feature.status),
            "MO2 coverage audit",
        );
    }

    assert_contains(
        &comparison,
        "area = \"Data Files / Diagnostics / Tools / Downloads views\"\nstatus = \"Partial\"",
        "website comparison table",
    );
    assert_contains(
        &comparison,
        "type = \"capability_matrix\"",
        "website comparison table",
    );
}

#[test]
fn public_installation_docs_do_not_market_staged_channels_as_live() {
    let readme = read_repo_file("README.md");
    let installation = read_repo_file("docs/src/getting-started/installation.md");
    let contributing = read_repo_file("CONTRIBUTING.md");

    for (label, docs) in [
        ("README", readme.as_str()),
        ("installation guide", installation.as_str()),
        ("contributing guide", contributing.as_str()),
    ] {
        assert_contains(docs, "Nix", label);
    }

    assert_contains(
        &readme,
        "flake/Home-Manager path is live",
        "README install status",
    );
    assert_contains(
        &installation,
        "Nix/Home Manager is live",
        "installation guide channel status",
    );
    assert_contains(
        &installation,
        "package families below are staged release wiring",
        "installation guide channel status",
    );
    assert_contains(
        &contributing,
        "Only the Nix flake, home-manager module, and the Attic cache are live",
        "contributing distribution table",
    );
}
