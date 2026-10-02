//! The Oblivion Remastered game plugin: a UE5 wrapper around the Gamebryo
//! engine, mixing `~mods` `.pak` layout with Bethesda-style plugins.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use modde_core::installer::InstallMethod;

use crate::policies::{BareLayoutPolicy, ContentPolicy, DllOverridePolicy, StagingDllSearch};
use crate::traits::{ContentCategory, GamePlugin, ModSafety};

/// [`GamePlugin`] for The Elder Scrolls IV: Oblivion Remastered.
pub struct OblivionRemasteredGame;

pub static OBLIVION_REMASTERED: OblivionRemasteredGame = OblivionRemasteredGame;

const STEAM_APP_ID: &str = "2623190";
const PROJECT_NAME: &str = "OblivionRemastered";

const OR_SAVE_BREAKING_EXT: &[&str] = &["esp", "esm", "pak", "ucas", "utoc", "dll", "lua"];
const OR_COSMETIC_EXT: &[&str] = &["dds", "png", "jpg", "tga", "nif"];
const OR_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("esp", ContentCategory::Plugin),
    ("esm", ContentCategory::Plugin),
    ("pak", ContentCategory::Archive),
    ("ucas", ContentCategory::Archive),
    ("utoc", ContentCategory::Archive),
    ("nif", ContentCategory::Mesh),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("lua", ContentCategory::Script),
    ("dll", ContentCategory::Binary),
];

const OR_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: OR_SAVE_BREAKING_EXT,
    cosmetic_ext: OR_COSMETIC_EXT,
    save_breaking_dirs: &["plugins", "logicmods"],
    categories: OR_CONTENT_CATEGORIES,
};

const OR_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &["data", "mods", "content", "paks", "~mods"],
    root_file_exts: &["esp", "esm", "pak", "ucas", "utoc"],
    case_insensitive_dirs: true,
};

const OR_PROXY_DLLS: &[&str] = &["dwmapi", "xinput1_3", "d3d11", "dxgi", "version", "winmm"];
const OR_DLL_POLICY: DllOverridePolicy = DllOverridePolicy {
    proxy_dlls: OR_PROXY_DLLS,
    staging_search: StagingDllSearch::DirectChildDirs,
};

/// The UE5 `Content/Paks` directory for Oblivion Remastered, relative to `install`.
#[must_use]
pub fn paks_root(install: &Path) -> PathBuf {
    install.join(PROJECT_NAME).join("Content").join("Paks")
}

impl GamePlugin for OblivionRemasteredGame {
    fn game_id(&self) -> &'static str {
        "oblivion-remastered"
    }

    fn display_name(&self) -> &'static str {
        "The Elder Scrolls IV: Oblivion Remastered"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        paks_root(install).join("~mods")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        Some(
            modde_core::paths::steam_common()
                .parent()?
                .join("compatdata")
                .join(STEAM_APP_ID)
                .join("pfx/drive_c/users/steamuser/Documents/My Games/Oblivion Remastered/Saves"),
        )
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            STEAM_APP_ID,
            Path::new("Documents/My Games/Oblivion Remastered/Saves"),
        )
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        OR_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        OR_CONTENT_POLICY.classify_extension(ext)
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join(PROJECT_NAME).join("Binaries/Win64")
    }

    fn wine_dll_overrides(&self, game_dir: &Path) -> smallvec::SmallVec<[String; 4]> {
        OR_DLL_POLICY.from_executable_dir(&self.executable_dir(game_dir))
    }

    fn wine_dll_overrides_from_staging(&self, staging: &Path) -> smallvec::SmallVec<[String; 4]> {
        OR_DLL_POLICY.from_staging(staging)
    }

    fn archive_extensions(&self) -> &[&str] {
        &["pak", "ucas", "utoc"]
    }

    fn has_plugin_system(&self) -> bool {
        true
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(2623190)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("oblivionremastered")
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        if has_root_file_with_ext(extracted_dir, &["pak", "ucas", "utoc"]) {
            return Some(InstallMethod::SingleFileSet);
        }
        if extracted_dir.join("Data").is_dir() {
            return Some(InstallMethod::StripContentRoot {
                root: "Data".to_string(),
            });
        }
        None
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        OR_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
    }
}

fn has_root_file_with_ext(dir: &Path, extensions: &[&str]) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let path = entry.path();
            path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| {
                        extensions
                            .iter()
                            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
                    })
        })
    })
}
