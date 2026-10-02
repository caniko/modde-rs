//! The Witcher 3 game plugin: `REDkit` mod layout, script-conflict detection,
//! and save/scanner wiring.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use anyhow::Result;
use modde_core::installer::InstallMethod;

use crate::policies::{BareLayoutPolicy, ContentPolicy, DllOverridePolicy, StagingDllSearch};
use crate::traits::{ContentCategory, GamePlugin, ModSafety};

/// [`GamePlugin`] implementation for The Witcher 3: Wild Hunt.
pub struct Witcher3Game;

pub static WITCHER3: Witcher3Game = Witcher3Game;

const WITCHER_SAVE_BREAKING_EXT: &[&str] = &["ws", "xml", "bundle", "cache", "csv", "dll"];
const WITCHER_COSMETIC_EXT: &[&str] = &["dds", "png", "jpg", "xbm"];
const WITCHER_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("ws", ContentCategory::Script),
    ("xml", ContentCategory::Config),
    ("csv", ContentCategory::Config),
    ("bundle", ContentCategory::Archive),
    ("cache", ContentCategory::Archive),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
    ("xbm", ContentCategory::Texture),
    ("dll", ContentCategory::Binary),
];

const WITCHER_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: WITCHER_SAVE_BREAKING_EXT,
    cosmetic_ext: WITCHER_COSMETIC_EXT,
    save_breaking_dirs: &["content/scripts"],
    categories: WITCHER_CONTENT_CATEGORIES,
};

const WITCHER_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &["mods", "dlc", "bin", "content"],
    root_file_exts: &["ws", "bundle", "xml"],
    case_insensitive_dirs: true,
};

const WITCHER_PROXY_DLLS: &[&str] = &["dxgi", "d3d11", "version", "winmm"];
const WITCHER_DLL_POLICY: DllOverridePolicy = DllOverridePolicy {
    proxy_dlls: WITCHER_PROXY_DLLS,
    staging_search: StagingDllSearch::DirectChildDirs,
};

/// Returns `true` if a mod directory contains any `.ws` script files, which
/// can conflict with other mods' scripts.
#[must_use]
pub fn has_script_conflict(mod_dir: &Path) -> bool {
    let mut stack = vec![mod_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ws"))
            {
                return true;
            }
        }
    }
    false
}

/// List relative `.ws` script paths provided by more than one installed mod
/// under `mods_root` (i.e. the scripts that actually collide).
#[must_use]
pub fn script_conflict_paths(mods_root: &Path) -> Vec<String> {
    let mut providers: std::collections::BTreeMap<String, usize> =
        std::collections::BTreeMap::new();
    let Ok(mods) = std::fs::read_dir(mods_root) else {
        return Vec::new();
    };
    for entry in mods.flatten() {
        let mod_dir = entry.path();
        if !mod_dir.is_dir() {
            continue;
        }
        for rel in script_paths(&mod_dir) {
            *providers.entry(rel).or_default() += 1;
        }
    }
    providers
        .into_iter()
        .filter_map(|(path, count)| (count > 1).then_some(path))
        .collect()
}

fn script_paths(mod_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![mod_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ws"))
                && let Ok(rel) = path.strip_prefix(mod_dir)
            {
                out.push(rel.to_string_lossy().replace('\\', "/").to_lowercase());
            }
        }
    }
    out
}

impl GamePlugin for Witcher3Game {
    fn game_id(&self) -> &'static str {
        "witcher3"
    }

    fn display_name(&self) -> &'static str {
        "The Witcher 3: Wild Hunt"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("mods")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        Some(modde_core::paths::home_dir().join("Documents/The Witcher 3/gamesaves"))
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            "292030",
            Path::new("Documents/The Witcher 3/gamesaves"),
        )
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        WITCHER_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        WITCHER_CONTENT_POLICY.classify_extension(ext)
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join("bin/x64")
    }

    fn wine_dll_overrides(&self, game_dir: &Path) -> smallvec::SmallVec<[String; 4]> {
        WITCHER_DLL_POLICY.from_executable_dir(&self.executable_dir(game_dir))
    }

    fn wine_dll_overrides_from_staging(&self, staging: &Path) -> smallvec::SmallVec<[String; 4]> {
        WITCHER_DLL_POLICY.from_staging(staging)
    }

    fn deploy_to_install(&self, staging: &Path, install: &Path) -> Result<()> {
        if ["mods", "dlc", "bin", "content"]
            .iter()
            .any(|root| staging.join(root).exists())
        {
            modde_core::fs::deploy_symlinks(staging, install)
        } else {
            let target = self.mod_root(install)?;
            self.deploy(staging, &target)
        }
    }

    fn archive_extensions(&self) -> &[&str] {
        &["bundle", "cache"]
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(292030)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("witcher3")
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        let roots = present_roots(extracted_dir, &["mods", "dlc", "bin", "content"]);
        if !roots.is_empty() {
            return Some(InstallMethod::MultiRootOverlay { roots });
        }

        std::fs::read_dir(extracted_dir)
            .ok()?
            .flatten()
            .find_map(|entry| {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                (path.is_dir() && name.to_lowercase().starts_with("mod")).then_some({
                    InstallMethod::DirectoryMod {
                        directory_name: Some(name),
                    }
                })
            })
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        WITCHER_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
            || extracted_dir
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.to_lowercase().starts_with("mod"))
    }
}

fn present_roots(dir: &Path, roots: &[&str]) -> Vec<String> {
    roots
        .iter()
        .filter(|root| dir.join(root).exists())
        .map(|root| (*root).to_string())
        .collect()
}
