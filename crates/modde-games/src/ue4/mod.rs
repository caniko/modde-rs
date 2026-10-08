//! Data-driven support for Unreal Engine 4 games: a shared [`Ue4Game`] plugin
//! (pak `~mods` layout, proxy-DLL overrides) parameterised per title.

pub mod saves;
pub mod scanner;

use std::path::{Path, PathBuf};

use modde_core::collision::CollisionSeverity;
use modde_core::installer::InstallMethod;
use modde_core::paths;
use smallvec::SmallVec;

use crate::optiscaler::OptiScalerProfile;
use crate::policies::{CollisionPolicy, ContentPolicy, DllOverridePolicy, StagingDllSearch};
use crate::traits::{ContentCategory, DeployTarget, DeployTargetKind, GamePlugin, ModSafety};

/// Data-driven UE4 game plugin.
///
/// UE4 titles share a near-identical layout: a project folder under the install
/// root (`SB/`, `Palworld/`, etc.) that contains `Content/Paks/` for pak files
/// and `Binaries/Win64/` for the shipping executable and proxy DLLs. Mods drop
/// into `Content/Paks/~mods/` (the tilde forces mount order after base paks).
///
/// A new UE4 game needs only a new `const` instance; the trait impl is shared.
pub struct Ue4Game {
    game_id: &'static str,
    display_name: &'static str,
    /// Steam App ID as a string (parsed on demand for `steam_app_id_u32`).
    steam_app_id: &'static str,
    /// UE4 project short name — the folder under the install root that
    /// contains `Content/Paks/` and `Binaries/Win64/`. For Stellar Blade this
    /// is `"SB"`.
    project_name: &'static str,
    /// Nexus Mods game domain name, if the game is tracked there.
    nexus_domain: Option<&'static str>,
    /// Whether this specific UE4 game participates in modde's per-profile
    /// save layer. This is intentionally per-game; UE4 save semantics vary.
    save_profiles: bool,
}

impl Ue4Game {
    /// Construct a UE4 game definition; save-profile support defaults off
    /// (enable it via [`Ue4Game::with_save_profiles`]).
    #[must_use]
    pub const fn new(
        game_id: &'static str,
        display_name: &'static str,
        steam_app_id: &'static str,
        project_name: &'static str,
        nexus_domain: Option<&'static str>,
    ) -> Self {
        Self {
            game_id,
            display_name,
            steam_app_id,
            project_name,
            nexus_domain,
            save_profiles: false,
        }
    }

    /// Opt this UE4 game into modde's per-profile save layer.
    ///
    /// This is a const builder so game definitions can stay data-only while
    /// avoiding engine-wide assumptions about save-file layout.
    #[must_use]
    pub const fn with_save_profiles(mut self, enabled: bool) -> Self {
        self.save_profiles = enabled;
        self
    }

    /// `<install>/<ProjectName>/Content/Paks`
    #[must_use]
    pub fn paks_root(&self, install: &Path) -> PathBuf {
        install.join(self.project_name).join("Content").join("Paks")
    }

    /// The UE4 project short name (the folder under the install root).
    #[must_use]
    pub fn project_name(&self) -> &'static str {
        self.project_name
    }
}

/// UE4SS / proxy DLLs commonly used by UE4 mod loaders. Detected in
/// `<install>/<ProjectName>/Binaries/Win64/` and surfaced as `WINEDLLOVERRIDES`
/// so Wine loads the native (mod) DLL instead of its built-in stub.
const UE4_PROXY_DLLS: &[&str] = &[
    "dwmapi",    // UE4SS default proxy
    "xinput1_3", // UE4SS alternate + some trainers
    "d3d11",     // ReShade / ENB-style
    "dxgi",      // ReShade / DLSS swappers
    "version",   // Generic ASI loader
    "winmm",     // Generic ASI loader
    "dinput8",   // Generic hook
];

/// File extensions that indicate a UE4 mod alters game logic.
const UE4_SAVE_BREAKING_EXT: &[&str] = &["pak", "ucas", "utoc", "dll", "lua"];

/// File extensions that are purely cosmetic in UE4 games.
const UE4_COSMETIC_EXT: &[&str] = &["png", "jpg", "dds", "tga", "ini"];

const UE4_CONTENT_CATEGORIES: &[(&str, ContentCategory)] = &[
    ("pak", ContentCategory::Archive),
    ("ucas", ContentCategory::Archive),
    ("utoc", ContentCategory::Archive),
    ("dll", ContentCategory::Binary),
    ("so", ContentCategory::Binary),
    ("lua", ContentCategory::Script),
    ("dds", ContentCategory::Texture),
    ("png", ContentCategory::Texture),
    ("tga", ContentCategory::Texture),
    ("jpg", ContentCategory::Texture),
    ("ini", ContentCategory::Config),
    ("json", ContentCategory::Config),
    ("yaml", ContentCategory::Config),
    ("xml", ContentCategory::Config),
    ("toml", ContentCategory::Config),
];

const UE4_CONTENT_POLICY: ContentPolicy = ContentPolicy {
    save_breaking_ext: UE4_SAVE_BREAKING_EXT,
    cosmetic_ext: UE4_COSMETIC_EXT,
    save_breaking_dirs: &[],
    categories: UE4_CONTENT_CATEGORIES,
};

const UE4_DLL_POLICY: DllOverridePolicy = DllOverridePolicy {
    proxy_dlls: UE4_PROXY_DLLS,
    staging_search: StagingDllSearch::DirectChildDirs,
};

pub(crate) const UE4_COLLISION_SEVERITIES: &[(&str, CollisionSeverity)] = &[
    ("pak", CollisionSeverity::Dangerous),
    ("ucas", CollisionSeverity::Dangerous),
    ("utoc", CollisionSeverity::Dangerous),
    ("dll", CollisionSeverity::Dangerous),
    ("lua", CollisionSeverity::Dangerous),
    ("ini", CollisionSeverity::Config),
    ("cfg", CollisionSeverity::Config),
    ("json", CollisionSeverity::Config),
    ("toml", CollisionSeverity::Config),
    ("xml", CollisionSeverity::Config),
    ("yaml", CollisionSeverity::Config),
    ("dds", CollisionSeverity::Cosmetic),
    ("png", CollisionSeverity::Cosmetic),
    ("jpg", CollisionSeverity::Cosmetic),
    ("tga", CollisionSeverity::Cosmetic),
];

pub(crate) const UE4_COLLISION_POLICY: CollisionPolicy = CollisionPolicy {
    archive_extensions: &["pak", "ucas", "utoc"],
    severities: UE4_COLLISION_SEVERITIES,
};

pub const STELLAR_BLADE: Ue4Game = Ue4Game::new(
    "stellar-blade",
    "Stellar Blade",
    "3489700",
    "SB",
    Some("stellarblade"),
)
.with_save_profiles(true);

pub const SUBNAUTICA2: Ue4Game = Ue4Game::new(
    "subnautica2",
    "Subnautica 2",
    "1962700",
    "Subnautica2",
    Some("subnautica2"),
)
.with_save_profiles(true);

pub(crate) const STELLAR_BLADE_OPTISCALER_PROFILES: &[OptiScalerProfile] = &[OptiScalerProfile {
    id: "community-dxgi",
    name: "Community tested dxgi.dll",
    source_url: "https://github.com/optiscaler/OptiScaler/wiki/Stellar-Blade",
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
    notes: "Use OptiPatcher to unlock DLSS and DLSS-FG inputs without spoofing. The community compatibility notes report that the game may crash on first boot but work afterwards, and that setting the in-game sharpness slider to 0 can fix DLSSG HUD interpolation.",
}];

impl GamePlugin for Ue4Game {
    fn game_id(&self) -> &str {
        self.game_id
    }

    fn display_name(&self) -> &str {
        self.display_name
    }

    /// Deploy target: `<install>/<ProjectName>/Content/Paks/~mods`.
    ///
    /// The tilde prefix forces UE4's pak mounter to load these after the
    /// shipping paks so mods can override base content.
    fn mod_directory(&self, install: &Path) -> PathBuf {
        self.paks_root(install).join("~mods")
    }

    fn save_directory(&self) -> Option<PathBuf> {
        let compat = paths::steam_common()
            .parent()?
            .join("compatdata")
            .join(self.steam_app_id)
            .join("pfx/drive_c/users/steamuser/AppData/Local")
            .join(self.project_name)
            .join("Saved/SaveGames");
        if compat.exists() {
            return Some(compat);
        }
        None
    }

    fn supports_save_profiles(&self) -> bool {
        self.save_profiles
    }

    fn save_directory_at(&self, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        crate::library::context::steam_user_path(
            install,
            prefix,
            self.steam_app_id,
            &Path::new("AppData/Local")
                .join(self.project_name)
                .join("Saved/SaveGames"),
        )
    }

    fn deploy_targets(&self) -> &'static [DeployTarget] {
        &[DeployTarget {
            id: "ue4-saved-config",
            label: "UE4 Saved/Config",
            kind: DeployTargetKind::UserConfig,
        }]
    }

    /// Resolve the per-user config dir under the Proton prefix:
    /// `<steam_root>/compatdata/<APP_ID>/pfx/drive_c/users/steamuser/`
    /// `AppData/Local/<Project>/Saved/Config/Windows/`.
    ///
    /// Returns `None` if the prefix doesn't exist yet — the caller is
    /// expected to surface that to the user (typically: launch the
    /// game once so Proton creates the prefix).
    fn resolve_deploy_target(&self, id: &str, install: &Path) -> Option<PathBuf> {
        self.resolve_deploy_target_at(id, install, None)
    }

    fn resolve_deploy_target_at(
        &self,
        id: &str,
        install: &Path,
        prefix: Option<&Path>,
    ) -> Option<PathBuf> {
        if id != "ue4-saved-config" {
            return None;
        }
        let prefix = prefix
            .map(Path::to_path_buf)
            .or_else(|| crate::library::context::steam_prefix(install, self.steam_app_id))?;
        if !prefix.exists() {
            return None;
        }
        Some(
            prefix
                .join("drive_c/users/steamuser/AppData/Local")
                .join(self.project_name)
                .join("Saved/Config/Windows"),
        )
    }

    fn executable_dir(&self, install: &Path) -> PathBuf {
        install
            .join(self.project_name)
            .join("Binaries")
            .join("Win64")
    }

    fn wine_dll_overrides(&self, game_dir: &Path) -> SmallVec<[String; 4]> {
        UE4_DLL_POLICY.from_executable_dir(&self.executable_dir(game_dir))
    }

    fn wine_dll_overrides_from_staging(&self, staging: &Path) -> SmallVec<[String; 4]> {
        UE4_DLL_POLICY.from_staging(staging)
    }

    fn classify_mod(&self, mod_dir: &Path) -> ModSafety {
        UE4_CONTENT_POLICY.classify_mod(mod_dir)
    }

    fn classify_extension(&self, ext: &str) -> ContentCategory {
        UE4_CONTENT_POLICY.classify_extension(ext)
    }

    fn archive_extensions(&self) -> &[&str] {
        &["pak", "ucas", "utoc"]
    }

    fn steam_app_id_u32(&self) -> Option<u32> {
        self.steam_app_id.parse().ok()
    }

    fn nexus_game_domain(&self) -> Option<&str> {
        self.nexus_domain
    }

    fn analyze_mod_archive(&self, extracted_dir: &Path) -> Option<InstallMethod> {
        has_root_file_with_ext(extracted_dir, &["pak", "ucas", "utoc"])
            .then_some(InstallMethod::SingleFileSet)
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
