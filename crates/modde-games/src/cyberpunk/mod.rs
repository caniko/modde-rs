//! The Cyberpunk 2077 game plugin: `REDengine` mod layout, `REDmod` deploy,
//! and the associated scanner, save tracker, and collision classifier.

pub mod collision;
pub mod manifest;
pub mod redmod;
pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use smallvec::SmallVec;

use crate::optiscaler::OptiScalerProfile;
use crate::policies::{BareLayoutPolicy, ContentPolicy, DllOverridePolicy, StagingDllSearch};
use crate::traits::{ContentCategory, GamePlugin, HotDeployCapability, ModSafety};

/// [`GamePlugin`] for Cyberpunk 2077 (`REDengine` 4).
pub struct Cyberpunk2077;

pub static CYBERPUNK2077: Cyberpunk2077 = Cyberpunk2077;

pub(crate) const CYBERPUNK_OPTISCALER_PROFILES: &[OptiScalerProfile] = &[OptiScalerProfile {
    id: "community-dxgi",
    name: "Community tested dxgi.dll",
    source_url: "https://github.com/optiscaler/OptiScaler/wiki/Cyberpunk-2077",
    tested_optiscaler_version: "0.9",
    source_mode: Some("github_release"),
    goverlay_channel: None,
    proxy_dll: "dxgi.dll",
    release_tag: Some("official:v0.9.1"),
    release_asset: None,
    wine_dll_overrides: &[],
    copy_companion_files: true,
    enable_optipatcher: true,
    fsr4_variant: Some("latest_fp8"),
    emulate_fp8: false,
    spoof_dlss: false,
    ini_overrides: &[],
    notes: "Use OptiPatcher to unlock DLSS and DLSS-FG inputs without spoofing. Cyberpunk 2077's DLSS path is the primary OptiScaler input. The game may need DLSS-FG toggled on in the in-game menu for frame generation to activate.",
}];

/// File extensions that indicate a mod alters game logic.
const CYBERPUNK_SAVE_BREAKING_EXT: &[&str] = &["reds", "lua", "tweak", "xl", "yaml", "yls"];

/// Directories within a mod that signal save-breaking content.
const CYBERPUNK_SAVE_BREAKING_DIRS: &[&str] = &[
    "r6/scripts",
    "r6/tweaks",
    "bin/x64/plugins/cyber_engine_tweaks/mods",
];

/// Extensions that are purely cosmetic.
const CYBERPUNK_COSMETIC_EXT: &[&str] = &["archive", "xl", "png", "jpg", "dds", "tga", "ini"];

const CYBERPUNK_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("archive", ContentCategory::Archive),
    ("dll", ContentCategory::Binary),
    ("so", ContentCategory::Binary),
    ("reds", ContentCategory::Script),
    ("lua", ContentCategory::Script),
    ("tweak", ContentCategory::Script),
    ("xl", ContentCategory::Script),
    ("yaml", ContentCategory::Config),
    ("yls", ContentCategory::Config),
    ("yml", ContentCategory::Config),
    ("ini", ContentCategory::Config),
    ("json", ContentCategory::Config),
    ("toml", ContentCategory::Config),
    ("xml", ContentCategory::Config),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
];

const CYBERPUNK_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: CYBERPUNK_SAVE_BREAKING_EXT,
    cosmetic_ext: CYBERPUNK_COSMETIC_EXT,
    save_breaking_dirs: CYBERPUNK_SAVE_BREAKING_DIRS,
    categories: CYBERPUNK_CONTENT_CATEGORIES,
};

/// Windows system DLLs commonly hijacked by mod frameworks as proxy/hook DLLs.
/// When present in the game's executable directory, these need Wine `n,b` overrides
/// so Wine loads the native (mod) version instead of its built-in stub.
const KNOWN_PROXY_DLLS: &[&str] = &[
    "version",   // CET (Cyber Engine Tweaks), ASI loaders
    "winmm",     // ASI loader, some mod frameworks
    "dinput8",   // Various mod frameworks
    "d3d11",     // ReShade, ENB
    "dxgi",      // OptiScaler, ReShade (often handled by fgmod)
    "winhttp",   // Some mod loaders
    "xinput1_3", // Controller hook mods
];

const CYBERPUNK_DLL_POLICY: DllOverridePolicy = DllOverridePolicy {
    proxy_dlls: KNOWN_PROXY_DLLS,
    staging_search: StagingDllSearch::NestedModsBinX64,
};

const CYBERPUNK_BARE_LAYOUT_POLICY: BareLayoutPolicy = BareLayoutPolicy {
    root_dirs: &[
        "r6", "archive", "archives", "bin", "engine", "mods", "red4ext",
    ],
    root_file_exts: &[],
    case_insensitive_dirs: false,
};

impl GamePlugin for Cyberpunk2077 {
    fn game_id(&self) -> &'static str {
        "cyberpunk2077"
    }

    fn display_name(&self) -> &'static str {
        "Cyberpunk 2077"
    }

    fn mod_directory(&self, install: &Path) -> PathBuf {
        install.join("mods")
    }

    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        if !target.exists() {
            std::fs::create_dir_all(target)
                .with_context(|| format!("failed to create {}", target.display()))?;
        }
        // Symlink each mod directory from staging into game mods dir
        for entry in std::fs::read_dir(staging)
            .with_context(|| format!("failed to read directory: {}", staging.display()))?
        {
            let entry = entry?;
            let dst = target.join(entry.file_name());
            if dst.exists() || dst.symlink_metadata().is_ok() {
                if dst.is_dir() {
                    std::fs::remove_dir_all(&dst)
                        .with_context(|| format!("failed to remove {}", dst.display()))?;
                } else {
                    std::fs::remove_file(&dst)
                        .with_context(|| format!("failed to remove {}", dst.display()))?;
                }
            }
            modde_core::fs::symlink(&entry.path(), &dst)?;
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
                    format!(
                        "failed to refresh Cyberpunk hot-deploy root {}",
                        dst.display()
                    )
                })?;
            }
        }

        Ok(())
    }

    fn post_deploy(&self, install: &Path) -> Result<()> {
        // Run REDmod deploy if available
        redmod::deploy_if_available(install)
    }

    fn save_directory(&self) -> Option<PathBuf> {
        let save_suffix = "pfx/drive_c/users/steamuser/Saved Games/CD Projekt Red/Cyberpunk 2077";

        // Check Heroic prefixes (GOG / sideload)
        let heroic_prefixes =
            modde_core::paths::home_dir().join("Games/Heroic/Prefixes/default/Cyberpunk 2077");
        let heroic_path = heroic_prefixes.join(save_suffix);
        if heroic_path.exists() {
            return Some(heroic_path);
        }

        // Steam Proton prefix
        let compat = modde_core::paths::steam_common()
            .parent()? // steamapps/
            .join("compatdata/1091500")
            .join(save_suffix);
        if compat.exists() {
            return Some(compat);
        }
        None
    }

    fn supports_save_profiles(&self) -> bool {
        true
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            "1091500",
            Path::new("Saved Games/CD Projekt Red/Cyberpunk 2077"),
        )
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        CYBERPUNK_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        CYBERPUNK_CONTENT_POLICY.classify_extension(ext)
    }

    fn wine_dll_overrides(&self, game_dir: &Path) -> SmallVec<[String; 4]> {
        CYBERPUNK_DLL_POLICY.from_executable_dir(&self.executable_dir(game_dir))
    }

    fn wine_dll_overrides_from_staging(&self, staging: &Path) -> SmallVec<[String; 4]> {
        CYBERPUNK_DLL_POLICY.from_staging(staging)
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.join("bin").join("x64")
    }

    fn archive_extensions(&self) -> &[&str] {
        &["archive"]
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        Some(1091500)
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        Some("cyberpunk2077")
    }

    fn nexus_game_id_u32(&self) -> Option<u32> {
        // Nexus Mods v2 GraphQL game ID for Cyberpunk 2077.
        // Source: https://api.nexusmods.com/v1/games.json
        Some(3333)
    }

    fn analyze_mod_archive(
        &self,
        extracted_dir: &Path,
    ) -> Option<modde_core::installer::InstallMethod> {
        // REDmod signature: top-level `info.json` + `archives/` subdir.
        // The `archives/` dir may also be spelled `archive/` on some
        // mods; check both.
        let info_json = extracted_dir.join("info.json");
        if !info_json.is_file() {
            return None;
        }
        let archives = extracted_dir.join("archives");
        let archive = extracted_dir.join("archive");
        if archives.is_dir() || archive.is_dir() {
            return Some(modde_core::installer::InstallMethod::REDmod {
                manifest: PathBuf::from("info.json"),
            });
        }
        None
    }

    fn recognizes_bare_layout(&self, extracted_dir: &Path) -> bool {
        // Cyberpunk mods drop loose into one of these top-level dirs.
        // If any of them exist at the extraction root, treat the archive
        // as a bare extract — the deploy step will symlink into
        // `<install>/mods/<name>/` via the REDmod loader.
        CYBERPUNK_BARE_LAYOUT_POLICY.recognizes(extracted_dir)
    }
}
