//! Central registry of every supported game: the [`GameRegistration`] records
//! that bind a `game_id` to its plugin, scanner, save tracker, and launcher IDs,
//! plus the lookup helpers ([`resolve_game`], [`all_games`]) used across the crate.

use std::sync::{OnceLock, RwLock};

use crate::generic::loader::load_user_games;
use crate::optiscaler::OptiScalerProfile;
use crate::traits::{
    GamePlugin, HotDeploySupport, ModScanner, SaveDependencyAnalyzer, SaveTracker,
};

/// Factory producing a boxed collision classifier for a registered game.
pub type CollisionClassifierFactory = fn() -> Box<dyn modde_core::collision::CollisionClassifier>;

/// The modding engine a game is built on, used to share engine-wide behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineFamily {
    Bethesda,
    Bannerlord,
    CyberpunkRedEngine,
    Gamebryo,
    Generic,
    Larian,
    Smapi,
    Unreal4,
    Witcher,
}

/// Per-launcher identifiers used to locate a game install across Steam and Heroic.
#[derive(Debug, Clone, Copy, Default)]
pub struct LauncherIds {
    pub steam_app_id: Option<&'static str>,
    pub steam_dir: Option<&'static str>,
    pub heroic_gog_app_id: Option<&'static str>,
    pub heroic_epic_app_id: Option<&'static str>,
}

/// A single supported game's metadata and capability wiring: it ties a
/// `game_id` to its [`GamePlugin`], optional [`ModScanner`]/[`SaveTracker`],
/// launcher IDs, and Nexus/Wabbajack identifiers.
#[derive(Clone, Copy)]
pub struct GameRegistration {
    pub game_id: &'static str,
    pub display_name: &'static str,
    pub engine: EngineFamily,
    pub launcher: LauncherIds,
    pub wabbajack_names: &'static [&'static str],
    pub nexus_domain: Option<&'static str>,
    pub nexus_game_id: Option<u32>,
    pub supports_save_profiles: bool,
    pub plugin: &'static dyn GamePlugin,
    pub scanner: Option<&'static dyn ModScanner>,
    pub save_tracker: Option<&'static dyn SaveTracker>,
    pub save_dependency_analyzer: Option<&'static dyn SaveDependencyAnalyzer>,
    pub collision_classifier: Option<CollisionClassifierFactory>,
    pub hot_deploy: HotDeploySupport,
    pub optiscaler_profiles: &'static [OptiScalerProfile],
}

impl GameRegistration {
    /// Yield this game's Wabbajack names lowercased and stripped to ASCII
    /// alphanumerics, for robust matching against manifest game strings.
    pub fn normalized_wabbajack_names(self) -> impl Iterator<Item = String> {
        self.wabbajack_names.iter().map(|name| {
            name.chars()
                .filter(char::is_ascii_alphanumeric)
                .flat_map(char::to_lowercase)
                .collect()
        })
    }
}

mod classifiers;
mod games;

pub(crate) use classifiers::generic_collision_classifier;
pub use games::{GAME_REGISTRY, SUPPORTED_GAME_IDS};

// Persisted game IDs must not acquire generic semantics when a feature is disabled.
pub(crate) const RESERVED_GAME_IDS: &[&str] = &[
    "skyrim-se", "skyrim-ae", "fallout4", "fallout76", "starfield", "cyberpunk2077",
    "stellar-blade", "baldurs-gate3", "stardew-valley", "fallout-new-vegas", "oblivion",
    "oblivion-remastered", "bannerlord", "witcher3", "subnautica2",
];

static REGISTRY: OnceLock<RwLock<&'static [GameRegistration]>> = OnceLock::new();

fn build_registry_snapshot() -> &'static [GameRegistration] {
    Box::leak(
        GAME_REGISTRY
            .iter()
            .copied()
            .chain(load_user_games())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    )
}

/// Return the current registry snapshot of all registered games.
#[must_use]
pub fn all_games() -> &'static [GameRegistration] {
    *REGISTRY
        .get_or_init(|| RwLock::new(build_registry_snapshot()))
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Rebuild the registry snapshot, picking up newly added user-defined games.
pub fn reload_registry() {
    let registry = REGISTRY.get_or_init(|| RwLock::new(build_registry_snapshot()));
    *registry
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = build_registry_snapshot();
}

/// List the `game_id` of every registered game.
#[must_use]
pub fn supported_game_ids() -> Vec<&'static str> {
    all_games().iter().map(|game| game.game_id).collect()
}

/// Look up a game registration by its `game_id`.
#[must_use]
pub fn resolve_game(game_id: &str) -> Option<&'static GameRegistration> {
    all_games().iter().find(|game| game.game_id == game_id)
}

/// Resolve a Nexus Mods domain (e.g. `"stellarblade"`, `"skyrimspecialedition"`)
/// to its registered game. Used by the install pipeline so a Nexus URL
/// like `nexusmods.com/<domain>/mods/<id>` can pick up game-specific
/// install hints even when the domain string differs from the modde
/// `game_id`.
#[must_use]
pub fn resolve_game_by_nexus_domain(domain: &str) -> Option<&'static GameRegistration> {
    all_games()
        .iter()
        .find(|game| game.nexus_domain == Some(domain))
}

/// Iterate over games that have at least one known launcher (Steam or Heroic) ID.
pub fn launcher_games() -> impl Iterator<Item = &'static GameRegistration> {
    all_games().iter().filter(|game| {
        game.launcher.steam_app_id.is_some()
            || game.launcher.heroic_gog_app_id.is_some()
            || game.launcher.heroic_epic_app_id.is_some()
    })
}
