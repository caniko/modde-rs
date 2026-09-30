//! Data-driven support for Bethesda Creation Engine games (Skyrim, Fallout,
//! Starfield), grouping per-game submodules (archives, INI handling, FOMOD,
//! plugin scanning, saves) and defining [`BethesdaGame`], a single
//! [`GamePlugin`] implementation parameterized by static per-title metadata.

pub mod archive_index;
pub mod archives;
pub mod collision;
pub mod diagnostics;
pub mod fomod;
pub mod ini;
pub mod ini_profiles;
pub mod ini_tweaks;
pub mod loot;
pub mod plugin_header;
pub mod plugins_txt;
pub mod records;
pub mod save_analysis;
pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use modde_core::paths;

use crate::policies::{BareLayoutPolicy, ContentPolicy};
use crate::traits::{ContentCategory, GamePlugin, ModSafety};

/// Data-driven Bethesda game plugin.
///
/// All Bethesda games share the same deploy strategy (symlink into `Data/`)
/// and differ only in metadata. This replaces four near-identical unit structs.
pub struct BethesdaGame {
    game_id: &'static str,
    display_name: &'static str,
    /// Steam App ID (for Proton save path detection).
    steam_app_id: &'static str,
    /// "My Games" subdirectory name where saves are stored.
    my_games_dir: &'static str,
    /// INI file names managed per-profile.
    ini_files: &'static [&'static str],
    /// Archive file extensions this game uses.
    archive_ext: &'static [&'static str],
    /// Nexus Mods game domain name.
    nexus_domain: &'static str,
    /// Game folder name in Proton's AppData/Local for plugins.txt.
    plugins_txt_folder_name: &'static str,
    /// Whether this specific Bethesda game participates in modde's
    /// per-profile save layer. Not every Bethesda plugin has shipped save
    /// traversal support yet.
    save_profiles: bool,
}

impl BethesdaGame {
    #[must_use]
    pub const fn new(
        game_id: &'static str,
        display_name: &'static str,
        steam_app_id: &'static str,
        my_games_dir: &'static str,
        ini_files: &'static [&'static str],
        archive_ext: &'static [&'static str],
        nexus_domain: &'static str,
        plugins_txt_folder_name: &'static str,
    ) -> Self {
        Self {
            game_id,
            display_name,
            steam_app_id,
            my_games_dir,
            ini_files,
            archive_ext,
            nexus_domain,
            plugins_txt_folder_name,
            save_profiles: false,
        }
    }

    /// Opt this Bethesda game into modde's per-profile save layer.
    #[must_use]
    pub const fn with_save_profiles(mut self, enabled: bool) -> Self {
        self.save_profiles = enabled;
        self
    }
}

/// File extensions that indicate a Bethesda mod alters game logic.
const BETHESDA_SAVE_BREAKING_EXT: &[&str] = &["esp", "esm", "esl", "pex", "dll", "psc"];

/// Extensions that are purely cosmetic in Bethesda games.
const BETHESDA_COSMETIC_EXT: &[&str] = &[
    "nif", "bsa", "ba2", "dds", "png", "tga", "jpg", "hkx", "fuz", "wav", "xwm", "swf", "ini",
    "json",
];

const BETHESDA_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("esp", ContentCategory::Plugin),
    ("esm", ContentCategory::Plugin),
    ("esl", ContentCategory::Plugin),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
    ("nif", ContentCategory::Mesh),
    ("wav", ContentCategory::Sound),
    ("xwm", ContentCategory::Sound),
    ("fuz", ContentCategory::Sound),
    ("mp3", ContentCategory::Sound),
    ("ogg", ContentCategory::Sound),
    ("pex", ContentCategory::Script),
    ("psc", ContentCategory::Script),
    ("swf", ContentCategory::Interface),
    ("bsa", ContentCategory::Archive),
    ("ba2", ContentCategory::Archive),
    ("ini", ContentCategory::Config),
    ("json", ContentCategory::Config),
    ("yaml", ContentCategory::Config),
    ("xml", ContentCategory::Config),
    ("toml", ContentCategory::Config),
    ("dll", ContentCategory::Binary),
    ("so", ContentCategory::Binary),
];

const BETHESDA_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: BETHESDA_SAVE_BREAKING_EXT,
    cosmetic_ext: BETHESDA_COSMETIC_EXT,
    save_breaking_dirs: &[],
    categories: BETHESDA_CONTENT_CATEGORIES,
};

const BETHESDA_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &[
        "data",
        "meshes",
        "textures",
        "scripts",
        "interface",
        "sound",
        "music",
        "materials",
        "seq",
        "shadersfx",
        "strings",
    ],
    root_file_exts: &["esp", "esm", "esl", "bsa", "ba2"],
    case_insensitive_dirs: true,
};

pub const SKYRIM_SE: BethesdaGame = BethesdaGame::new(
    "skyrim-se",
    "The Elder Scrolls V: Skyrim Special Edition",
    "489830",
    "Skyrim Special Edition",
    &["Skyrim.ini", "SkyrimPrefs.ini", "SkyrimCustom.ini"],
    &["bsa", "ba2"],
    "skyrimspecialedition",
    "Skyrim Special Edition",
)
.with_save_profiles(true);

pub const SKYRIM_AE: BethesdaGame = BethesdaGame::new(
    "skyrim-ae",
    "The Elder Scrolls V: Skyrim Anniversary Edition",
    "489830",
    "Skyrim Special Edition",
    &["Skyrim.ini", "SkyrimPrefs.ini", "SkyrimCustom.ini"],
    &["bsa", "ba2"],
    "skyrimspecialedition",
    "Skyrim Special Edition",
)
.with_save_profiles(true);

pub const FALLOUT4: BethesdaGame = BethesdaGame::new(
    "fallout4",
    "Fallout 4",
    "377160",
    "Fallout4",
    &["Fallout4.ini", "Fallout4Prefs.ini", "Fallout4Custom.ini"],
    &["ba2"],
    "fallout4",
    "Fallout4",
)
.with_save_profiles(true);

pub const FALLOUT76: BethesdaGame = BethesdaGame::new(
    "fallout76",
    "Fallout 76",
    "1151340",
    "Fallout 76",
    &["Fallout76.ini", "Fallout76Prefs.ini", "Fallout76Custom.ini"],
    &["ba2"],
    "fallout76",
    "Fallout76",
)
.with_save_profiles(true);

pub const STARFIELD: BethesdaGame = BethesdaGame::new(
    "starfield",
    "Starfield",
    "1716740",
    "Starfield",
    &["StarfieldPrefs.ini", "StarfieldCustom.ini"],
    &["ba2"],
    "starfield",
    "Starfield",
)
.with_save_profiles(true);

impl GamePlugin for BethesdaGame {
    fn game_id(&self) -> &str {
        self.game_id
    }

    fn display_name(&self) -> &str {
        self.display_name
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Data")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        // Proton prefix: compatdata/<APP_ID>/pfx/drive_c/Users/steamuser/Documents/My Games/<DIR>/Saves
        let compat = paths::steam_common()
            .parent()? // steamapps/
            .join("compatdata")
            .join(self.steam_app_id)
            .join("pfx/drive_c/Users/steamuser/Documents/My Games")
            .join(self.my_games_dir)
            .join("Saves");
        if compat.exists() {
            return Some(compat);
        }
        None
    }

    fn supports_save_profiles(&self) -> bool {
        self.save_profiles
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(install, prefix, self.steam_app_id,
            &Path::new("Documents/My Games").join(self.my_games_dir).join("Saves"))
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        BETHESDA_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        BETHESDA_CONTENT_POLICY.classify_extension(ext)
    }

    fn ini_file_names(&self) -> &[&str] {
        self.ini_files
    }

    fn archive_extensions(&self) -> &[&str] {
        self.archive_ext
    }

    fn has_plugin_system(&self) -> bool {
        true
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        self.steam_app_id.parse().ok()
    }

    fn plugins_txt_folder(&self) -> Option<&str> {
        Some(self.plugins_txt_folder_name)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some(self.nexus_domain)
    }

    fn nexus_game_id_u32(&self) -> Option<u32> {
        // Nexus Mods v2 GraphQL numeric game IDs.
        // Source: https://api.nexusmods.com/v1/games.json
        match self.game_id {
            "skyrim-se" => Some(1704),
            "skyrim-ae" => Some(1704), // Skyrim SE/AE share the same Nexus domain
            "fallout4" => Some(1151),
            "fallout76" => Some(2590),
            "starfield" => Some(4187),
            _ => None,
        }
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        BETHESDA_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
    }
}
