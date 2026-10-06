//! The Gamebryo-engine game plugin (Oblivion, Fallout 3/New Vegas): a
//! data-driven [`GamePlugin`] shared across the supported Gamebryo titles,
//! plus `plugins.txt`-style load-order file helpers.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use modde_core::installer::InstallMethod;

use crate::policies::{BareLayoutPolicy, ContentPolicy};
use crate::traits::{ContentCategory, GamePlugin, ModSafety};

/// A configurable [`GamePlugin`] instance for a specific Gamebryo-engine game.
pub struct GamebryoGame {
    game_id: &'static str,
    display_name: &'static str,
    steam_app_id: &'static str,
    my_games_dir: &'static str,
    ini_files: &'static [&'static str],
    archive_ext: &'static [&'static str],
    nexus_domain: &'static str,
}

impl GamebryoGame {
    #[must_use]
    pub const fn new(
        game_id: &'static str,
        display_name: &'static str,
        steam_app_id: &'static str,
        my_games_dir: &'static str,
        ini_files: &'static [&'static str],
        archive_ext: &'static [&'static str],
        nexus_domain: &'static str,
    ) -> Self {
        Self {
            game_id,
            display_name,
            steam_app_id,
            my_games_dir,
            ini_files,
            archive_ext,
            nexus_domain,
        }
    }
}

const GAMEBRYO_SAVE_BREAKING_EXT: &[&str] = &["esp", "esm", "pex", "dll", "obse", "nvse"];
const GAMEBRYO_COSMETIC_EXT: &[&str] = &[
    "nif", "bsa", "dds", "png", "tga", "jpg", "kf", "wav", "mp3", "ogg", "ini", "xml",
];

const GAMEBRYO_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("esp", ContentCategory::Plugin),
    ("esm", ContentCategory::Plugin),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
    ("nif", ContentCategory::Mesh),
    ("kf", ContentCategory::Mesh),
    ("wav", ContentCategory::Sound),
    ("mp3", ContentCategory::Sound),
    ("ogg", ContentCategory::Sound),
    ("pex", ContentCategory::Script),
    ("obse", ContentCategory::Script),
    ("nvse", ContentCategory::Script),
    ("bsa", ContentCategory::Archive),
    ("ini", ContentCategory::Config),
    ("xml", ContentCategory::Config),
    ("dll", ContentCategory::Binary),
];

const GAMEBRYO_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: GAMEBRYO_SAVE_BREAKING_EXT,
    cosmetic_ext: GAMEBRYO_COSMETIC_EXT,
    save_breaking_dirs: &["scripts"],
    categories: GAMEBRYO_CONTENT_CATEGORIES,
};

const GAMEBRYO_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &[
        "data", "meshes", "textures", "sound", "music", "menus", "scripts", "shaders",
    ],
    root_file_exts: &["esp", "esm", "bsa"],
    case_insensitive_dirs: true,
};

pub const FALLOUT_NEW_VEGAS: GamebryoGame = GamebryoGame::new(
    "fallout-new-vegas",
    "Fallout: New Vegas",
    "22380",
    "FalloutNV",
    &["Fallout.ini", "FalloutPrefs.ini", "FalloutCustom.ini"],
    &["bsa"],
    "newvegas",
);

pub const OBLIVION: GamebryoGame = GamebryoGame::new(
    "oblivion",
    "The Elder Scrolls IV: Oblivion",
    "22330",
    "Oblivion",
    &["Oblivion.ini"],
    &["bsa"],
    "oblivion",
);

/// Read a `plugins.txt`-style load-order file, stripping comments and the
/// leading `*` enabled-marker from each plugin name.
pub fn read_plugin_order_file(path: &Path) -> std::io::Result<Vec<String>> {
    let content = std::fs::read_to_string(path)?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with(';'))
        .map(|line| line.trim_start_matches('*').to_string())
        .collect())
}

/// Write a `plugins.txt`-style load-order file, marking every plugin enabled
/// with a leading `*`.
pub fn write_plugin_order_file(path: &Path, plugins: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut content = String::new();
    for plugin in plugins {
        content.push('*');
        content.push_str(plugin);
        content.push('\n');
    }
    std::fs::write(path, content)
}

impl GamePlugin for GamebryoGame {
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
        let compat = modde_core::paths::steam_common()
            .parent()?
            .join("compatdata")
            .join(self.steam_app_id)
            .join("pfx/drive_c/users/steamuser/Documents/My Games")
            .join(self.my_games_dir)
            .join("Saves");
        Some(compat)
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            self.steam_app_id,
            &Path::new("Documents/My Games")
                .join(self.my_games_dir)
                .join("Saves"),
        )
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        GAMEBRYO_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        GAMEBRYO_CONTENT_POLICY.classify_extension(ext)
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

    fn nexus_game_domain(&self) -> Option<&str> {
        Some(self.nexus_domain)
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        extracted_dir
            .join("Data")
            .is_dir()
            .then(|| InstallMethod::StripContentRoot {
                root: "Data".to_string(),
            })
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        GAMEBRYO_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
    }
}
