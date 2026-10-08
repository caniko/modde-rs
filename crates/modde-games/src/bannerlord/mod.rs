//! The Mount & Blade II: Bannerlord game plugin: `Modules` layout plus
//! `SubModule.xml` parsing and dependency checking.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use anyhow::Context;
use modde_core::installer::InstallMethod;

use crate::policies::{BareLayoutPolicy, ContentPolicy};
use crate::traits::{ContentCategory, GamePlugin, ModSafety};

/// [`GamePlugin`] for Mount & Blade II: Bannerlord.
pub struct BannerlordGame;

pub static BANNERLORD: BannerlordGame = BannerlordGame;

const BANNERLORD_SAVE_BREAKING_EXT: &[&str] = &["dll", "xml", "xslt", "xsl", "pak"];
const BANNERLORD_COSMETIC_EXT: &[&str] = &["png", "jpg", "dds", "tga"];
const BANNERLORD_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("dll", ContentCategory::Binary),
    ("xml", ContentCategory::Config),
    ("xslt", ContentCategory::Config),
    ("xsl", ContentCategory::Config),
    ("pak", ContentCategory::Archive),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
];

const BANNERLORD_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: BANNERLORD_SAVE_BREAKING_EXT,
    cosmetic_ext: BANNERLORD_COSMETIC_EXT,
    save_breaking_dirs: &["bin", "submodule.xml"],
    categories: BANNERLORD_CONTENT_CATEGORIES,
};

const BANNERLORD_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &["modules"],
    root_file_exts: &["xml"],
    case_insensitive_dirs: true,
};

/// Module identity and declared dependencies parsed from a `SubModule.xml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BannerlordModuleInfo {
    pub id: String,
    pub name: String,
    pub dependencies: Vec<String>,
}

/// Parse a Bannerlord `SubModule.xml` into a [`BannerlordModuleInfo`].
pub fn parse_submodule_xml(path: &Path) -> anyhow::Result<BannerlordModuleInfo> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let id = attr_after(&content, "<Id", "value").unwrap_or_else(|| {
        path.parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    });
    let name = attr_after(&content, "<Name", "value").unwrap_or_else(|| id.clone());
    let dependencies = content
        .lines()
        .filter(|line| line.contains("<DependedModule"))
        .filter_map(|line| attr_after(line, "<DependedModule", "Id"))
        .collect();
    Ok(BannerlordModuleInfo {
        id,
        name,
        dependencies,
    })
}

/// Find dependencies referenced by modules that are not present in the set,
/// returned as `(module_id, missing_dependency_id)` pairs.
#[must_use]
pub fn missing_dependencies(modules: &[BannerlordModuleInfo]) -> Vec<(String, String)> {
    let available = modules
        .iter()
        .map(|module| module.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let mut missing = Vec::new();
    for module in modules {
        for dependency in &module.dependencies {
            if !available.contains(dependency.as_str()) {
                missing.push((module.id.clone(), dependency.clone()));
            }
        }
    }
    missing
}

fn attr_after(content: &str, marker: &str, attr: &str) -> Option<String> {
    let start = content.find(marker)?;
    let rest = &content[start..];
    let needle = format!("{attr}=\"");
    let value_start = rest.find(&needle)? + needle.len();
    let rest = &rest[value_start..];
    let value_end = rest.find('"')?;
    Some(rest[..value_end].to_string())
}

impl GamePlugin for BannerlordGame {
    fn game_id(&self) -> &'static str {
        "bannerlord"
    }

    fn display_name(&self) -> &'static str {
        "Mount & Blade II: Bannerlord"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("Modules")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        Some(
            modde_core::paths::home_dir()
                .join("Documents/Mount and Blade II Bannerlord/Game Saves/Native"),
        )
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            "261550",
            Path::new("Documents/Mount and Blade II Bannerlord/Game Saves/Native"),
        )
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        BANNERLORD_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        BANNERLORD_CONTENT_POLICY.classify_extension(ext)
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join("bin/Win64_Shipping_Client")
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(261550)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("mountandblade2bannerlord")
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        if extracted_dir.join("SubModule.xml").is_file() {
            return Some(InstallMethod::DirectoryModFromXml {
                marker: PathBuf::from("SubModule.xml"),
                id_attr: "Id.value".to_string(),
                fallback_name: None,
            });
        }
        extracted_dir
            .join("Modules")
            .is_dir()
            .then(|| InstallMethod::StripContentRoot {
                root: "Modules".to_string(),
            })
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        extracted_dir.join("SubModule.xml").is_file()
            || BANNERLORD_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
    }
}
