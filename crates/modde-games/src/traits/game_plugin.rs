use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use smallvec::SmallVec;

use super::{ContentCategory, ContentSummary, DeployTarget, HotDeployCapability, ModSafety};

/// Trait implemented by each supported game.
pub trait GamePlugin: Send + Sync {
    /// Unique game identifier (e.g. "skyrim-se").
    fn game_id(&self) -> &str;

    /// Human-readable display name.
    fn display_name(&self) -> &str;

    /// Attempt to detect the game's install location.
    /// Default: delegates to `detection::find_game_install(self.game_id())`.
    fn detect_install(&self) -> Option<PathBuf> {
        crate::detection::find_game_install(&modde_core::GameId::from(self.game_id()))
    }

    /// Return the mod directory relative to the install path.
    fn mod_directory(&self, install: &Path) -> PathBuf;

    /// Resolve the actual deployment root for enabled mods.
    ///
    /// Most supported games use a directory under the install root, so the
    /// default delegates to [`GamePlugin::mod_directory`]. Games whose mod
    /// loader reads from a user-data path (for example Proton `AppData`) can
    /// override this without breaking existing install-relative callers.
    fn mod_root(&self, install: &Path) -> Result<PathBuf> {
        Ok(self.mod_directory(install))
    }

    /// Prefix-aware deployment root; install-relative games use the default.
    fn mod_root_at(&self, install: &Path, _prefix: Option<&Path>) -> Result<PathBuf> {
        self.mod_root(install)
    }

    /// Deploy staged mods into the game's mod directory.
    /// Default: recursive symlink farm via `modde_core::fs::deploy_symlinks`.
    fn deploy(&self, staging: &Path, target: &Path) -> Result<()> {
        modde_core::fs::deploy_symlinks(staging, target)
    }

    /// Deploy staged mods using the game install root as context.
    ///
    /// The default resolves [`GamePlugin::mod_root`] and delegates to
    /// [`GamePlugin::deploy`]. Games that stage multi-root overlays can
    /// override this to deploy to several install-relative destinations.
    fn deploy_to_install(&self, staging: &Path, install: &Path) -> Result<()> {
        let target = self.mod_root(install)?;
        self.deploy(staging, &target)
    }

    /// Preserve multi-root deployment strategies while allowing prefix-scoped
    /// games to override their user-data target.
    fn deploy_to_install_at(&self, staging: &Path, install: &Path, _prefix: Option<&Path>) -> Result<()> {
        self.deploy_to_install(staging, install)
    }

    /// Whether this game supports experimental path-level live VFS patching.
    fn hot_deploy_capability(&self) -> HotDeployCapability {
        HotDeployCapability::unsupported()
    }

    /// Apply an already validated hot-deploy patch to the profile staging tree
    /// and the live game deployment target.
    fn apply_hot_deploy_patch(
        &self,
        _patch: &modde_core::hot_deploy::HotDeployPatch,
        _staging: &Path,
        _install: &Path,
    ) -> Result<()> {
        bail!("hot-deploy is not supported for {}", self.display_name())
    }

    /// Run any post-deployment steps (e.g. `REDmod` deploy).
    fn post_deploy(&self, _install: &Path) -> Result<()> {
        Ok(())
    }

    /// Run post-deployment configuration against the selected prefix.
    fn post_deploy_at(&self, install: &Path, _prefix: Option<&Path>) -> Result<()> {
        self.post_deploy(install)
    }

    /// Return the save directory for this game, if known.
    fn save_directory(&self) -> Option<PathBuf> {
        None
    }

    /// Exact-install save discovery. Return None when this plugin cannot resolve
    /// the supplied prefix; callers can then require an explicit save path.
    fn save_directory_at(&self, _install: &Path, _prefix: Option<&Path>) -> Option<PathBuf> {
        None
    }

    /// Alternate deployment roots this game exposes to the installer
    /// (e.g. user-config dirs for INI tweak packs). Default: none, in
    /// which case the installer only ever stages into the game install
    /// dir. The order is significant: when the analyzer needs to pick
    /// a default target for a given [`super::DeployTargetKind`] it takes the
    /// first one of that kind.
    fn deploy_targets(&self) -> &'static [DeployTarget] {
        &[]
    }

    /// Resolve a [`DeployTarget::id`] this plugin advertises to a real
    /// filesystem path, using the live `install` dir for any path that
    /// must be derived from it (e.g. Steam `compatdata` adjacent to
    /// `steamapps/common/<game>`). Returns `None` if the target id is
    /// unknown to this plugin or the path cannot be resolved on this
    /// system (e.g. the Wine prefix doesn't exist yet).
    fn resolve_deploy_target(&self, _id: &str, _install: &Path) -> Option<PathBuf> {
        None
    }

    /// Prefix-aware counterpart used by installation-scoped deployment.
    fn resolve_deploy_target_at(&self, id: &str, install: &Path, prefix: Option<&Path>) -> Option<PathBuf> {
        if prefix.is_some() { return None; }
        self.resolve_deploy_target(id, install)
    }

    /// Whether this game participates in modde's per-profile save layer.
    ///
    /// Disabled games still support normal profile/mod management, but modde
    /// must not swap saves, compute save fingerprints, or expose save commands
    /// for them.
    fn supports_save_profiles(&self) -> bool {
        false
    }

    /// Classify whether a mod is save-breaking based on its installed content.
    ///
    /// `mod_dir` is the path to the mod's staging directory. The game plugin
    /// inspects the files within to determine if the mod alters game logic
    /// (scripts, plugins, tweaks) or is purely cosmetic (textures, meshes).
    ///
    /// Default: `Unknown` (conservative — included in fingerprints).
    fn classify_mod(&self, _mod_dir: &Path) -> ModSafety {
        ModSafety::Unknown
    }

    /// Scan the game directory for proxy/hook DLLs that need Wine `n,b` overrides.
    ///
    /// Returns DLL base names (without extension) that should be added to
    /// `WINEDLLOVERRIDES` as `name=n,b` so Wine loads the native version
    /// instead of its built-in stub.
    fn wine_dll_overrides(&self, _game_dir: &Path) -> SmallVec<[String; 4]> {
        SmallVec::new()
    }

    /// Scan the staging directory for proxy DLLs that mods deploy.
    /// This catches DLLs that may have been deleted by other tools (e.g. fgmod)
    /// from the game directory but are still needed.
    fn wine_dll_overrides_from_staging(&self, _staging: &Path) -> SmallVec<[String; 4]> {
        SmallVec::new()
    }

    /// Return the directory containing the game executable, relative to the install root.
    /// Used to locate proxy DLLs that need Wine overrides.
    fn executable_dir(&self, install: &Path) -> PathBuf {
        install.to_path_buf()
    }

    // ── DRY trait methods ─────────────────────────────────────────
    fn ini_file_names(&self) -> &[&str] {
        &[]
    }
    fn archive_extensions(&self) -> &[&str] {
        &[]
    }
    fn has_plugin_system(&self) -> bool {
        false
    }
    fn steam_app_id_u32(&self) -> Option<u32> {
        None
    }
    fn plugins_txt_folder(&self) -> Option<&str> {
        None
    }
    fn nexus_game_domain(&self) -> Option<&str> {
        None
    }

    /// Numeric Nexus game ID. Required by the GraphQL v2 API for
    /// browse/search queries (which take `gameId: Int`, not a domain
    /// string). Games that only speak REST can leave this `None`.
    fn nexus_game_id_u32(&self) -> Option<u32> {
        None
    }

    // ── Install-method detection (V8 installer pipeline) ────────

    /// Claim an extracted archive as a game-specific install method.
    ///
    /// Runs **before** the generic probes (FOMOD, BAIN, DLL overlay) in
    /// [`analyze`](modde_core::installer::analyze()), so a game can authoritatively
    /// identify layouts it knows about — e.g. Cyberpunk recognizing a
    /// `REDmod` by `info.json` + `archives/` presence, or ENB for Bethesda.
    ///
    /// Return `None` to fall through to the generic probes.
    fn analyze_mod_archive(
        &self,
        _extracted_dir: &Path,
    ) -> Option<modde_core::installer::InstallMethod> {
        None
    }

    /// Decide whether an extracted archive drops cleanly into the game's
    /// mod dir without any staging (e.g. a Skyrim archive with a
    /// top-level `Data/` directory, or a Cyberpunk archive with `r6/`).
    ///
    /// Called as the last fallback by
    /// [`analyze`](modde_core::installer::analyze()) — if this returns `true` the
    /// plan becomes `InstallMethod::BareExtract`, otherwise the analyzer
    /// falls through to [`InstallMethod::Unknown`](modde_core::installer::InstallMethod::Unknown) and the caller dumps
    /// a dossier for the skill path.
    fn recognizes_bare_layout(&self, _extracted_dir: &Path) -> bool {
        false
    }

    /// Classify a file extension into a content category.
    fn classify_extension(&self, ext: &str) -> ContentCategory {
        match ext {
            "esp" | "esm" | "esl" => ContentCategory::Plugin,
            "dds" | "png" | "tga" | "jpg" => ContentCategory::Texture,
            "nif" => ContentCategory::Mesh,
            "wav" | "xwm" | "fuz" | "mp3" | "ogg" => ContentCategory::Sound,
            "pex" | "psc" | "reds" | "lua" => ContentCategory::Script,
            "swf" => ContentCategory::Interface,
            "bsa" | "ba2" | "archive" => ContentCategory::Archive,
            "ini" | "json" | "yaml" | "xml" | "toml" => ContentCategory::Config,
            "dll" | "so" => ContentCategory::Binary,
            _ => ContentCategory::Other,
        }
    }

    /// Scan a mod directory and return a content summary.
    fn summarize_content(&self, mod_dir: &Path) -> ContentSummary {
        let mut summary = ContentSummary::default();
        let mut stack = vec![mod_dir.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = match std::fs::read_dir(&dir) {
                Ok(e) => e,
                Err(_) => continue,
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let cat = self.classify_extension(&ext.to_lowercase());
                    *summary.counts.entry(cat).or_insert(0) += 1;
                }
            }
        }
        summary
    }
}
