//! The Baldur's Gate 3 game plugin: Larian `.pak` mod layout, `modsettings.lsx`
//! load-order management, and Proton-prefix path resolution.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use modde_core::installer::InstallMethod;

use crate::policies::{BareLayoutPolicy, ContentPolicy};
use crate::traits::{ContentCategory, GamePlugin, HotDeployCapability, ModSafety};

/// [`GamePlugin`] for Baldur's Gate 3 (Larian's Divinity engine).
pub struct LarianBg3Game;

pub static BALDURS_GATE3: LarianBg3Game = LarianBg3Game;

const STEAM_APP_ID: &str = "1086940";

const BG3_SAVE_BREAKING_EXT: &[&str] = &["pak", "dll", "json", "lsx"];
const BG3_COSMETIC_EXT: &[&str] = &["png", "jpg", "dds", "tga"];
const BG3_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("pak", ContentCategory::Archive),
    ("dll", ContentCategory::Binary),
    ("json", ContentCategory::Config),
    ("lsx", ContentCategory::Config),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
];

const BG3_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: BG3_SAVE_BREAKING_EXT,
    cosmetic_ext: BG3_COSMETIC_EXT,
    save_breaking_dirs: &["script extender", "scriptextender"],
    categories: BG3_CONTENT_CATEGORIES,
};

const BG3_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &["mods", "playerprofiles"],
    root_file_exts: &["pak", "lsx"],
    case_insensitive_dirs: true,
};

/// Resolve BG3's `AppData/Local` data root, preferring the Steam Proton prefix
/// derived from `install` and falling back to an install-relative path.
#[must_use]
pub fn data_root_from_install(install: &Path) -> PathBuf {
    let proton = install
        .ancestors()
        .find(|path| path.file_name().and_then(|name| name.to_str()) == Some("common"))
        .and_then(|common| common.parent())
        .map(|steamapps| {
            steamapps
                .join("compatdata")
                .join(STEAM_APP_ID)
                .join("pfx/drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3")
        });

    proton.unwrap_or_else(|| install.join("Larian Studios/Baldur's Gate 3"))
}

/// The BG3 `Mods` directory derived from [`data_root_from_install`].
#[must_use]
pub fn mods_dir_from_install(install: &Path) -> PathBuf {
    data_root_from_install(install).join("Mods")
}

/// Path to the `modsettings.lsx` load-order file derived from `install`.
#[must_use]
pub fn modsettings_path_from_install(install: &Path) -> PathBuf {
    data_root_from_install(install).join("PlayerProfiles/Public/modsettings.lsx")
}

/// BG3 savegame directory inside the default Steam Proton prefix, if it exists.
#[must_use]
pub fn save_dir_from_steam_default() -> Option<PathBuf> {
    Some(
        modde_core::paths::steam_common()
            .parent()?
            .join("compatdata")
            .join(STEAM_APP_ID)
            .join(
                "pfx/drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3/PlayerProfiles/Public/Savegames",
            ),
    )
}

/// Read the ordered list of enabled module folders from a `modsettings.lsx` file.
pub fn read_modsettings(path: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    Ok(content
        .lines()
        .filter_map(|line| line.split_once("value=\""))
        .filter_map(|(_, rest)| rest.split_once('"'))
        .map(|(value, _)| value.to_string())
        .filter(|value| !value.is_empty())
        .collect())
}

/// Write a `modsettings.lsx` file enabling the given module folders in order.
pub fn write_modsettings(path: &Path, mods: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let module_nodes = mods
        .iter()
        .map(|name| format!("          <node id=\"ModuleShortDesc\"><attribute id=\"Folder\" type=\"LSString\" value=\"{name}\" /></node>"))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(
        path,
        format!(
            r#"<?xml version="1.0" encoding="utf-8"?>
<save>
  <region id="ModuleSettings">
    <node id="root">
      <children>
{module_nodes}
      </children>
    </node>
  </region>
</save>
"#
        ),
    )
    .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

impl GamePlugin for LarianBg3Game {
    fn game_id(&self) -> &'static str {
        "baldurs-gate3"
    }

    fn display_name(&self) -> &'static str {
        "Baldur's Gate 3"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        mods_dir_from_install(install)
    }

    fn mod_root(&self, install: &Path) -> Result<PathBuf> {
        Ok(mods_dir_from_install(install))
    }

    fn mod_root_at(&self, install: &Path, prefix: Option<&Path>) -> Result<PathBuf> {
        if let Some(prefix) = prefix {
            return Ok(prefix.join(
                "drive_c/users/steamuser/AppData/Local/Larian Studios/Baldur's Gate 3/Mods",
            ));
        }
        self.mod_root(install)
    }

    fn deploy_to_install_at(
        &self,
        staging: &Path,
        install: &Path,
        prefix: Option<&Path>,
    ) -> Result<()> {
        self.deploy(staging, &self.mod_root_at(install, prefix)?)
    }

    fn save_directory(&self) -> Option<PathBuf> {
        save_dir_from_steam_default()
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            STEAM_APP_ID,
            Path::new(
                "AppData/Local/Larian Studios/Baldur's Gate 3/PlayerProfiles/Public/Savegames",
            ),
        )
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        BG3_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        BG3_CONTENT_POLICY.classify_extension(ext)
    }

    fn archive_extensions(&self) -> &[&str] {
        &["pak"]
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join("bin")
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(1086940)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("baldursgate3")
    }

    fn post_deploy(&self, install: &Path) -> Result<()> {
        self.post_deploy_at(install, None)
    }

    fn post_deploy_at(&self, install: &Path, prefix: Option<&Path>) -> Result<()> {
        let mods_dir = self.mod_root_at(install, prefix)?;
        let mut mods = Vec::new();
        if mods_dir.is_dir() {
            for entry in std::fs::read_dir(&mods_dir)
                .with_context(|| format!("failed to read directory: {}", mods_dir.display()))?
                .flatten()
            {
                let path = entry.path();
                if path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("pak"))
                    && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
                {
                    mods.push(stem.to_string());
                }
            }
        }
        mods.sort();
        if !mods.is_empty() {
            let data_root = mods_dir
                .parent()
                .context("BG3 mod directory has no data root")?;
            write_modsettings(
                &data_root.join("PlayerProfiles/Public/modsettings.lsx"),
                &mods,
            )?;
        }
        Ok(())
    }

    fn hot_deploy_capability(&self) -> HotDeployCapability {
        HotDeployCapability::experimental_cosmetic_only()
    }

    fn apply_hot_deploy_patch(
        &self,
        patch: &modde_core::hot_deploy::HotDeployPatch,
        staging: &Path,
        install: &Path,
    ) -> Result<()> {
        modde_core::hot_deploy::apply_patch_to_staging(staging, patch)?;
        let target = self.mod_directory(install);
        std::fs::create_dir_all(&target)
            .with_context(|| format!("failed to create {}", target.display()))?;
        for root in patch.touched_roots() {
            let src = staging.join(&root);
            let dst = target.join(&root);
            if dst.symlink_metadata().is_ok() {
                modde_core::hot_deploy::remove_path(&dst)?;
            }
            if src.symlink_metadata().is_ok() {
                modde_core::fs::symlink(&src, &dst).with_context(|| {
                    format!("failed to refresh BG3 hot-deploy root {}", dst.display())
                })?;
            }
        }
        self.post_deploy(install)
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        if has_root_file_with_ext(extracted_dir, &["pak"]) {
            return Some(InstallMethod::SingleFileSet);
        }
        extracted_dir
            .join("Mods")
            .is_dir()
            .then(|| InstallMethod::StripContentRoot {
                root: "Mods".to_string(),
            })
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        BG3_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
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
