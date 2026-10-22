#![allow(clippy::wildcard_imports)]
use super::*;

pub fn demo_profile() -> Profile {
    let mods = vec![
        demo_mod("skyui", "SkyUI", Some("5.2.0"), None, None, Some(12_604)),
        demo_mod(
            "uiextensions",
            "UIExtensions",
            Some("1.2.0"),
            None,
            Some("Required by several follower mods."),
            Some(57_046),
        ),
        demo_mod(
            "smim",
            "Static Mesh Improvement Mod (SMIM)",
            Some("2.06"),
            Some(2),
            Some("Big visual upgrade — keep high in the order."),
            Some(8_655),
        ),
        demo_mod(
            "noble-skyrim",
            "Noble Skyrim - Texture Overhaul",
            Some("1.1"),
            Some(2),
            None,
            Some(50_022),
        ),
        demo_mod(
            "ussep",
            "Unofficial Skyrim Special Edition Patch",
            Some("4.3.2"),
            Some(1),
            Some("Load order foundation. Do not disable."),
            Some(266),
        ),
        demo_mod(
            "alternate-start",
            "Alternate Start - Live Another Life",
            Some("4.1.6"),
            Some(1),
            None,
            Some(272),
        ),
        // A couple of disabled entries so the toggle column reads naturally.
        disabled_mod(
            "old-magic",
            "Apocalypse - Magic of Skyrim (disabled)",
            Some("9.45"),
            Some(1),
        ),
        demo_mod(
            "immersive-armors",
            "Immersive Armors",
            Some("8.1"),
            None,
            Some("Pending cleanup of conflicting meshes."),
            Some(19_733),
        ),
    ];

    Profile {
        id: Some(1),
        name: "Demo".to_string(),
        game_id: modde_core::GameId::from("skyrim-se"),
        source: ProfileSource::Manual,
        mods,
        overrides: PathBuf::from("/tmp/demo/overrides"),
        load_order_rules: SmallVec::new(),
        load_order_lock: None,
    }
}

fn demo_mod(
    mod_id: &str,
    display_name: &str,
    version: Option<&str>,
    category_id: Option<i64>,
    notes: Option<&str>,
    nexus_mod_id: Option<u64>,
) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(display_name.to_string()),
        enabled: true,
        version: version.map(str::to_string),
        category_id,
        notes: notes.map(str::to_string),
        nexus_mod_id: nexus_mod_id.map(NexusModId::from),
        ..Default::default()
    }
}

fn disabled_mod(
    mod_id: &str,
    display_name: &str,
    version: Option<&str>,
    category_id: Option<i64>,
) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(display_name.to_string()),
        enabled: false,
        version: version.map(str::to_string),
        category_id,
        ..Default::default()
    }
}

pub fn populate_mod_list(app: &mut Modde) {
    let profile = demo_profile();
    app.mod_id_filter_keys = modde_core::filter::mod_id_filter_keys(&profile.mods);
    app.active_profile = Some(profile.name.clone());
    app.profiles = vec![demo_profile_summary()];
    app.loaded_profile = Some(profile);
    app.selected_mod_index = Some(0);
    app.active_view = View::ModList;
    app.status_message = "Loaded profile 'Demo' (8 mods)".to_string();
}

pub fn demo_profile_summary() -> modde_core::profile::ProfileSummary {
    modde_core::profile::ProfileSummary {
        id: 1,
        name: "Demo".to_string(),
        game_id: modde_core::GameId::from("skyrim-se"),
        mod_count: 8,
        source_type: "manual".to_string(),
    }
}

pub fn populate_downloads(app: &mut Modde) {
    use modde_sources::queue::DownloadState;

    // Enqueue four tasks, then mutate each into a distinct state so the queue
    // shows the full range of UI rows (active progress, queued, complete,
    // failed). `track_download` would also work but only yields Queued rows.
    let active = enqueue_demo(app, "noble-skyrim", "Noble Skyrim - Texture Overhaul");
    let queued = enqueue_demo(app, "smim", "Static Mesh Improvement Mod (SMIM)");
    let complete = enqueue_demo(app, "skyui", "SkyUI");
    let failed = enqueue_demo(app, "immersive-armors", "Immersive Armors");

    if let Some(task) = app.download_queue.get_mut(active) {
        task.state = DownloadState::Active {
            bytes_downloaded: 734_003_200,
            total_bytes: Some(1_181_116_006),
        };
    }
    // `queued` is left in the default Queued state from enqueue.
    let _ = queued;
    if let Some(task) = app.download_queue.get_mut(complete) {
        task.state = DownloadState::Complete {
            path: PathBuf::from("/home/demo/.local/share/modde/downloads/skyui.zip"),
            hash: 0xDEAD_BEEF,
        };
    }
    if let Some(task) = app.download_queue.get_mut(failed) {
        task.state = DownloadState::Failed {
            error: "connection reset by peer (Nexus rate limit)".to_string(),
        };
    }

    app.active_view = View::Downloads;
    app.status_message = "1 active, 1 queued, 1 complete, 1 failed".to_string();
}

fn enqueue_demo(app: &mut Modde, key: &str, name: &str) -> usize {
    app.download_queue.enqueue(
        key.to_string(),
        PathBuf::from(format!(
            "/home/demo/.local/share/modde/downloads/{key}.download"
        )),
        None,
        modde_sources::meta::DownloadMeta {
            url: format!("https://example.invalid/{key}"),
            expected_hash: None,
            bytes_downloaded: 0,
            total_bytes: None,
            nexus_mod_id: None,
            nexus_file_id: None,
            game_domain: Some("skyrimspecialedition".to_string()),
            mod_name: Some(name.to_string()),
            version: None,
            status: "queued".to_string(),
        },
    )
}

/// A small but realistic FOMOD config with two install steps, several groups
/// and plugins. No filesystem is required — images are omitted.
const DEMO_FOMOD_XML: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
    <moduleName>Demo Texture Pack FOMOD</moduleName>
    <installSteps order="Explicit">
        <installStep name="Choose Texture Resolution">
            <optionalFileGroups order="Explicit">
                <group name="Texture Resolution" type="SelectExactlyOne">
                    <plugins order="Explicit">
                        <plugin name="2K Textures">
                            <description>Balanced quality and VRAM usage. Recommended for most setups.</description>
                            <typeDescriptor><type name="Recommended"/></typeDescriptor>
                        </plugin>
                        <plugin name="4K Textures">
                            <description>Highest quality. Requires 8GB+ VRAM.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                        <plugin name="1K Textures (Performance)">
                            <description>Lowest VRAM footprint for low-end hardware.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
            </optionalFileGroups>
        </installStep>
        <installStep name="Optional Add-ons">
            <optionalFileGroups order="Explicit">
                <group name="Extra Content" type="SelectAny">
                    <plugins order="Explicit">
                        <plugin name="Parallax Meshes">
                            <description>Adds depth to flat surfaces. Needs an ENB that supports parallax.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                        <plugin name="Snow Shader Tweaks">
                            <description>Improves snow material blending.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
                <group name="Compatibility Patches" type="SelectAtMostOne">
                    <plugins order="Explicit">
                        <plugin name="USSEP Patch">
                            <description>Patch for the Unofficial Skyrim Special Edition Patch.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                        <plugin name="No Patch">
                            <description>Install the base files only.</description>
                            <typeDescriptor><type name="Optional"/></typeDescriptor>
                        </plugin>
                    </plugins>
                </group>
            </optionalFileGroups>
        </installStep>
    </installSteps>
</config>
"#;

pub fn populate_fomod_wizard(app: &mut Modde) {
    let config = fomod_oxide::ModuleConfig::parse(DEMO_FOMOD_XML)
        .expect("parse embedded demo FOMOD ModuleConfig");
    let installer = fomod_oxide::Installer::new(config);
    let mut state = FOMODWizardState::with_installer(installer);

    // Apply the installer's default selections (mirrors update.rs StartFOMOD).
    let defaults = state.default_selections();
    app.fomod_selections.clear();
    for (step_idx, group_idx, sel) in defaults {
        state.select(step_idx, group_idx, sel.clone());
        app.fomod_selections.insert((step_idx, group_idx), sel);
    }

    app.fomod_installer = Some(state);
    app.fomod_source_dir = Some(PathBuf::from("/tmp/demo/fomod-source"));
    app.fomod_dest_dir = Some(PathBuf::from("/tmp/demo/fomod-dest"));
    app.fomod_wizard_pos = 0;
    app.fomod_can_undo = false;
    app.refresh_fomod_visible_steps();
    // `refresh_fomod_conflicts` is pub(in crate::app); derive the same data here.
    app.fomod_conflicts = app
        .fomod_installer
        .as_ref()
        .map(|i| SmallVec::from_vec(i.detect_conflicts()))
        .unwrap_or_default();

    // The view reads from `fomod_installer`; the View payload is just a marker.
    app.active_view = View::FOMODWizard(FOMODWizardState::new());
    app.status_message = "FOMOD wizard started".to_string();
}

pub fn populate_tools(app: &mut Modde) {
    // A loaded profile gives the tools view a game label context.
    let profile = demo_profile();
    app.active_profile = Some(profile.name.clone());
    app.loaded_profile = Some(profile);
    app.tool_state = demo_tool_state();
    app.active_view = View::Tools;
    app.status_message = "Tools for Skyrim SE".to_string();
}

fn demo_tool_state() -> crate::app::ToolState {
    use crate::app::{ToolReleaseSupport, ToolState, ToolUiEntry};

    let entry = ToolUiEntry {
        tool_id: "optiscaler".to_string(),
        display_name: "OptiScaler".to_string(),
        description: "FSR / DLSS / XeSS upscaling injection".to_string(),
        category: "Graphics".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: true,
        settings: serde_json::json!({}),
        setting_specs: Vec::new(),
        generated_config_path: None,
        applied_files: vec!["dxgi.dll".to_string(), "OptiScaler.ini".to_string()],
        has_file_patching: true,
        release_support: ToolReleaseSupport::Supported,
        status_message: Some("Managed; proxy d3d12.dll".to_string()),
        env_preview: Vec::new(),
        dll_overrides: Vec::new(),
        wrapper_preview: Vec::new(),
        derived_facts: Vec::new(),
        optiscaler_state: Some("version goverlay-edge 0.9.12".to_string()),
        optiscaler_latest_backup: None,
        optiscaler_detected_files: 3,
        apply_pending: false,
        apply_missing_inputs: Vec::new(),
        setting_history: Vec::new(),
        config_checklist: Vec::new(),
        dirty_keys: std::collections::HashSet::new(),
    };

    let reshade = ToolUiEntry {
        tool_id: "reshade".to_string(),
        display_name: "ReShade".to_string(),
        description: "Post-processing shader injector".to_string(),
        category: "Graphics".to_string(),
        available: true,
        availability_text: "available".to_string(),
        enabled: false,
        ..entry.clone()
    };

    ToolState {
        entries: vec![entry, reshade],
        active_tool_id: Some("optiscaler".to_string()),
        game_label: Some("Skyrim SE".to_string()),
        game_dir_configured: true,
        ..Default::default()
    }
}

pub fn populate_browse_nexus(app: &mut Modde) {
    // The browse view renders its game-picker + search chrome even without
    // fetched results, with a loaded profile providing the game domain.
    let profile = demo_profile();
    app.active_profile = Some(profile.name.clone());
    app.loaded_profile = Some(profile);
    app.active_view = View::BrowseNexus;
    app.status_message = "Browse Nexus".to_string();
}

pub fn populate_library(app: &mut Modde) {
    use crate::views::library::{LibraryGameInstall, build_game_entries};

    let installs = [
        (
            "skyrim-se",
            "The Elder Scrolls V: Skyrim Special Edition",
            "Steam",
            "/home/demo/.local/share/Steam/steamapps/common/Skyrim Special Edition",
        ),
        (
            "fallout4",
            "Fallout 4",
            "Steam",
            "/home/demo/.local/share/Steam/steamapps/common/Fallout 4",
        ),
        (
            "cyberpunk2077",
            "Cyberpunk 2077",
            "Heroic (GOG)",
            "/home/demo/Games/Heroic/Cyberpunk 2077",
        ),
        (
            "stardew-valley",
            "Stardew Valley",
            "Steam",
            "/home/demo/.local/share/Steam/steamapps/common/Stardew Valley",
        ),
    ]
    .into_iter()
    .map(
        |(game_id, display_name, source_label, install_path)| LibraryGameInstall {
            game_id: game_id.to_string(),
            display_name: display_name.to_string(),
            install_path: PathBuf::from(install_path),
            source_label: source_label.to_string(),
        },
    )
    .collect();

    let mut entries = build_game_entries(installs);
    if let Some(first) = entries.first_mut() {
        first.favorite = true;
        first.managed = true;
    }
    app.library.selected_id = entries.first().map(|entry| entry.id.clone());
    app.library.entries = entries;
    app.active_view = View::Library;
}
