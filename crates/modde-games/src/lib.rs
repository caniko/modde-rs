//! Per-game support for modde: the game registry plus each game's plugin,
//! scanner, save tracker, and tool integrations, exposed through `game_id`-keyed
//! resolver functions.

#[cfg(feature = "bethesda")]
use anyhow::Context;
use anyhow::Result;
use smallvec::SmallVec;

#[cfg(feature = "bannerlord")]
pub mod bannerlord;
#[cfg(feature = "bethesda")]
pub mod bethesda;
#[cfg(feature = "bg3")]
pub mod bg3;
#[cfg(feature = "cyberpunk")]
pub mod cyberpunk;
pub mod detection;
#[cfg(feature = "gamebryo")]
pub mod gamebryo;
pub mod generic;
pub mod gpu;
pub mod launcher;
pub mod oblivion_remastered;
pub mod optiscaler;
pub mod policies;
pub mod registry;
pub mod save_patterns;
pub mod scanner_patterns;
#[cfg(feature = "stardew")]
pub mod stardew;
pub mod tools;
pub mod traits;
#[cfg(feature = "ue4")]
pub mod ue4;
#[cfg(feature = "witcher3")]
pub mod witcher3;

pub use detection::{DetectedGame, LauncherSource, find_detected_game, scan_installed_games};
pub use generic::loader::{load_user_games, reload_user_games};
pub use generic::manage::{
    AddUserGameResult, DetectCandidateDir, add_user_game, detect_candidates, read_user_game_spec,
    remove_user_game,
};
pub use optiscaler::{
    OptiScalerIniOverride, OptiScalerProfile, default_optiscaler_profile,
    resolve_optiscaler_profiles,
};
pub use registry::{
    EngineFamily, GameRegistration, LauncherIds, all_games, resolve_game, supported_game_ids,
};
pub use traits::{
    DeployTarget, DeployTargetKind, DiscoveredFile, DiscoveredMod, GamePlugin, HotDeployCapability,
    HotDeploySupport, ModClassifyConfig, ModSafety, ModScanner, ModSource, SaveDependencyAnalyzer,
    SaveDependencyFinding, SaveDependencyKind, SaveRemovalGateReport, SaveTracker, ScanContext,
    classify_mod_by_content, slug, walk_files_relative,
};

/// Build an [`modde_core::installer::InstallProbe`] that delegates to a
/// game plugin's [`GamePlugin::analyze_mod_archive`] and
/// [`GamePlugin::recognizes_bare_layout`] hooks.
///
/// Only `&'static dyn GamePlugin` is accepted because all registered
/// plugins are `static` (see [`resolve_game_plugin`]), and the probe's
/// closures need `'static` captures to cross async task boundaries.
///
/// ```ignore
/// let plugin = resolve_game_plugin("skyrim-se").unwrap();
/// let probe = game_probe(plugin);
/// let plan = modde_core::installer::analyze(&extracted, &probe, hash)?;
/// ```
pub fn game_probe(plugin: &'static dyn GamePlugin) -> modde_core::installer::InstallProbe {
    let mut probe = modde_core::installer::InstallProbe::new(
        move |dir: &std::path::Path| plugin.analyze_mod_archive(dir),
        move |dir: &std::path::Path| plugin.recognizes_bare_layout(dir),
    );
    // Surface the first `UserConfig` deploy target the plugin
    // advertises so analyze can route config-only archives.
    if let Some(target) = plugin
        .deploy_targets()
        .iter()
        .find(|t| t.kind == crate::traits::DeployTargetKind::UserConfig)
    {
        probe = probe.with_user_config_target(target.id);
    }
    probe
}

/// All recognized game IDs, in registry order.
pub const SUPPORTED_GAME_IDS: &[&str] = registry::SUPPORTED_GAME_IDS;

/// (`game_id`, `display_name`) for every supported game, derived from the plugin registry.
#[must_use]
pub fn supported_games() -> SmallVec<[(&'static str, &'static str); 8]> {
    registry::all_games()
        .iter()
        .map(|game| (game.game_id, game.display_name))
        .collect()
}

/// Map a Wabbajack manifest `game` field (e.g. `"Cyberpunk2077"`, `"SkyrimSpecialEdition"`)
/// to the internal `game_id` (e.g. `"cyberpunk2077"`, `"skyrim-se"`).
///
/// Returns `None` if the name is not recognized.
#[must_use]
pub fn normalize_wabbajack_game(wj_game: &str) -> Option<&'static str> {
    let key: String = wj_game
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect();

    registry::all_games()
        .iter()
        .find(|game| game.normalized_wabbajack_names().any(|name| name == key))
        .map(|game| game.game_id)
}

/// Resolve a `game_id` string to the corresponding `GamePlugin` implementation.
#[must_use]
pub fn resolve_game_plugin(game_id: &str) -> Option<&'static dyn GamePlugin> {
    registry::resolve_game(game_id).map(|game| game.plugin)
}

/// Like [`resolve_game_plugin`] but keyed on the Nexus Mods domain.
///
/// The Nexus URL format embeds the domain (e.g. `stellarblade`,
/// `skyrimspecialedition`), which in many cases doesn't match modde's
/// `game_id` (`stellar-blade`, `skyrim-se`). The install pipeline uses
/// this to recover the right plugin when a user pastes a Nexus URL.
#[must_use]
pub fn resolve_game_plugin_by_nexus_domain(domain: &str) -> Option<&'static dyn GamePlugin> {
    registry::resolve_game_by_nexus_domain(domain).map(|game| game.plugin)
}

/// Resolve a `game_id` to its `ModScanner` implementation, if one exists.
#[must_use]
pub fn resolve_mod_scanner(game_id: &str) -> Option<&'static dyn ModScanner> {
    registry::resolve_game(game_id).and_then(|game| game.scanner)
}

/// Resolve a `game_id` to its `CollisionClassifier` implementation, if one exists.
#[must_use]
pub fn resolve_collision_classifier(
    game_id: &str,
) -> Option<Box<dyn modde_core::collision::CollisionClassifier>> {
    registry::resolve_game(game_id).and_then(|game| game.collision_classifier.map(|build| build()))
}

/// Resolve a `game_id` to its `SaveTracker` implementation, if one exists.
#[must_use]
pub fn resolve_save_tracker(game_id: &str) -> Option<&'static dyn SaveTracker> {
    registry::resolve_game(game_id).and_then(|game| game.save_tracker)
}

/// Resolve a `game_id` to its save dependency analyzer, if one exists.
#[must_use]
pub fn resolve_save_dependency_analyzer(
    game_id: &str,
) -> Option<&'static dyn SaveDependencyAnalyzer> {
    registry::resolve_game(game_id).and_then(|game| game.save_dependency_analyzer)
}

/// Return whether a game participates in modde's per-profile save layer.
#[must_use]
pub fn supports_save_profiles(game_id: &str) -> bool {
    registry::resolve_game(game_id).is_some_and(|game| game.supports_save_profiles)
}

/// Read the native plugin order for a game from `plugins.txt`, when the game uses one.
#[cfg(feature = "bethesda")]
pub fn read_native_plugin_order(game_id: &str) -> Result<Vec<modde_core::PluginEntry>> {
    let plugin = resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game '{game_id}'"))?;
    let app_id = plugin
        .steam_app_id_u32()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;
    let folder = plugin
        .plugins_txt_folder()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;

    let entries = bethesda::plugins_txt::read_plugins_txt(app_id, folder)
        .with_context(|| format!("failed to read plugins.txt for '{game_id}'"))?;

    Ok(entries
        .into_iter()
        .enumerate()
        .map(|(sort_index, entry)| modde_core::PluginEntry {
            plugin_name: entry.name,
            sort_index: sort_index as i64,
            enabled: entry.enabled,
        })
        .collect())
}

/// Persist plugin order back to the game's native `plugins.txt`, when supported.
#[cfg(feature = "bethesda")]
pub fn write_native_plugin_order(game_id: &str, plugins: &[modde_core::PluginEntry]) -> Result<()> {
    let plugin = resolve_game_plugin(game_id)
        .ok_or_else(|| anyhow::anyhow!("unsupported game '{game_id}'"))?;
    let app_id = plugin
        .steam_app_id_u32()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;
    let folder = plugin
        .plugins_txt_folder()
        .ok_or_else(|| anyhow::anyhow!("game '{game_id}' does not expose plugins.txt"))?;

    let entries: Vec<bethesda::plugins_txt::PluginEntry> = plugins
        .iter()
        .map(|plugin| bethesda::plugins_txt::PluginEntry {
            name: plugin.plugin_name.clone(),
            enabled: plugin.enabled,
        })
        .collect();

    bethesda::plugins_txt::write_plugins_txt(app_id, folder, &entries)
        .with_context(|| format!("failed to write plugins.txt for '{game_id}'"))
}

/// Native `plugins.txt` support requires the `bethesda` feature.
#[cfg(not(feature = "bethesda"))]
pub fn read_native_plugin_order(game_id: &str) -> Result<Vec<modde_core::PluginEntry>> {
    anyhow::bail!("native plugin order for '{game_id}' requires the bethesda feature")
}

/// Native `plugins.txt` support requires the `bethesda` feature.
#[cfg(not(feature = "bethesda"))]
pub fn write_native_plugin_order(game_id: &str, _plugins: &[modde_core::PluginEntry]) -> Result<()> {
    anyhow::bail!("native plugin order for '{game_id}' requires the bethesda feature")
}
