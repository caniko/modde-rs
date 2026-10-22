use super::*;
use crate::profile::ProfileSource;
use crate::resolver::GameId;

fn enabled_mod(mod_id: &str, display_name: Option<&str>) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: display_name.map(str::to_string),
        enabled: true,
        ..EnabledMod::default()
    }
}

fn staged_file(rel_path: &str) -> StagedFile {
    StagedFile {
        rel_path: rel_path.to_string(),
        origin_rel_path: rel_path.to_string(),
        size: 1024,
        merge_group: None,
    }
}

/// Synthetic install state matching the fixture logs below:
/// - `broken-mod` ships the active plugin `BrokenMod.esp`
/// - `bad-skse-plugin` ships the native DLL `badplugin.dll`
fn correlation_input() -> CrashCorrelationInput {
    let profile = Profile {
        id: Some(1),
        name: "default".to_string(),
        game_id: GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods: vec![
            enabled_mod("broken-mod", Some("BrokenMod.esp")),
            enabled_mod("bad-skse-plugin", Some("Bad SKSE Plugin")),
            enabled_mod("innocent-mod", Some("Innocent Mod")),
        ],
        overrides: PathBuf::from("/tmp/overrides"),
        load_order_rules: Default::default(),
        load_order_lock: None,
    };
    CrashCorrelationInput {
        game_id: "skyrim-se".to_string(),
        profile,
        active_plugins: vec![
            PluginEntry {
                plugin_name: "Skyrim.esm".to_string(),
                sort_index: 0,
                enabled: true,
            },
            PluginEntry {
                plugin_name: "BrokenMod.esp".to_string(),
                sort_index: 1,
                enabled: true,
            },
        ],
        installed_files: vec![
            (
                "bad-skse-plugin".to_string(),
                staged_file("SKSE/Plugins/badplugin.dll"),
            ),
            ("broken-mod".to_string(), staged_file("BrokenMod.esp")),
        ],
        tool_files: Vec::new(),
    }
}

fn crash_logger_sse_fixture() -> &'static str {
    "Skyrim SSE v1.6.1170\n\
     CrashLoggerSSE v1-14-1-0\n\
     Unhandled exception \"EXCEPTION_ACCESS_VIOLATION\" at 0x7FF61D2A4B30\n\
     \n\
     PROBABLE CALL STACK:\n\
     \t[0] badplugin.dll+0012F30\n\
     \t[1] SkyrimSE.exe+05B11F0\n\
     \n\
     REGISTERS:\n\
     \tRAX 0x0\n\
     \n\
     STACK:\n\
     \t[RSP+0] 0x7FF61D2A4B30\n\
     \n\
     MODULES:\n\
     \tSkyrimSE.exe\n\
     \tbadplugin.dll\n\
     \n\
     SKSE PLUGINS:\n\
     \tbadplugin.dll v1.2.3\n\
     \n\
     PLUGINS:\n\
     \t[00] Skyrim.esm\n\
     \t[FE:000] BrokenMod.esp\n"
}

fn net_script_framework_fixture() -> &'static str {
    ".NET Script Framework version 18\n\
     Unhandled native exception occurred at 0x7FF61D2A4B30\n\
     \n\
     Possible relevant objects\n\
     [ 1] TESObjectREFR(FormId: 0x00012345, File: `BrokenMod.esp`)\n\
     \n\
     Probable callstack\n\
     [0] badplugin.dll+0012F30\n\
     [1] SkyrimSE.exe+05B11F0\n\
     \n\
     Modules\n\
     badplugin.dll\n\
     SkyrimSE.exe\n\
     \n\
     Plugins\n\
     [00] Skyrim.esm\n\
     [FE:000] BrokenMod.esp\n"
}

fn assert_correlation(report: &CrashCorrelationReport) {
    // The .esp suspect must be matched with highest-confidence evidence.
    let esp = report
        .suspects
        .iter()
        .find(|s| s.mod_id.as_deref() == Some("broken-mod"))
        .expect("BrokenMod.esp should be correlated to mod 'broken-mod'");
    assert_eq!(esp.confidence, CrashConfidence::Highest);
    assert_eq!(esp.plugin_name.as_deref(), Some("BrokenMod.esp"));
    assert!(
        esp.evidence.iter().any(|e| e.kind == CrashTokenKind::Plugin
            && e.reason == "exact active plugin name mentioned by log"),
        "expected exact-active-plugin evidence, got: {:?}",
        esp.evidence
    );

    // The DLL must be matched against the installed-files manifest.
    let dll = report
        .suspects
        .iter()
        .find(|s| s.mod_id.as_deref() == Some("bad-skse-plugin"))
        .expect("badplugin.dll should be correlated to mod 'bad-skse-plugin'");
    assert_eq!(dll.confidence, CrashConfidence::High);
    assert!(
        dll.evidence.iter().any(|e| e.kind == CrashTokenKind::Dll
            && e.token.eq_ignore_ascii_case("badplugin.dll")
            && e.reason.contains("installed file manifest")),
        "expected DLL manifest-match evidence, got: {:?}",
        dll.evidence
    );

    // Suspects are ranked: confidence is non-increasing down the list,
    // and the plugin suspect outranks the DLL suspect.
    assert!(
        report
            .suspects
            .windows(2)
            .all(|w| w[0].confidence >= w[1].confidence),
        "suspects must be sorted by descending confidence"
    );
    let esp_rank = report
        .suspects
        .iter()
        .position(|s| s.mod_id.as_deref() == Some("broken-mod"))
        .expect("esp suspect present");
    let dll_rank = report
        .suspects
        .iter()
        .position(|s| s.mod_id.as_deref() == Some("bad-skse-plugin"))
        .expect("dll suspect present");
    assert_eq!(esp_rank, 0, "highest-confidence suspect must rank first");
    assert!(esp_rank < dll_rank);
}

#[test]
fn crash_logger_sse_log_correlates_end_to_end() {
    let report = correlate_crash_log(
        Path::new("/tmp/crash-2026-06-12.log"),
        crash_logger_sse_fixture(),
        CrashLogFormat::Auto,
        correlation_input(),
    );
    assert_eq!(report.format, CrashLogFormat::CrashLoggerSse);
    assert_eq!(report.game_id, "skyrim-se");
    assert_eq!(report.profile_name, "default");
    assert_eq!(report.raw_sha256.len(), 64);
    assert!(
        report
            .signature
            .exception_line
            .as_deref()
            .is_some_and(|line| line.contains("EXCEPTION_ACCESS_VIOLATION"))
    );
    assert_correlation(&report);
}

#[test]
fn net_script_framework_log_autodetects_and_correlates() {
    let report = correlate_crash_log(
        Path::new("/tmp/Crash_2026_06_12.txt"),
        net_script_framework_fixture(),
        CrashLogFormat::Auto,
        correlation_input(),
    );
    assert_eq!(report.format, CrashLogFormat::NetScriptFramework);
    assert_correlation(&report);
}

#[test]
fn crashlogger_parser_uses_upstream_section_headers() {
    let log = "Skyrim SSE v1.6.1170\nCrashLoggerSSE v1-14-1-0\nUnhandled native exception occurred\n\nPROBABLE CALL STACK:\n\tSomeMod.dll+123\n\nREGISTERS:\n\nSTACK:\n\tData\\meshes\\foo\\bar.nif\n\nMODULES:\n\tSomeMod.dll\n\nSKSE PLUGINS:\n\tSomeMod.dll\n\nPLUGINS:\n\t[FE:000] Example.esp\n";
    let signature = parse_crash_log(log, CrashLogFormat::CrashLoggerSse);
    assert_eq!(signature.detected_format, CrashLogFormat::CrashLoggerSse);
    assert!(signature.tokens.iter().any(|t| t.value == "Example.esp"));
    assert!(signature.tokens.iter().any(|t| t.value == "SomeMod.dll"));
    assert!(
        signature
            .tokens
            .iter()
            .any(|t| t.value == "Data\\meshes\\foo\\bar.nif")
    );
}

#[test]
fn trainwreck_parser_uses_script_extender_plugins_header() {
    let log = "Trainwreck v1.4.0\nUnhandled native exception occurred\n\nPROBABLE CALL STACK:\n\tEngineFixes.dll+ABC\n\nREGISTERS:\n\nSTACK:\n\nMODULES:\n\tEngineFixes.dll\n\nSCRIPT EXTENDER PLUGINS:\n\tEngineFixes.dll\n";
    let signature = parse_crash_log(log, CrashLogFormat::Trainwreck);
    assert_eq!(signature.detected_format, CrashLogFormat::Trainwreck);
    assert!(
        signature
            .tokens
            .iter()
            .any(|t| t.section == "script-extender-plugins" && t.value == "EngineFixes.dll")
    );
}

#[test]
fn net_script_framework_parser_detects_sections() {
    let log = ".NET Script Framework v18\nUnhandled native exception\n\nPossible relevant objects\n[ 1] TESObjectREFR(FormId: 00012345, File: `Example.esp`)\n\nProbable callstack\nSomeMod.dll+123\n\nStack\nData\\textures\\foo.dds\n";
    let signature = parse_crash_log(log, CrashLogFormat::Auto);
    assert_eq!(
        signature.detected_format,
        CrashLogFormat::NetScriptFramework
    );
    assert!(signature.tokens.iter().any(|t| t.value == "Example.esp"));
    assert!(signature.tokens.iter().any(|t| t.value == "SomeMod.dll"));
    assert!(
        signature
            .tokens
            .iter()
            .any(|t| t.value == "Data\\textures\\foo.dds")
    );
}

#[test]
fn format_names_round_trip_and_headers_pick_the_parser() {
    for format in [
        CrashLogFormat::Auto,
        CrashLogFormat::CrashLoggerSse,
        CrashLogFormat::NetScriptFramework,
        CrashLogFormat::Trainwreck,
        CrashLogFormat::Generic,
    ] {
        assert_eq!(CrashLogFormat::parse(format.as_str()), Some(format));
    }
    assert_eq!(
        CrashLogFormat::parse("crashlogger"),
        Some(CrashLogFormat::CrashLoggerSse)
    );
    assert_eq!(CrashLogFormat::parse("minidump"), None);

    for (head, format) in [
        ("CrashLoggerSSE v1.2", CrashLogFormat::CrashLoggerSse),
        ("Trainwreck crash log", CrashLogFormat::Trainwreck),
        ("Unhandled exception", CrashLogFormat::Generic),
    ] {
        assert_eq!(detect_format(head), format, "{head}");
        assert_eq!(
            parse_crash_log(head, CrashLogFormat::Auto).detected_format,
            format
        );
    }
    assert_eq!(
        parse_crash_log("anything", CrashLogFormat::Trainwreck).detected_format,
        CrashLogFormat::Trainwreck
    );
    assert_eq!(CrashConfidence::Highest.to_string(), "Highest");
    assert_eq!(CrashConfidence::Low.to_string(), "Low");
}

#[test]
fn tool_dlls_and_asset_paths_correlate_to_their_mods() {
    let mut input = correlation_input();
    input
        .profile
        .mods
        .push(enabled_mod("armor-mod", Some("Armor")));
    input.installed_files.push((
        "armor-mod".to_string(),
        staged_file("meshes/armor/cuirass.nif"),
    ));
    input
        .installed_files
        .push(("bad-skse-plugin".to_string(), staged_file("dxgi.dll")));
    input.tool_files.push("bin/dxgi.dll".to_string());
    let log = "Unhandled exception\nmodule dxgi.dll\nwhile loading meshes\\armor\\cuirass.nif\n";

    let report = correlate_crash_log(
        Path::new("/tmp/generic.log"),
        log,
        CrashLogFormat::Auto,
        input,
    );
    assert_eq!(report.format, CrashLogFormat::Generic);
    let evidence = |mod_id: &str| {
        report
            .suspects
            .iter()
            .find(|s| s.mod_id.as_deref() == Some(mod_id))
            .unwrap_or_else(|| panic!("no suspect {mod_id}: {:?}", report.suspects))
    };
    let tool = evidence("bad-skse-plugin");
    assert!(
        tool.evidence
            .iter()
            .any(|e| e.reason.contains("tracked as tool-applied file")),
        "{:?}",
        tool.evidence
    );
    let asset = evidence("armor-mod");
    assert_eq!(asset.confidence, CrashConfidence::Medium);
    assert_eq!(asset.evidence[0].kind, CrashTokenKind::AssetPath);
}
