//! The Stardew Valley (SMAPI) game plugin and its mod-layout policies.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use modde_core::installer::InstallMethod;

use crate::policies::{BareLayoutPolicy, ContentPolicy};
use crate::traits::{ContentCategory, GamePlugin, ModSafety};

/// [`GamePlugin`] for Stardew Valley, modded via the SMAPI loader.
pub struct SmapiGame;

pub static STARDEW_VALLEY: SmapiGame = SmapiGame;

const STARDEW_SAVE_BREAKING_EXT: &[&str] = &["dll", "json", "xnb", "tmx", "tbin"];
const STARDEW_COSMETIC_EXT: &[&str] = &["png", "jpg", "xnb"];
const STARDEW_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("dll", ContentCategory::Binary),
    ("json", ContentCategory::Config),
    ("xnb", ContentCategory::Archive),
    ("tmx", ContentCategory::Config),
    ("tbin", ContentCategory::Config),
    ("png", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
];

const STARDEW_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: STARDEW_SAVE_BREAKING_EXT,
    cosmetic_ext: STARDEW_COSMETIC_EXT,
    save_breaking_dirs: &[],
    categories: STARDEW_CONTENT_CATEGORIES,
};

const STARDEW_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &["mods"],
    root_file_exts: &["json", "dll"],
    case_insensitive_dirs: true,
};

impl GamePlugin for SmapiGame {
    fn game_id(&self) -> &'static str {
        "stardew-valley"
    }

    fn display_name(&self) -> &'static str {
        "Stardew Valley"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Mods")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        Some(modde_core::paths::user_config_dir().join("StardewValley/Saves"))
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        if prefix.is_none() { return self.save_directory(); }
        crate::library::context::steam_user_path(install, prefix, "413150",
            Path::new("AppData/Roaming/StardewValley/Saves"))
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        STARDEW_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        STARDEW_CONTENT_POLICY.classify_extension(ext)
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.to_path_buf()
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(413150)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("stardewvalley")
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        if extracted_dir.join("manifest.json").is_file() {
            return Some(InstallMethod::DirectoryMod {
                directory_name: None,
            });
        }
        extracted_dir
            .join("Mods")
            .is_dir()
            .then(|| InstallMethod::StripContentRoot {
                root: "Mods".to_string(),
            })
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        extracted_dir.join("manifest.json").is_file()
            || STARDEW_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
    }
}
