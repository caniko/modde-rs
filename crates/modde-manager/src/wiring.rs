//! Declarative runtime wiring for launchable game instances (OctoWoW HD/Lutris).
//!
//! `status` and `plan` are read-only: they report verified/missing/mismatched/
//! unverifiable items and never execute Wine, Lutris, or the launcher.
//! `apply` owns exactly three idempotent mutations: Wine prefix creation via
//! `wineboot`, the Lutris game yml, and the Lutris `pga.db` row (backup-first,
//! Lutris-closed gate). It never touches client binaries, MPQs, or game data.

use super::*;
use crate::transaction::overlap;
use files::{Anchor, Image, Lease, fd_path, relative};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Optional per-instance runtime wiring. All fields optional so existing
/// configs keep parsing; bumping `managerSchemaVersion` to 5 advertises
/// launcher tuning overrides, offline validation, and the tightened
/// registration/readiness contract.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Wiring {
    #[serde(default)]
    pub runtime: Runtime,
    #[serde(default)]
    pub tunings: Tunings,
    #[serde(default)]
    pub dll_overrides: Vec<String>,
    #[serde(default)]
    pub launch: Launch,
    #[serde(default)]
    pub prefix: Option<Prefix>,
    #[serde(default)]
    pub lutris: Option<LutrisEntry>,
    #[serde(default)]
    pub client_integrity: Option<ClientIntegrity>,
    #[serde(default)]
    pub data_patches: Option<DataPatches>,
    #[serde(default)]
    pub endpoints: Option<Endpoints>,
    #[serde(default)]
    pub launcher: Option<LauncherState>,
    #[serde(default)]
    pub client_version: Option<ClientVersion>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runtime {
    #[serde(default = "default_runtime_kind")]
    pub kind: String,
    #[serde(default = "default_runtime_version")]
    pub version: String,
    #[serde(default = "default_runtime_arch")]
    pub arch: String,
    #[serde(default = "default_anticheat")]
    pub anticheat: bool,
}

impl Default for Runtime {
    fn default() -> Self {
        Self {
            kind: default_runtime_kind(),
            version: default_runtime_version(),
            arch: default_runtime_arch(),
            anticheat: default_anticheat(),
        }
    }
}

fn default_runtime_kind() -> String {
    "wine".into()
}
fn default_runtime_version() -> String {
    "latest".into()
}
fn default_runtime_arch() -> String {
    "wow64".into()
}
fn default_anticheat() -> bool {
    false
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tunings {
    #[serde(default = "default_true")]
    pub dxvk: bool,
    #[serde(default)]
    pub vkd3d: bool,
    #[serde(default = "default_true")]
    pub esync: bool,
    #[serde(default = "default_true")]
    pub fsync: bool,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Launch {
    #[serde(default = "default_launch_exe")]
    pub executable: String,
}

fn default_launch_exe() -> String {
    "VanillaFixes.exe".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Prefix {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LutrisEntry {
    pub slug: String,
    #[serde(default = "default_lutris_name")]
    pub name: String,
    #[serde(default = "default_game_slug")]
    pub game_slug: String,
    /// System command prefix (Lutris `system.prefix_command`): prepended to
    /// the wine command. OctoWoW launcher uses the packaged stdio-repair
    /// wrapper; a declared game-entry wrapper stays in front of the
    /// synthesized launch gate (see `game_gate_prefix`) instead of
    /// replacing it.
    #[serde(default)]
    pub command_prefix: Option<String>,
}

fn default_lutris_name() -> String {
    "OctoWoW".into()
}

fn default_game_slug() -> String {
    "octowow".into()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientIntegrity {
    #[serde(default)]
    pub require_files: Vec<String>,
    #[serde(default)]
    pub wow_exe: Option<WowExe>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WowExe {
    pub size: u64,
    pub sha256: String,
    #[serde(default = "default_true")]
    pub laa: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataPatches {
    #[serde(default)]
    pub native_letters: Vec<String>,
    #[serde(default)]
    pub patch_a: Option<PatchExpectation>,
    #[serde(default = "default_true")]
    pub forbid_renames: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchExpectation {
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    #[serde(default)]
    pub assert_realmlist: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LauncherState {
    #[serde(default)]
    pub installer: Option<InstallerExpectation>,
    #[serde(default)]
    pub prefix: Option<PathBuf>,
    #[serde(default)]
    pub tweaks: BTreeMap<String, bool>,
    /// Installed launcher executable (under the launcher prefix). When
    /// present the Lutris entry launches it; otherwise the installer
    /// (first-run path before the launcher exists).
    #[serde(default)]
    pub executable: Option<PathBuf>,
    /// Lutris entry for the launcher itself (usually the installer first,
    /// re-pointed after installation). Same adapter as game entries.
    #[serde(default)]
    pub lutris: Option<LutrisEntry>,
    /// Launcher-specific tuning overrides. Omitted fields inherit the game
    /// tunings; explicit values win (including explicit `false`). Env merges
    /// with the game env, launcher keys winning, `null` removing a key.
    /// Lets the launcher use DXVK while the game keeps its bundled d3d9.
    #[serde(default)]
    pub tunings: TuningsPatch,
}

/// One Lutris game entry, game or launcher: everything the yml renderer
/// and the structural check compare. Built once per caller, so render,
/// check, plan, and execute never disagree about desired state.
#[derive(Debug, Clone)]
pub struct EntrySpec {
    pub slug: String,
    pub name: String,
    pub game_slug: String,
    pub exe: String,
    pub dir: String,
    pub prefix: Option<String>,
    pub runner_version: String,
    pub wine_arch: String,
    pub dxvk: bool,
    pub vkd3d: bool,
    pub esync: bool,
    pub fsync: bool,
    pub dll_overrides: Vec<String>,
    pub extra_env: BTreeMap<String, String>,
    pub command_prefix: Option<String>,
}

/// The instance name embeds into the game entry's `prefix_command`, which
/// Lutris shlex-splits before exec: restrict it to a token-safe charset so
/// a hostile or sloppy name cannot break (or inject into) the split.
fn validate_instance_token(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("unsafe instance name for prefix_command: {name}");
    }
    Ok(())
}

/// The game entry's launch gate, composed into `system.prefix_command`:
/// the bare `modde-manager` token resolves through PATH exactly like the
/// entry's system wine (the deployed wrapper passes `--config` explicitly,
/// so none is threaded through status/plan/apply — the raw-binary
/// fallback re-exec exists only for a bare binary and runs at most once),
/// `--` ends the gate's own flags so Lutris's appended wine invocation
/// lands as the passthrough command. A declared per-site wrapper stays in
/// front of the gate; the gate itself is synthesized per instance and
/// never declared.
fn game_gate_prefix(name: &str, declared: Option<&str>) -> Result<String> {
    validate_instance_token(name)?;
    let gate = format!("modde-manager onboard gate --instance {name} --");
    Ok(match declared {
        Some(prefix) => format!("{prefix} {gate}"),
        None => gate,
    })
}

fn game_spec(
    name: &str,
    instance: &Instance,
    entry: &LutrisEntry,
    runner: &Runner,
    wiring: &Wiring,
) -> Result<EntrySpec> {
    Ok(EntrySpec {
        slug: entry.slug.clone(),
        name: entry.name.clone(),
        game_slug: entry.game_slug.clone(),
        exe: instance
            .root
            .join(&wiring.launch.executable)
            .display()
            .to_string(),
        dir: instance.root.display().to_string(),
        prefix: wiring
            .prefix
            .as_ref()
            .map(|prefix| prefix.path.display().to_string()),
        runner_version: runner.version.clone(),
        wine_arch: mapped_arch(&wiring.runtime.arch).to_owned(),
        dxvk: wiring.tunings.dxvk,
        vkd3d: wiring.tunings.vkd3d,
        esync: wiring.tunings.esync,
        fsync: wiring.tunings.fsync,
        dll_overrides: wiring.dll_overrides.clone(),
        extra_env: wiring.tunings.env.clone(),
        command_prefix: Some(game_gate_prefix(name, entry.command_prefix.as_deref())?),
    })
}

/// Launcher-effective tunings: game tunings with the launcher's partial
/// overrides applied. Omitted fields inherit; explicit `false` wins over
/// `true`; env merges with launcher keys winning and `null` removing.
fn launcher_effective_tunings(wiring: &Wiring) -> Tunings {
    let mut out = wiring.tunings.clone();
    if let Some(launcher) = &wiring.launcher {
        let patch = &launcher.tunings;
        if let Some(dxvk) = patch.dxvk {
            out.dxvk = dxvk;
        }
        if let Some(vkd3d) = patch.vkd3d {
            out.vkd3d = vkd3d;
        }
        if let Some(esync) = patch.esync {
            out.esync = esync;
        }
        if let Some(fsync) = patch.fsync {
            out.fsync = fsync;
        }
        if let Some(env) = &patch.env {
            for (key, value) in env {
                match value {
                    Some(value) => {
                        out.env.insert(key.clone(), value.clone());
                    }
                    None => {
                        out.env.remove(key);
                    }
                }
            }
        }
    }
    out
}

fn launcher_spec(
    installer: &InstallerExpectation,
    prefix: &Path,
    entry: &LutrisEntry,
    runner: &Runner,
    wiring: &Wiring,
    executable: Option<&PathBuf>,
) -> Result<EntrySpec> {
    // Installed launcher wins when declared; the installer is the
    // first-run path before the launcher exists.
    let exe = executable.map(PathBuf::as_path).unwrap_or(&installer.path);
    let dir = exe
        .parent()
        .context("launcher executable needs a parent directory")?
        .display()
        .to_string();
    let tunings = launcher_effective_tunings(wiring);
    Ok(EntrySpec {
        slug: entry.slug.clone(),
        name: entry.name.clone(),
        game_slug: entry.game_slug.clone(),
        exe: exe.display().to_string(),
        dir,
        prefix: Some(prefix.display().to_string()),
        runner_version: runner.version.clone(),
        wine_arch: mapped_arch(&wiring.runtime.arch).to_owned(),
        dxvk: tunings.dxvk,
        vkd3d: tunings.vkd3d,
        esync: tunings.esync,
        fsync: tunings.fsync,
        // Non-game process: no game-client DLL overrides.
        dll_overrides: Vec::new(),
        extra_env: tunings.env,
        command_prefix: entry.command_prefix.clone(),
    })
}

/// Lutris wine arch is win32|win64; WOW64-capable setups use a win64 prefix.
fn mapped_arch(arch: &str) -> &str {
    if arch == "wow64" { "win64" } else { arch }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallerExpectation {
    pub path: PathBuf,
    #[serde(default)]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientVersion {
    pub file: PathBuf,
    pub contains: String,
}

/// Presence-tracking overlay: every field is optional, so explicit empty
/// lists, `false`, and null env values stay meaningful instead of being
/// mistaken for "unspecified". Whole blocks are replaced, never merged
/// (except `tunings.env`, where a null value deletes a preset key).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WiringPatch {
    #[serde(default)]
    pub runtime: RuntimePatch,
    #[serde(default)]
    pub tunings: TuningsPatch,
    #[serde(default)]
    pub dll_overrides: Option<Vec<String>>,
    #[serde(default)]
    pub launch: LaunchPatch,
    #[serde(default)]
    pub prefix: Option<Prefix>,
    #[serde(default)]
    pub lutris: Option<LutrisEntry>,
    #[serde(default)]
    pub client_integrity: Option<ClientIntegrity>,
    #[serde(default)]
    pub data_patches: Option<DataPatches>,
    #[serde(default)]
    pub endpoints: Option<Endpoints>,
    #[serde(default)]
    pub launcher: Option<LauncherState>,
    #[serde(default)]
    pub client_version: Option<ClientVersion>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePatch {
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default)]
    pub anticheat: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TuningsPatch {
    #[serde(default)]
    pub dxvk: Option<bool>,
    #[serde(default)]
    pub vkd3d: Option<bool>,
    #[serde(default)]
    pub esync: Option<bool>,
    #[serde(default)]
    pub fsync: Option<bool>,
    #[serde(default)]
    pub env: Option<BTreeMap<String, Option<String>>>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchPatch {
    #[serde(default)]
    pub executable: Option<String>,
}

/// Reusable OctoWoW HD launch defaults. Site identity (accounts, paths,
/// addon pins, user settings, endpoints, HD patch approval) stays in the
/// consumer declaration; only reusable launch behavior lives here.
///
/// The HD patch set is deliberately undeclared: each installation's
/// operator-confirmed letters must be stated explicitly in the consumer
/// `wiring.data_patches` block. An HD launch with no declared set fails
/// with an actionable error instead of inferring approval from whatever
/// files happen to be installed.
pub fn octowow_hd_defaults() -> Wiring {
    Wiring {
        runtime: Runtime {
            kind: "wine".into(),
            version: "latest".into(),
            arch: "wow64".into(),
            anticheat: false,
        },
        tunings: Tunings {
            dxvk: false, // client bundles d3d9.dll; Lutris must not add a second layer
            vkd3d: false,
            esync: true,
            fsync: true,
            env: BTreeMap::from([("WINEDEBUG".into(), "-all".into())]),
        },
        dll_overrides: vec!["d3d9=n,b".into()],
        launch: Launch {
            executable: "VanillaFixes.exe".into(),
        },
        prefix: None, // site path; consumer must declare
        lutris: Some(LutrisEntry {
            slug: "octowow-community".into(),
            name: "OctoWoW".into(),
            game_slug: default_game_slug(),
            command_prefix: None,
        }),
        client_integrity: Some(ClientIntegrity {
            require_files: vec![
                "WoW.exe".into(),
                "VanillaFixes.exe".into(),
                "VanillaHelpers.dll".into(),
                "d3d9.dll".into(),
            ],
            wow_exe: None, // digest pinned by the consumer after launcher verify
        }),
        // No implicit HD approval: the consumer declares its measured set.
        data_patches: None,
        endpoints: None,
        launcher: None,
        client_version: None,
    }
}

/// Expand `preset` once for every caller (status, plan, apply, tests).
/// Replace rules: user lists replace preset lists wholesale; `tunings.env`
/// merges with user keys winning; every other user field overrides its
/// preset scalar. No preset + no wiring is "not configured", never implicit.
/// Materialize a full declaration without a preset: every field falls back
/// to the documented serde defaults, so a partial block stays meaningful.
fn unpreset_defaults() -> Wiring {
    Wiring {
        runtime: Runtime {
            kind: default_runtime_kind(),
            version: default_runtime_version(),
            arch: default_runtime_arch(),
            anticheat: default_anticheat(),
        },
        tunings: Tunings {
            dxvk: true,
            vkd3d: false,
            esync: true,
            fsync: true,
            env: BTreeMap::new(),
        },
        dll_overrides: Vec::new(),
        launch: Launch {
            executable: default_launch_exe(),
        },
        prefix: None,
        lutris: None,
        client_integrity: None,
        data_patches: None,
        endpoints: None,
        launcher: None,
        client_version: None,
    }
}

fn apply_patch(mut base: Wiring, patch: &WiringPatch) -> Wiring {
    if let Some(kind) = &patch.runtime.kind {
        base.runtime.kind = kind.clone();
    }
    if let Some(version) = &patch.runtime.version {
        base.runtime.version = version.clone();
    }
    if let Some(arch) = &patch.runtime.arch {
        base.runtime.arch = arch.clone();
    }
    if let Some(anticheat) = patch.runtime.anticheat {
        base.runtime.anticheat = anticheat;
    }
    // Each tuning overrides independently; siblings are never touched.
    if let Some(dxvk) = patch.tunings.dxvk {
        base.tunings.dxvk = dxvk;
    }
    if let Some(vkd3d) = patch.tunings.vkd3d {
        base.tunings.vkd3d = vkd3d;
    }
    if let Some(esync) = patch.tunings.esync {
        base.tunings.esync = esync;
    }
    if let Some(fsync) = patch.tunings.fsync {
        base.tunings.fsync = fsync;
    }
    if let Some(env) = &patch.tunings.env {
        for (key, value) in env {
            match value {
                Some(value) => {
                    base.tunings.env.insert(key.clone(), value.clone());
                }
                None => {
                    base.tunings.env.remove(key);
                }
            }
        }
    }
    if let Some(overrides) = &patch.dll_overrides {
        base.dll_overrides = overrides.clone();
    }
    if let Some(executable) = &patch.launch.executable {
        base.launch.executable = executable.clone();
    }
    if patch.prefix.is_some() {
        base.prefix = patch.prefix.clone();
    }
    if patch.lutris.is_some() {
        base.lutris = patch.lutris.clone();
    }
    if patch.client_integrity.is_some() {
        base.client_integrity = patch.client_integrity.clone();
    }
    if patch.data_patches.is_some() {
        base.data_patches = patch.data_patches.clone();
    }
    if patch.endpoints.is_some() {
        base.endpoints = patch.endpoints.clone();
    }
    if patch.launcher.is_some() {
        base.launcher = patch.launcher.clone();
    }
    if patch.client_version.is_some() {
        base.client_version = patch.client_version.clone();
    }
    base
}

pub fn expand_preset(preset: Option<&str>, user: &Option<WiringPatch>) -> Result<Wiring> {
    match (preset, user) {
        (None, None) => {
            bail!("wiring not configured for this instance (need preset or wiring block)")
        }
        (None, Some(patch)) => Ok(apply_patch(unpreset_defaults(), patch)),
        (Some("octowow-hd"), patch) => {
            let base = octowow_hd_defaults();
            Ok(match patch {
                Some(patch) => apply_patch(base, patch),
                None => base,
            })
        }
        (Some(other), _) => bail!("unknown wiring preset '{other}'; supported: octowow-hd"),
    }
}

/// Single resolution path for status, plan, and apply.
pub fn resolve_wiring(instance: &Instance) -> Result<Wiring> {
    expand_preset(instance.preset.as_deref(), &instance.wiring)
}

/// Offline declaration validation: pure deserializer + resolver checks with
/// no filesystem, Wine, graphical session, or mutable user-state access.
/// Nix checks feed generated configs through this (via
/// `onboard validate`); live file presence stays in `status`, never here.
/// Callers (status, prepare, launch, gate, and the CLI dispatch) run this
/// before any filesystem observation so malformed declarations fail with
/// actionable errors, never with inspection side effects.
pub fn validate_declaration(name: &str, instance: &Instance) -> Result<()> {
    validate_instance_token(name)?;
    if !instance.root.is_absolute() {
        bail!("client root must be absolute: {}", instance.root.display());
    }
    if has_parent_traversal(&instance.root) {
        bail!(
            "client root must not contain '..': {}",
            instance.root.display()
        );
    }
    let wiring = resolve_wiring(instance)?;
    // Launch executable: bare file name inside the client root, never a
    // path escape or absolute path.
    if wiring.launch.executable.is_empty()
        || wiring.launch.executable.contains('/')
        || wiring.launch.executable.contains('\\')
        || wiring.launch.executable.contains("..")
    {
        bail!(
            "launch executable must be a bare file name: '{}'",
            wiring.launch.executable
        );
    }
    // Prefix: absolute sibling of the root, never inside it, never
    // traversal-ambiguous (lexical `starts_with`/`overlap` checks are only
    // sound once `..` is rejected).
    if let Some(prefix) = &wiring.prefix {
        if !prefix.path.is_absolute() {
            bail!("prefix path must be absolute");
        }
        if has_parent_traversal(&prefix.path) {
            bail!(
                "prefix path must not contain '..': {}",
                prefix.path.display()
            );
        }
        if overlap(&prefix.path, &instance.root) {
            bail!("prefix must be a sibling, never inside the game folder");
        }
    }
    // Lutris entries: token-safe slugs, non-empty names, distinct game vs
    // launcher slugs (they share the yml namespace and pga.db slugs).
    if let Some(entry) = &wiring.lutris {
        validate_slug(&entry.slug)?;
        if entry.name.is_empty() {
            bail!("lutris entry name must not be empty");
        }
    }
    // Client integrity shapes: bare file names, plausible digest shapes.
    if let Some(integrity) = &wiring.client_integrity {
        for file in &integrity.require_files {
            if file.is_empty()
                || file.contains('/')
                || file.contains('\\')
                || file.contains("..")
            {
                bail!("require_files must be bare file names: '{file}'");
            }
        }
        if let Some(expected) = &integrity.wow_exe {
            if expected.size == 0 {
                bail!("wow_exe size must be non-zero");
            }
            if expected.sha256.len() != 64
                || !expected.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            {
                bail!("wow_exe sha256 must be 64 hex characters");
            }
        }
    }
    // HD patch policy: single alphanumerics, no case-insensitive
    // duplicates (they would be ambiguous on disk), plausible patch-A.
    // An explicitly empty approval (no letters, no patch-A) is rejected:
    // declare the operator-confirmed set or omit the block entirely.
    if let Some(patches) = &wiring.data_patches {
        let mut seen = std::collections::BTreeSet::new();
        for letter in &patches.native_letters {
            if letter.len() != 1 || !letter.bytes().all(|c| c.is_ascii_alphanumeric()) {
                bail!("unsafe patch letter: {letter}");
            }
            if !seen.insert(letter.to_ascii_uppercase()) {
                bail!("duplicate patch letter (case-insensitive): {letter}");
            }
        }
        if patches.native_letters.is_empty() && patches.patch_a.is_none() {
            bail!("data_patches declares no letters and no patch-A; declare the operator-confirmed set or omit the block");
        }
        if let Some(expected) = &patches.patch_a {
            if expected.size == 0 {
                bail!("patch_a size must be non-zero");
            }
            if expected.sha256.len() != 64
                || !expected.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            {
                bail!("patch_a sha256 must be 64 hex characters");
            }
        }
    }
    // Structural tunings: these keys are owned by the prefix/DLL
    // plumbing and silently ignored in `tunings.env` — reject them so a
    // declaration cannot look effective while doing nothing.
    for key in ["WINEPREFIX", "WINEARCH", "WINEDLLOVERRIDES"] {
        if wiring.tunings.env.contains_key(key) {
            bail!("tunings.env must not set {key} (owned by prefix/DLL plumbing)");
        }
        if let Some(launcher) = &wiring.launcher
            && let Some(env) = &launcher.tunings.env
            && env.contains_key(key)
        {
            bail!("launcher.tunings.env must not set {key} (owned by prefix/DLL plumbing)");
        }
    }
    // Launcher shapes: absolute traversal-free paths, executable under its
    // prefix (lexical containment is only sound once `..` is rejected).
    if let Some(launcher) = &wiring.launcher {
        if let Some(installer) = &launcher.installer {
            if !installer.path.is_absolute() {
                bail!("launcher installer path must be absolute");
            }
            if has_parent_traversal(&installer.path) {
                bail!(
                    "launcher installer path must not contain '..': {}",
                    installer.path.display()
                );
            }
            if let Some(digest) = &installer.sha256
                && (digest.len() != 64
                    || !digest.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                bail!("installer sha256 must be 64 hex characters");
            }
        }
        if let Some(prefix) = &launcher.prefix {
            if !prefix.is_absolute() {
                bail!("launcher prefix path must be absolute");
            }
            if has_parent_traversal(prefix) {
                bail!(
                    "launcher prefix path must not contain '..': {}",
                    prefix.display()
                );
            }
            if overlap(prefix, &instance.root) {
                bail!("launcher prefix must be a sibling, never inside the game folder");
            }
            if let Some(executable) = &launcher.executable {
                if !executable.is_absolute() {
                    bail!("launcher executable path must be absolute");
                }
                if has_parent_traversal(executable) {
                    bail!(
                        "launcher executable path must not contain '..': {}",
                        executable.display()
                    );
                }
                if !executable.starts_with(prefix) {
                    bail!("launcher executable must live under the launcher prefix");
                }
            }
        } else if launcher.executable.is_some() {
            bail!("launcher executable needs a declared launcher prefix");
        }
        if let Some(entry) = &launcher.lutris {
            validate_slug(&entry.slug)?;
            if entry.name.is_empty() {
                bail!("launcher lutris entry name must not be empty");
            }
        }
    }
    if let (Some(game), Some(launcher)) = (&wiring.lutris, &wiring.launcher)
        && let Some(lentry) = &launcher.lutris
        && game.slug == lentry.slug
    {
        bail!(
            "game and launcher lutris slugs must differ (both use '{}')",
            game.slug
        );
    }
    Ok(())
}

/// Lexical parent-traversal detector for offline validation: any `..`
/// component makes prefix-containment checks (`starts_with`/`overlap`)
/// unsound (e.g. `/prefix/../outside` lexically starts with `/prefix`).
/// Live resolution never follows such paths; validation rejects them first.
fn has_parent_traversal(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemState {
    Verified,
    Missing,
    Mismatched,
    Unverifiable,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusItem {
    pub name: String,
    pub state: ItemState,
    pub detail: String,
    pub fix: String,
}

fn item(name: &str, state: ItemState, detail: String, fix: String) -> StatusItem {
    StatusItem {
        name: name.into(),
        state,
        detail,
        fix,
    }
}

pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .context("HOME must be an absolute path")
}

/// Lutris locations resolved per the XDG Base Directory spec: explicit
/// `XDG_DATA_HOME`/`XDG_CONFIG_HOME` win over `$HOME`-joined fallbacks.
/// Lutris itself resolves this way, so joining $HOME blindly splits brain
/// from the real client whenever those variables are set (as on NixOS).
#[derive(Debug, Clone)]
pub struct HomeDirs {
    pub home: PathBuf,
    pub data: PathBuf,
    pub config: PathBuf,
}

impl HomeDirs {
    pub fn from_home(home: PathBuf) -> Self {
        Self::resolve(
            home,
            std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
            std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        )
    }

    fn resolve(home: PathBuf, data_var: Option<PathBuf>, config_var: Option<PathBuf>) -> Self {
        let pick = |var: Option<PathBuf>, fallback: &str| {
            var.filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
        };
        Self {
            data: pick(data_var, ".local/share"),
            config: pick(config_var, ".config"),
            home,
        }
    }

    /// Test-only: ignore the ambient environment for hermetic fixtures.
    #[cfg(test)]
    pub fn isolated(home: PathBuf) -> Self {
        Self {
            data: home.join(".local/share"),
            config: home.join(".config"),
            home,
        }
    }
}

/// Lutris installations as matched (config dir, data dir) pairs: native
/// first, then the Flatpak sandbox. Config and database are always selected
/// together — never a native yml with a Flatpak database or vice versa.
fn lutris_sites(dirs: &HomeDirs) -> [(PathBuf, PathBuf); 2] {
    [
        (dirs.config.join("lutris/games"), dirs.data.join("lutris")),
        (
            dirs.home
                .join(".var/app/net.lutris.Lutris/config/lutris/games"),
            dirs.home.join(".var/app/net.lutris.Lutris/data/lutris"),
        ),
    ]
}

fn runner_search_dirs(dirs: &HomeDirs) -> Vec<PathBuf> {
    lutris_sites(dirs)
        .into_iter()
        .map(|(_, data)| data.join("runners/wine"))
        .collect()
}

/// First site whose database exists (native preferred). The paired config
/// dir travels with it.
fn lutris_site(dirs: &HomeDirs) -> Option<(PathBuf, PathBuf)> {
    lutris_sites(dirs)
        .into_iter()
        .find(|(_, data)| data.join("pga.db").is_file())
}

/// The slug's yml in the database's own installation. A yml living in the
/// other installation does not count (wrong home). Both installations are
/// searched only while database-less, for orphan detection.
fn lutris_yml_path(dirs: &HomeDirs, slug: &str) -> Option<PathBuf> {
    if let Some((config, _)) = lutris_site(dirs) {
        let candidate = config.join(format!("{slug}.yml"));
        return candidate.is_file().then_some(candidate);
    }
    lutris_sites(dirs)
        .into_iter()
        .map(|(config, _)| config.join(format!("{slug}.yml")))
        .find(|p| p.is_file())
}

fn lutris_db_path(dirs: &HomeDirs) -> Option<PathBuf> {
    lutris_site(dirs).map(|(_, data)| data.join("pga.db"))
}

#[derive(Debug, Clone)]
pub struct Runner {
    pub path: PathBuf,
    pub version: String,
}

fn is_executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

/// Numeric version key from a runner directory name (`wine-ge-9-2` holds
/// [9,2] after its non-numeric prefix segments).
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap_or(0))
        .collect()
}

/// First meaningful version component (`wine-ge-9-2` -> 9).
fn major_version(name: &str) -> Option<u64> {
    version_key(name).into_iter().find(|n| *n != 0)
}

/// Scan Lutris runner dirs for `wine-*/bin/wine` executables, newest first
/// by numeric version. The wine-ge-8 floor is the minimum that runs this
/// client; anything older is ignored, not warned about.
fn scan_runners(dirs: &HomeDirs) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for base in runner_search_dirs(dirs) {
        let entries = match fs::read_dir(&base) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("wine-") {
                continue;
            }
            if major_version(&name).is_none_or(|major| major < 8) {
                continue;
            }
            let wine = entry.path().join("bin/wine");
            if is_executable(&wine) {
                found.push((name, wine));
            }
        }
    }
    found.sort_by(|a, b| version_key(&a.0).cmp(&version_key(&b.0)));
    found.reverse();
    found
}

fn system_wine() -> Result<Runner> {
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let candidate = dir.join("wine");
        if is_executable(&candidate) {
            return Ok(Runner {
                path: candidate,
                version: "system".into(),
            });
        }
    }
    bail!("no system wine on PATH");
}

/// Resolve the declared wine runner. `latest` picks the numerically newest
/// installed runner; an exact version pins a directory name; `system` uses
/// `PATH` (looked up in Rust, no shell).
pub fn discover_runner(dirs: &HomeDirs, kind: &str, version: &str) -> Result<Runner> {
    if kind != "wine" {
        bail!("unsupported runtime kind '{kind}'; supported: wine");
    }
    if version == "system" {
        return system_wine();
    }
    let found = scan_runners(dirs);
    if version != "latest" {
        return found
            .into_iter()
            .find(|(name, _)| name == version)
            .map(|(name, path)| Runner {
                path,
                version: name,
            })
            .with_context(|| format!("pinned wine runner '{version}' not installed"));
    }
    found
        .into_iter()
        .next()
        .map(|(name, path)| Runner {
            path,
            version: name,
        })
        .context("no wine runner installed (need wine-ge-8+ with WOW64 support)")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RecordedRunner {
    version: String,
    path: PathBuf,
}

fn runtime_record_path(instance: &Instance) -> Option<PathBuf> {
    instance
        .state_dir
        .as_ref()
        .map(|dir| dir.join("wiring-runtime.json"))
}

/// Fail-closed record read: a corrupt record blocks instead of silently
/// falling back to discovery and switching runtimes.
fn read_recorded(instance: &Instance) -> Result<Option<RecordedRunner>> {
    let Some(path) = runtime_record_path(instance) else {
        return Ok(None);
    };
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(format!("read {}", path.display())),
    };
    serde_json::from_slice(&bytes)
        .with_context(|| format!("runtime record corrupt: {}", path.display()))
        .map(Some)
}

/// Select the runtime status, plan, and apply share. A valid record wins;
/// a declaration pin contradicting it fails closed. A recorded binary that
/// no longer executes blocks unless `reselect` explicitly re-resolves.
pub fn select_runner(
    dirs: &HomeDirs,
    instance: &Instance,
    wiring: &Wiring,
    reselect: bool,
) -> Result<(Runner, bool)> {
    // No bypasses: "system" resolves through PATH and is recorded like any
    // other selection, so the executed artifact is always the reviewed one.
    if !reselect {
        if let Some(recorded) = read_recorded(instance)? {
            if wiring.runtime.version != "latest" && wiring.runtime.version != recorded.version {
                bail!(
                    "declaration pins '{}' but '{}' is recorded; update the record explicitly",
                    wiring.runtime.version,
                    recorded.version
                );
            }
            if is_executable(&recorded.path) {
                return Ok((
                    Runner {
                        path: recorded.path,
                        version: recorded.version,
                    },
                    true,
                ));
            }
            bail!(
                "recorded runner '{}' no longer executes; pass --reselect to choose again",
                recorded.path.display()
            );
        }
    }
    Ok((
        discover_runner(dirs, &wiring.runtime.kind, &wiring.runtime.version)?,
        false,
    ))
}

/// Persist the executed selection; returns true when the record changed.
fn record_runner(instance: &Instance, runner: &Runner) -> Result<bool> {
    let Some(path) = runtime_record_path(instance) else {
        return Ok(false);
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_vec_pretty(&RecordedRunner {
        version: runner.version.clone(),
        path: runner.path.clone(),
    })?;
    if path.is_file() && fs::read(&path)? == body {
        return Ok(false);
    }
    atomic_write_0600(&path, &body)?;
    Ok(true)
}

/// Parse a 32-bit Windows executable header: returns (is_i386, large_address_aware).
pub fn pe_exe_flags(bytes: &[u8]) -> Result<(bool, bool)> {
    if bytes.len() < 0x40 || &bytes[0..2] != b"MZ" {
        bail!("not a Windows executable (MZ header)");
    }
    let pe = u32::from_le_bytes(bytes[0x3c..0x40].try_into().unwrap()) as usize;
    pe_coff_flags(bytes, pe)
}

fn pe_coff_flags(bytes: &[u8], pe: usize) -> Result<(bool, bool)> {
    if bytes.len() < pe.checked_add(24).context("PE offset overflows")? {
        bail!("truncated PE header");
    }
    if &bytes[pe..pe + 4] != b"PE\0\0" {
        bail!("not a Windows executable (PE signature)");
    }
    let machine = u16::from_le_bytes(bytes[pe + 4..pe + 6].try_into().unwrap());
    let characteristics = u16::from_le_bytes(bytes[pe + 22..pe + 24].try_into().unwrap());
    Ok((machine == 0x14c, characteristics & 0x20 != 0))
}

/// Bounded header read from an open executable: DOS header first, then the
/// COFF header at `e_lfanew` (validated before seeking, so arbitrary layouts
/// work and truncated or malicious offsets fail without reading payloads).
pub fn pe_exe_flags_file(file: &mut fs::File) -> Result<(bool, bool)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut dos = [0u8; 0x40];
    file.read_exact(&mut dos)
        .context("executable smaller than DOS header")?;
    if &dos[0..2] != b"MZ" {
        bail!("not a Windows executable (MZ header)");
    }
    let pe = u32::from_le_bytes(dos[0x3c..0x40].try_into().unwrap()) as u64;
    // COFF header must start past DOS and stay within a sane header bound.
    if pe < 0x40 || pe > (1 << 20) {
        bail!("implausible PE offset {pe:#x}");
    }
    file.seek(SeekFrom::Start(pe))?;
    let mut coff = [0u8; 24];
    file.read_exact(&mut coff).context("truncated PE header")?;
    pe_coff_flags(&coff, 0).context("invalid PE header")
}

/// Read a Wine prefix architecture from `system.reg` (`#arch=win64|win32`).
/// A WOW64-capable setup is a `win64` prefix running 32-bit binaries.
pub fn prefix_arch(prefix: &Path) -> Result<Option<String>> {
    let reg = prefix.join("system.reg");
    let body = match fs::read_to_string(&reg) {
        Ok(body) => body,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(format!("read {}", reg.display())),
    };
    for line in body.lines().take(8) {
        if let Some(arch) = line.strip_prefix("#arch=") {
            return Ok(Some(arch.trim().into()));
        }
    }
    bail!("prefix system.reg has no #arch marker")
}

/// Open a client file without following symlinks and without reading
/// payloads: the returned descriptor supports metadata, bounded header
/// reads, and streaming digests. Missing files report None; symlinks,
/// non-files, and inspection errors fail.
fn pinned_file(root: &Path, rel: &str) -> Result<Option<(fs::File, fs::Metadata)>> {
    use std::os::unix::fs::OpenOptionsExt;
    let anchor = Anchor::open(root)?;
    let parent = anchor.parent(Path::new(rel), false, &mut Vec::new())?;
    let Some(parent) = parent else {
        return Ok(None);
    };
    let candidate = fd_path(&parent).join(Path::new(rel).file_name().context("missing name")?);
    let file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&candidate)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            bail!("client path is a symlink: {rel}")
        }
        Err(e) => return Err(e).context(format!("inspect client file: {rel}")),
    };
    let meta = file.metadata()?;
    if !meta.is_file() {
        bail!("client path is not a file: {rel}");
    }
    Ok(Some((file, meta)))
}

/// Presence metadata without payloads (HD payloads are gigabytes).
fn root_file_meta(root: &Path, rel: &str) -> Result<Option<fs::Metadata>> {
    Ok(pinned_file(root, rel)?.map(|(_, meta)| meta))
}

/// First of several candidate names that exists (e.g. lowercase then
/// uppercase MPQ). Casing fallback applies to absence only — permission or
/// symlink errors fail instead of silently trying the next spelling.
fn existing_name(root: &Path, candidates: &[&str]) -> Result<Option<String>> {
    for candidate in candidates {
        match root_file_meta(root, candidate)? {
            Some(_) => return Ok(Some(candidate.to_string())),
            None => continue,
        }
    }
    Ok(None)
}

/// Case-insensitive `Data/patch-<letter>.mpq` lookup over one directory
/// inventory. The full filename compares ASCII case-insensitively, so
/// `Patch-E.mpq`, `patch-E.MPQ`, and `PATCH-e.MpQ` all satisfy letter `E`
/// without renaming anything on disk. Zero matches mean absent; more than
/// one case-variant is ambiguous and fails closed (never pick one).
/// Checking and applying never rename patches.
fn find_mpq_actual(names: &[String], letter: &str) -> Result<Option<String>> {
    let want = format!("patch-{letter}.mpq");
    let mut hits = Vec::new();
    for name in names {
        if name.eq_ignore_ascii_case(&want) {
            hits.push(name.clone());
        }
    }
    if hits.len() > 1 {
        hits.sort();
        bail!(
            "ambiguous patch files for letter '{letter}': {}; keep exactly one spelling",
            hits.join(", ")
        );
    }
    Ok(hits.into_iter().next())
}

/// Resolve one HD letter against an already-listed `Data/` inventory:
/// case-insensitive name match, then a pinned metadata check so symlinks,
/// directories, and permission errors fail closed instead of counting as
/// present. Returns the actual on-disk filename, if any.
fn resolve_hd_letter(root: &Path, names: &[String], letter: &str) -> Result<Option<String>> {
    let Some(actual) = find_mpq_actual(names, letter)? else {
        return Ok(None);
    };
    let rel = format!("Data/{actual}");
    match root_file_meta(root, &rel)? {
        Some(_) => Ok(Some(actual)),
        None => Ok(None),
    }
}

/// Streaming digest from the descriptor's current offset: size plus
/// SHA-256 without holding the payload.
fn stream_digest_file(file: &mut fs::File) -> Result<(u64, String)> {
    use std::io::Read;
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut buf = [0u8; 1 << 16];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hash.update(&buf[..read]);
    }
    Ok((size, hex(&hash.finalize())))
}

/// Small text read with a cap; version markers and realmlists are bytes,
/// never multi-gigabyte payloads.
fn read_capped(root: &Path, rel: &str, cap: u64) -> Result<Vec<u8>> {
    let anchor = Anchor::open(root)?;
    match anchor.read(Path::new(rel), false)? {
        Image::File(bytes) if (bytes.len() as u64) <= cap => Ok(bytes),
        Image::File(_) => bail!("client file unexpectedly large: {rel}"),
        Image::Missing => bail!("missing client file: {rel}"),
        _ => bail!("client path is not a file: {rel}"),
    }
}

fn validate_slug(slug: &str) -> Result<()> {
    if slug.is_empty()
        || !slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("unsafe lutris slug: {slug}");
    }
    Ok(())
}

/// Structural check of one Lutris game yml against the declaration: exact
/// exe, working dir, prefix, wine version/arch/toggles, DLL overrides,
/// the synthesized launch-gate `prefix_command`, and explicitly disabled
/// anti-cheat runtimes.
fn check_lutris_yml(
    name: &str,
    path: &Path,
    instance: &Instance,
    wiring: &Wiring,
    entry: &LutrisEntry,
) -> Result<StatusItem> {
    // No record yet: discovery decides, so the version check waits for apply.
    // A corrupt record fails the whole check (fail-closed, like selection).
    let recorded = read_recorded(instance).map(|record| record.map(|record| record.version));
    let spec = EntrySpec {
        slug: entry.slug.clone(),
        name: entry.name.clone(),
        game_slug: entry.game_slug.clone(),
        exe: instance
            .root
            .join(&wiring.launch.executable)
            .display()
            .to_string(),
        dir: instance.root.display().to_string(),
        prefix: wiring
            .prefix
            .as_ref()
            .map(|prefix| prefix.path.display().to_string()),
        runner_version: String::new(),
        wine_arch: mapped_arch(&wiring.runtime.arch).to_owned(),
        dxvk: wiring.tunings.dxvk,
        vkd3d: wiring.tunings.vkd3d,
        esync: wiring.tunings.esync,
        fsync: wiring.tunings.fsync,
        dll_overrides: wiring.dll_overrides.clone(),
        extra_env: wiring.tunings.env.clone(),
        command_prefix: Some(game_gate_prefix(name, entry.command_prefix.as_deref())?),
    };
    Ok(check_entry_yml(
        path,
        "lutris-yml",
        "run: onboard apply (rewrites this entry only)",
        &spec,
        recorded,
    ))
}

/// Structural check of one Lutris game yml against an entry spec: exact
/// exe, working dir, prefix, wine version/arch/toggles, DLL overrides, and
/// explicitly disabled anti-cheat runtimes.
fn check_entry_yml(
    path: &Path,
    item_name: &str,
    fix: &str,
    spec: &EntrySpec,
    recorded: Result<Option<String>>,
) -> StatusItem {
    let mismatch = |detail: String| item(item_name, ItemState::Mismatched, detail, fix.to_owned());
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) => {
            return item(
                item_name,
                ItemState::Unverifiable,
                format!("{e}"),
                "inspect the yml permissions".into(),
            );
        }
    };
    let parsed: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&body) {
        Ok(parsed) => parsed,
        Err(e) => return mismatch(format!("yml does not parse: {e}")),
    };
    let mut problems = Vec::new();
    let get = |keys: &[&str]| -> Option<&serde_yaml_ng::Value> {
        let mut current = Some(&parsed);
        for key in keys {
            current = match current {
                Some(serde_yaml_ng::Value::Mapping(map)) => {
                    map.get(&serde_yaml_ng::Value::String(key.to_string()))
                }
                _ => None,
            };
        }
        current
    };
    let expect_str = |keys: &[&str], want: &str, label: &str| -> Option<String> {
        match get(keys).and_then(|v| v.as_str()) {
            Some(have) if have == want => None,
            Some(have) => Some(format!("{label} is '{have}', want '{want}'")),
            None => Some(format!("{label} missing, want '{want}'")),
        }
    };
    if let Some(problem) = expect_str(&["slug"], &spec.slug, "slug") {
        problems.push(problem);
    }
    for (keys, want) in [
        (["game", "exe"], spec.exe.as_str()),
        (["game", "working_dir"], spec.dir.as_str()),
    ] {
        if let Some(problem) = expect_str(&keys, want, keys[1]) {
            problems.push(problem);
        }
    }
    if let Some(prefix) = &spec.prefix {
        if let Some(problem) = expect_str(&["game", "prefix"], prefix, "prefix") {
            problems.push(problem);
        }
    }
    if let Some(problem) = expect_str(&["runner"], "wine", "runner") {
        problems.push(problem);
    }
    // No record yet: discovery decides, so the version check waits for apply.
    // A corrupt record fails the whole check (fail-closed, like selection).
    match recorded {
        Ok(Some(version)) => {
            if let Some(problem) = expect_str(&["wine", "version"], &version, "wine version") {
                problems.push(problem);
            }
        }
        Ok(None) => {}
        Err(e) => problems.push(format!("runtime record unreadable: {e:#}")),
    }
    if let Some(problem) = expect_str(&["wine", "arch"], &spec.wine_arch, "wine arch") {
        problems.push(problem);
    }
    for (key, want) in [
        ("dxvk", spec.dxvk),
        ("vkd3d", spec.vkd3d),
        ("esync", spec.esync),
        ("fsync", spec.fsync),
        ("eac", false),
        ("battleye", false),
    ] {
        match get(&["wine", key]).and_then(|v| v.as_bool()) {
            Some(have) if have == want => {}
            Some(have) => problems.push(format!("wine.{key} is {have}, want {want}")),
            None => problems.push(format!("wine.{key} missing, want {want}")),
        }
    }
    // The deployed wine version decides which library environment Lutris
    // builds: system wine must run without the Lutris runtime (its `/lib`
    // and `/usr/lib` resolve to the FHS glibc, breaking the host wrapper
    // with a libc symbol lookup error); Lutris-managed runners keep theirs.
    let yml_version = get(&["wine", "version"]).and_then(|v| v.as_str()).unwrap_or("");
    let want_disable = yml_version == "system";
    match get(&["system", "disable_runtime"]).and_then(|v| v.as_bool()) {
        Some(have) if have == want_disable => {}
        Some(have) => problems.push(format!(
            "system.disable_runtime is {have}, want {want_disable} for wine version '{yml_version}'"
        )),
        None => problems.push(format!(
            "system.disable_runtime missing, want {want_disable} for wine version '{yml_version}'"
        )),
    }
    let overrides = get(&["system", "env", "WINEDLLOVERRIDES"]).and_then(|v| v.as_str());
    match overrides {
        Some(have) => {
            for dll in &spec.dll_overrides {
                if !have.split(';').any(|entry| entry.trim() == dll) {
                    problems.push(format!("dll override missing: {dll}"));
                }
            }
        }
        None if !spec.dll_overrides.is_empty() => {
            problems.push("WINEDLLOVERRIDES missing".into());
        }
        _ => {}
    }
    match (
        &spec.command_prefix,
        get(&["system", "prefix_command"]).and_then(|v| v.as_str()),
    ) {
        (Some(want), Some(have)) if have == want => {}
        (Some(want), Some(have)) => {
            problems.push(format!("system.prefix_command is '{have}', want '{want}'"))
        }
        (Some(want), None) => {
            problems.push(format!("system.prefix_command missing, want '{want}'"))
        }
        (None, Some(have)) => {
            problems.push(format!("system.prefix_command is '{have}', want absent"))
        }
        (None, None) => {}
    }
    if problems.is_empty() {
        item(
            item_name,
            ItemState::Verified,
            path.display().to_string(),
            String::new(),
        )
    } else {
        mismatch(problems.join("; "))
    }
}

fn sqlite3(args: &[String]) -> Result<String> {
    let output = Command::new("sqlite3").args(args).output()?;
    if !output.status.success() {
        bail!(
            "sqlite3 failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

/// Endpoints section of [`status`]: realmlist content assertions (the file
/// stays unmanaged). Read-only; returns exactly one `endpoints` item.
fn endpoints_item(root: &Path, endpoints: &Endpoints) -> StatusItem {
    let bytes = match read_capped(root, "realmlist.wtf", 1 << 20) {
        Ok(bytes) => bytes,
        Err(e) => {
            return item(
                "endpoints",
                ItemState::Missing,
                format!("{e:#}"),
                "restore realmlist.wtf from backup".into(),
            );
        }
    };
    let body = String::from_utf8_lossy(&bytes);
    let missing: Vec<&str> = endpoints
        .assert_realmlist
        .iter()
        .map(String::as_str)
        .filter(|line| !body.contains(*line))
        .collect();
    if missing.is_empty() {
        item(
            "endpoints",
            ItemState::Verified,
            "realmlist matches".into(),
            String::new(),
        )
    } else {
        item(
            "endpoints",
            ItemState::Mismatched,
            format!("missing lines: {}", missing.join(", ")),
            "update realmlist.wtf out-of-band (unmanaged file)".into(),
        )
    }
}

/// Read-only readiness report. Never executes wine, Lutris, or the launcher.
/// Declaration shapes are validated first (fail-closed on malformed
/// declarations); filesystem inspection errors for launch-readiness
/// findings (client files, wow-exe identity, HD patches) surface as
/// Mismatched/Unverifiable items — never as hard errors — so an
/// otherwise-valid registration can proceed while execution refuses.
pub fn status(name: &str, instance: &Instance, dirs: &HomeDirs) -> Result<Vec<StatusItem>> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    let mut items = Vec::new();
    if !instance.root.is_dir() {
        return Ok(vec![item(
            "client-root",
            ItemState::Missing,
            format!("client root absent: {}", instance.root.display()),
            "install the client before onboarding".into(),
        )]);
    }

    // Runner: persisted record wins over fresh discovery so the reviewed
    // selection is the executed one.
    let runner = select_runner(dirs, instance, &wiring, false);
    match &runner {
        Ok((found, recorded)) => {
            items.push(item(
                "runner",
                ItemState::Verified,
                format!("{} ({})", found.version, found.path.display()),
                String::new(),
            ));
            items.push(item(
                "runner-pinned",
                ItemState::Verified,
                if *recorded {
                    format!("recorded selection: {}", found.version)
                } else {
                    format!("unpinned discovery: {} (pins on apply)", found.version)
                },
                String::new(),
            ));
        }
        Err(e) => items.push(item(
            "runner",
            ItemState::Missing,
            format!("{e:#}"),
            "install a wine-ge-8+ runner via Lutris Runners -> Wine".into(),
        )),
    }

    // Client integrity: metadata presence for required files (payloads are
    // never loaded); size-first, streaming digest, header-only flags for
    // the executable. Inspection failures (symlinks, permissions) surface
    // as launch-readiness findings, never as hard errors: registration
    // proceeds, execution refuses. Fix hints point at the operator
    // workflow: client payloads are never fetched by the manager and never
    // by the launcher's Install/Verify — restore the operator-confirmed
    // file or re-pin the declaration after confirming provenance.
    if let Some(integrity) = &wiring.client_integrity {
        for file in &integrity.require_files {
            match root_file_meta(&instance.root, file) {
                Ok(Some(meta)) => items.push(item(
                    &format!("client-file:{file}"),
                    ItemState::Verified,
                    format!("present ({} bytes)", meta.len()),
                    String::new(),
                )),
                Ok(None) => items.push(item(
                    &format!("client-file:{file}"),
                    ItemState::Missing,
                    format!("{file} absent"),
                    "restore the operator-confirmed file or re-pin after confirming provenance, then re-check".into(),
                )),
                Err(e) => items.push(item(
                    &format!("client-file:{file}"),
                    ItemState::Mismatched,
                    format!("{e:#}"),
                    "restore the operator-confirmed file (no symlinks), then re-check".into(),
                )),
            }
        }
        if let Some(expected) = &integrity.wow_exe {
            let rel = "WoW.exe";
            match pinned_file(&instance.root, rel) {
                Err(e) => items.push(item(
                    "wow-exe",
                    ItemState::Mismatched,
                    format!("{e:#}"),
                    "restore the operator-confirmed WoW.exe (no symlinks) or re-pin after confirming provenance".into(),
                )),
                Ok(None) => items.push(item(
                    "wow-exe",
                    ItemState::Missing,
                    format!("{rel} absent"),
                    "restore the operator-confirmed WoW.exe or re-pin after confirming provenance".into(),
                )),
                Ok(Some((_, meta))) if meta.len() != expected.size => items.push(item(
                    "wow-exe",
                    ItemState::Mismatched,
                    format!("size={} want={}", meta.len(), expected.size),
                    "client drifted; restore the operator-confirmed WoW.exe or re-pin after confirming provenance".into(),
                )),
                Ok(Some((mut file, _))) => {
                    // Single descriptor: header flags first, then rewind and
                    // stream the digest. Payloads never sit in memory.
                    let flags = pe_exe_flags_file(&mut file);
                    use std::io::Seek;
                    let digest = file
                        .seek(std::io::SeekFrom::Start(0))
                        .context("rewind executable")
                        .and_then(|_| stream_digest_file(&mut file));
                    let (ok, detail) = match (flags, digest) {
                        (Ok((i386, laa)), Ok((size, digest)))
                            if digest == expected.sha256.to_ascii_lowercase()
                                && i386
                                && (!expected.laa || laa) =>
                        {
                            (true, format!("size={size} sha256={}", &digest[..16]))
                        }
                        (flags, digest) => (
                            false,
                            format!(
                                "flags={flags:?} digest={}",
                                digest
                                    .map(|(_, digest)| digest[..16].to_owned())
                                    .unwrap_or_else(|_| "<unreadable>".into())
                            ),
                        ),
                    };
                    items.push(item(
                        "wow-exe",
                        if ok {
                            ItemState::Verified
                        } else {
                            ItemState::Mismatched
                        },
                        detail,
                        if ok {
                            String::new()
                        } else {
                            "client drifted; restore the operator-confirmed WoW.exe or re-pin after confirming provenance".into()
                        },
                    ));
                }
            }
        }
    }

    // HD data patches: a single case-insensitive Data/ inventory shared by
    // presence, identity, and stray detection (payloads never loaded);
    // declared patch-A identity verified by streaming digest. Filenames
    // keep their on-disk spelling — checking never renames anything.
    // Ambiguity, symlinks, and listing failures surface as
    // Mismatched/Unverifiable findings (execution refuses, registration
    // proceeds), never as hard errors. One inventory only: presence and
    // stray detection must observe the same listing, so a failure between
    // two reads can never leave Verified presence beside a silently
    // skipped stray check.
    let hd_inventory: Result<Option<Vec<String>>> = if wiring.data_patches.is_some() {
        data_entry_names(&instance.root)
    } else {
        Ok(None)
    };
    if let Some(patches) = &wiring.data_patches {
        match &hd_inventory {
            Err(e) => {
                for letter in &patches.native_letters {
                    items.push(item(
                        &format!("hd-patch:{letter}"),
                        ItemState::Unverifiable,
                        format!("Data/ unreadable: {e:#}"),
                        "inspect Data/ permissions, then re-check".into(),
                    ));
                }
                items.push(item(
                    "hd-patch-letters",
                    ItemState::Unverifiable,
                    format!("Data/ unreadable: {e:#}"),
                    "inspect Data/ permissions, then re-check".into(),
                ));
            }
            Ok(data_names) => {
                for letter in &patches.native_letters {
                    if letter.len() != 1 || !letter.bytes().all(|c| c.is_ascii_alphanumeric()) {
                        bail!("unsafe patch letter: {letter}");
                    }
                    let want = format!("Data/patch-{letter}.mpq");
                    let found: Result<Option<String>> = match data_names {
                        None => Ok(None),
                        Some(names) => resolve_hd_letter(&instance.root, names, letter),
                    };
                    match found {
                        Ok(Some(actual)) => items.push(item(
                            &format!("hd-patch:{letter}"),
                            ItemState::Verified,
                            format!("present as Data/{actual}"),
                            String::new(),
                        )),
                        Ok(None) => items.push(item(
                            &format!("hd-patch:{letter}"),
                            ItemState::Missing,
                            format!("{want} absent (case-insensitive)"),
                            "HD patch absent; restore the operator-confirmed set, then re-check".into(),
                        )),
                        Err(e) => items.push(item(
                            &format!("hd-patch:{letter}"),
                            ItemState::Mismatched,
                            format!("{e:#}"),
                            "keep exactly one non-symlink spelling per letter, then re-check".into(),
                        )),
                    }
                }
                if let Some(expected) = &patches.patch_a {
                    let actual: Result<Option<String>> = match data_names {
                        None => Ok(None),
                        Some(names) => resolve_hd_letter(&instance.root, names, "A"),
                    };
                    match actual {
                        Err(e) => items.push(item(
                            "hd-patch-A",
                            ItemState::Mismatched,
                            format!("{e:#}"),
                            "keep exactly one non-symlink patch-A spelling, then re-check".into(),
                        )),
                        Ok(None) => items.push(item(
                            "hd-patch-A",
                            ItemState::Missing,
                            "no patch-A at all".into(),
                            "HD patch-A absent; restore the operator-confirmed set, then re-check".into(),
                        )),
                        Ok(Some(actual)) => {
                            match pinned_file(&instance.root, &format!("Data/{actual}")) {
                                Err(e) => items.push(item(
                                    "hd-patch-A",
                                    ItemState::Mismatched,
                                    format!("{e:#}"),
                                    "restore the operator-confirmed patch-A (no symlinks), then re-check".into(),
                                )),
                                Ok(None) => items.push(item(
                                    "hd-patch-A",
                                    ItemState::Missing,
                                    format!("Data/{actual} vanished during check"),
                                    "restore the operator-confirmed set, then re-check".into(),
                                )),
                                Ok(Some((mut file, meta))) => {
                                    // Size first (cheap reject), then a streaming digest.
                                    let ok = meta.len() == expected.size
                                        && stream_digest_file(&mut file)
                                            .map(|(_, digest)| {
                                                digest == expected.sha256.to_ascii_lowercase()
                                            })
                                            .unwrap_or(false);
                                    items.push(item(
                                        "hd-patch-A",
                                        if ok {
                                            ItemState::Verified
                                        } else {
                                            ItemState::Mismatched
                                        },
                                        format!("size={} want={}", meta.len(), expected.size),
                                        if ok {
                                            String::new()
                                        } else {
                                            "OctoWoW's own patch-A is back: re-copy the HD patch-A".into()
                                        },
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Rename dodge detection: single-letter patches outside the declared
    // native letters are known character-screen crash causes. Numbered
    // stock patches (patch-1..5.mpq) are always accepted. Names only —
    // payloads are never read (HD trees are gigabytes). Reuses the single
    // shared inventory above: listing failures are already reported as
    // Unverifiable, so only that same successful listing can add a stray
    // finding here — never a fresh second read.
    if let Some(patches) = &wiring.data_patches {
        if patches.forbid_renames {
            match &hd_inventory {
                Ok(Some(names)) => {
                    let strays: Vec<_> = names
                        .iter()
                        .filter_map(|name| classify_patch(name, &patches.native_letters))
                        .collect();
                    if !strays.is_empty() {
                        items.push(item(
                            "hd-patch-letters",
                            ItemState::Mismatched,
                            format!("undeclared patch files: {}", strays.join(", ")),
                            "single-letter files outside native letters are known rename dodges; restore native letters"
                                .into(),
                        ));
                    }
                }
                Ok(None) => items.push(item(
                    "hd-patch-letters",
                    ItemState::Missing,
                    "no Data/ directory".into(),
                    "install the client before onboarding".into(),
                )),
                // Listing failures already surface per-letter above; no
                // duplicate stray finding here.
                Err(_) => {}
            }
        }
    }

    // Bundled vs managed DXVK: the client ships d3d9.dll, so Lutris-managed
    // DXVK must stay off or two layers load.
    if wiring.tunings.dxvk && instance.root.join("d3d9.dll").is_file() {
        items.push(item(
            "dxvk",
            ItemState::Mismatched,
            "d3d9.dll bundled AND Lutris-managed DXVK on".into(),
            "set tunings.dxvk=false (bundled) or remove the bundled d3d9.dll".into(),
        ));
    }

    // Endpoints: realmlist content assertions (file stays unmanaged).
    if let Some(endpoints) = &wiring.endpoints {
        items.push(endpoints_item(&instance.root, endpoints));
    }

    // Launcher state: installer identity + prefix presence. Internal tweak
    // toggles are launcher-owned; file proxies below, nothing more is claimed.
    if let Some(launcher) = &wiring.launcher {
        if let Some(installer) = &launcher.installer {
            if !installer.path.is_absolute() {
                bail!("launcher installer path must be absolute");
            }
            match fs::read(&installer.path) {
                Ok(bytes) => {
                    let ok = installer
                        .sha256
                        .as_ref()
                        .is_none_or(|d| hex(&Sha256::digest(&bytes)) == d.to_ascii_lowercase());
                    items.push(item(
                        "launcher-installer",
                        if ok {
                            ItemState::Verified
                        } else {
                            ItemState::Mismatched
                        },
                        format!("size={}", bytes.len()),
                        if ok {
                            String::new()
                        } else {
                            "installer differs from declared digest".into()
                        },
                    ));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => items.push(item(
                    "launcher-installer",
                    ItemState::Missing,
                    installer.path.display().to_string(),
                    "place the official installer at the declared path".into(),
                )),
                Err(e) => items.push(item(
                    "launcher-installer",
                    ItemState::Unverifiable,
                    format!("{e}"),
                    "inspect installer permissions".into(),
                )),
            }
        }
        if let Some(prefix) = &launcher.prefix {
            items.push(item(
                "launcher-prefix",
                if prefix.join("system.reg").is_file() {
                    ItemState::Verified
                } else {
                    ItemState::Missing
                },
                prefix.display().to_string(),
                "run the launcher install once".into(),
            ));
        }
        for (tweak, wanted) in &launcher.tweaks {
            let proxy = match tweak.as_str() {
                "vanillaFixes" => Some("VanillaFixes.exe"),
                "vanillaHelpers" => Some("VanillaHelpers.dll"),
                "dxvk" => Some("d3d9.dll"),
                _ => None,
            };
            match proxy {
                Some(file) => {
                    let present = instance.root.join(file).is_file();
                    let ok = present == *wanted;
                    items.push(item(
                        &format!("tweak:{tweak}"),
                        if ok {
                            ItemState::Verified
                        } else {
                            ItemState::Mismatched
                        },
                        format!("{file} present={present}"),
                        "toggle in the launcher, Apply, re-check".into(),
                    ));
                }
                None => items.push(item(
                    &format!("tweak:{tweak}"),
                    ItemState::Unverifiable,
                    "no file proxy; launcher owns this toggle".into(),
                    "confirm in the launcher UI".into(),
                )),
            }
        }
    }

    // Declared client version marker (recorded by the operator after each
    // launcher update; the launcher owns version truth).
    if let Some(version) = &wiring.client_version {
        match read_capped(&instance.root, &version.file.to_string_lossy(), 1 << 20) {
            Ok(bytes) => {
                let body = String::from_utf8_lossy(&bytes);
                let ok = body.contains(&version.contains);
                items.push(item(
                    "client-version",
                    if ok {
                        ItemState::Verified
                    } else {
                        ItemState::Mismatched
                    },
                    format!(
                        "marker '{}' {}",
                        version.contains,
                        if ok { "found" } else { "absent" }
                    ),
                    if ok {
                        String::new()
                    } else {
                        "open the launcher and update, then record the new marker".into()
                    },
                ));
            }
            Err(e) => items.push(item(
                "client-version",
                ItemState::Missing,
                format!("{e:#}"),
                "record the version marker after a launcher update".into(),
            )),
        }
    }

    // Wine prefix: sibling dir + system.reg arch (same validator as apply).
    // Hard refusals (symlink, in-tree, incompatible arch) fail the check.
    if let Some(prefix) = &wiring.prefix {
        match validated_prefix(&prefix.path, &instance.root, &wiring)? {
            Some(arch) => items.push(item(
                "prefix",
                ItemState::Verified,
                format!("arch={arch} at {}", prefix.path.display()),
                String::new(),
            )),
            None => items.push(item(
                "prefix",
                ItemState::Missing,
                prefix.path.display().to_string(),
                "run: onboard apply (creates via wineboot)".into(),
            )),
        }
    }

    // Lutris entry: yml content + pga.db row + anticheat absence.
    if let Some(entry) = &wiring.lutris {
        validate_slug(&entry.slug)?;
        match lutris_yml_path(dirs, &entry.slug) {
            Some(path) => {
                items.push(check_lutris_yml(name, &path, instance, &wiring, entry)?);
            }
            None => items.push(item(
                "lutris-yml",
                ItemState::Missing,
                format!("no {}.yml in Lutris config dirs", entry.slug),
                "run: onboard apply".into(),
            )),
        }
        match lutris_db_path(dirs) {
            Some(db) => match sqlite3(&[
                db.display().to_string(),
                format!("SELECT executable FROM games WHERE slug='{}';", entry.slug),
            ]) {
                Ok(row) if !row.is_empty() => items.push(item(
                    "lutris-db",
                    ItemState::Verified,
                    format!("registered: {row}"),
                    String::new(),
                )),
                Ok(_) => items.push(item(
                    "lutris-db",
                    ItemState::Missing,
                    "slug not in pga.db".into(),
                    "run: onboard apply".into(),
                )),
                Err(e) => items.push(item(
                    "lutris-db",
                    ItemState::Unverifiable,
                    format!("{e:#}"),
                    "ensure sqlite3 is available".into(),
                )),
            },
            None => items.push(item(
                "lutris-db",
                ItemState::Missing,
                "no pga.db found (start Lutris once, then close it)".into(),
                "start Lutris once to create its database".into(),
            )),
        }
    }

    // Launcher entry: same adapter, separate slug. Only reported when the
    // installer, its prefix, and the entry are all declared.
    if let Some(launcher) = &wiring.launcher {
        if let (Some(installer), Some(prefix), Some(entry)) =
            (&launcher.installer, &launcher.prefix, &launcher.lutris)
        {
            // The check compares against the recorded runner, never this
            // placeholder: status resolves no runner version of its own.
            let dummy = Runner {
                path: PathBuf::new(),
                version: String::new(),
            };
            let spec = match launcher_spec(
                installer,
                prefix,
                entry,
                &dummy,
                &wiring,
                launcher.executable.as_ref(),
            ) {
                Ok(spec) => Some(spec),
                Err(e) => {
                    items.push(item(
                        "launcher-entry",
                        ItemState::Unverifiable,
                        format!("{e:#}"),
                        "fix the launcher declaration".into(),
                    ));
                    None
                }
            };
            if let Some(spec) = &spec {
                let recorded =
                    read_recorded(instance).map(|record| record.map(|record| record.version));
                match lutris_yml_path(dirs, &entry.slug) {
                    Some(path) => items.push(check_entry_yml(
                        &path,
                        "launcher-entry",
                        "run: onboard register-launcher",
                        spec,
                        recorded,
                    )),
                    None => items.push(item(
                        "launcher-entry",
                        ItemState::Missing,
                        format!("no {}.yml in Lutris config dirs", entry.slug),
                        "run: onboard register-launcher".into(),
                    )),
                }
            }
            // The launcher's stored client folder must equal the declared
            // client root (register-launcher owns it; apply never touches
            // launcher state).
            items.push(launcher_client_dir_item(instance, &wiring, prefix));
        }
    }

    let _ = name;
    Ok(items)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WiringChangeKind {
    /// Onboard apply owns this transition.
    Change,
    /// External action (launcher update, client install) apply never
    /// performs: readiness ones gate launch modes, registration proceeds
    /// without them; structural ones (client install) still refuse.
    Blocker,
}

#[derive(Debug, Clone, Serialize)]
pub struct WiringChange {
    pub resource: String,
    pub kind: WiringChangeKind,
    pub summary: String,
}

/// HD payload presence items (`hd-patch:<letter>` Missing): an absent
/// optional payload is not a broken vanilla client. They stay visible in
/// status/plan and are journaled as deferred after registration; all
/// launch-readiness findings are exempt from registration outright via
/// `is_launch_readiness_item`. The `hd-patch:` prefix matches the
/// per-letter presence items only: neither the `hd-patch-letters` dodge
/// detector nor the `hd-patch-A` identity check carries it.
fn is_deferred_hd_item(item: &StatusItem) -> bool {
    item.state == ItemState::Missing && item.name.starts_with("hd-patch:")
}

/// Launch-readiness findings: required client files (`client-file:*`), the
/// client's digest identity (`wow-exe`), and HD payload state (`hd-patch*`
/// — per-letter presence plus the rename-dodge and patch-A identity
/// detectors). Registration neither reads nor writes them, so they never
/// gate writing an otherwise valid, declared Lutris entry; they stay
/// visible in status/plan and gate execution instead: a game launch
/// requires every `client-file:*` and `wow-exe` item verified plus — when
/// `data_patches` is declared — every `hd-patch*` item verified. Native
/// `onboard launch` and the Lutris entry through its synthesized `onboard
/// gate` share the same policy.
fn is_launch_readiness_item(name: &str) -> bool {
    name == "wow-exe" || name.starts_with("hd-patch") || name.starts_with("client-file:")
}

/// Items apply neither owns nor gates on. The launcher lifecycle
/// (entry, stored client folder, launcher prefix, tweak proxies) belongs
/// to register-launcher and the launcher itself — apply never touches it.
/// Absent external artifacts (HD payloads, the bootstrap installer) never
/// gate registration either; they stay visible and, for HD, journaled as
/// deferred. Launch-readiness findings (required client files, wow-exe
/// digest, HD payload state) gate launch modes, not registration.
/// Everything else non-verified blocks registration or fails verification
/// — notably a digest-mismatched installer.
fn is_apply_exempt(item: &StatusItem) -> bool {
    item.name == "launcher-entry"
        || item.name == "launcher-client-dir"
        || item.name == "launcher-prefix"
        || item.name.starts_with("tweak:")
        || is_launch_readiness_item(&item.name)
        || (item.state == ItemState::Missing
            && (item.name == "launcher-installer" || is_deferred_hd_item(item)))
}

/// Items onboard apply owns; everything else non-verified shows up in
/// plan output as an external blocker (client install, launcher
/// maintenance) that apply never performs. Launch-readiness findings stay
/// visible as blockers but gate launch modes, not registration (see
/// `is_launch_readiness_item`); absent HD payloads are journaled as
/// deferred (see `is_deferred_hd_item`).
fn is_ownable(name: &str) -> bool {
    name == "prefix" || name.starts_with("lutris-") || name == "runner-pinned"
}

/// Read-only wiring plan derived from shared readiness observations:
/// ownable items become changes, the rest are blockers. Never writes,
/// never executes anything.
pub fn plan(name: &str, instance: &Instance, dirs: &HomeDirs) -> Result<Vec<WiringChange>> {
    let mut changes = Vec::new();
    for item in status(name, instance, dirs)? {
        if item.state != ItemState::Verified {
            let kind = if is_ownable(&item.name) {
                WiringChangeKind::Change
            } else {
                WiringChangeKind::Blocker
            };
            changes.push(WiringChange {
                resource: format!("{name}/wiring/{}", item.name),
                kind,
                summary: format!("{:?}: {}", item.state, item.fix),
            });
        }
    }
    Ok(changes)
}

/// Map a pgrep exit to a running verdict: success means a match, exit 1
/// means no match, anything else (signal, exit 2+) is an inspection
/// failure — never mistaken for "not running".
fn interpret_pgrep(status: std::process::ExitStatus) -> Result<bool> {
    if status.success() {
        return Ok(true);
    }
    match status.code() {
        Some(1) => Ok(false),
        Some(code) => bail!("pgrep failed with exit {code}; process state unknown"),
        None => bail!("pgrep terminated by signal; process state unknown"),
    }
}

/// True when a real Lutris process runs: an executable (or argv component)
/// named exactly `lutris`, or the Flatpak app-id. A bare substring would
/// match our own sqlite3 calls carrying lutris data paths in argv.
fn lutris_running() -> Result<bool> {
    let status = Command::new("pgrep")
        .args(["-f", "--", "(^|/)lutris( |$)|net\\.lutris\\.Lutris"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("check lutris processes")?;
    interpret_pgrep(status)
}

/// Render the deterministic Lutris game yml for this instance, including
/// the synthesized launch-gate `prefix_command`. Anti-cheat is always
/// absent: no EAC/BattleEye keys are ever emitted.
pub fn render_lutris_yml(
    name: &str,
    instance: &Instance,
    entry: &LutrisEntry,
    runner: &Runner,
    wiring: &Wiring,
) -> Result<String> {
    render_entry_yml(&game_spec(name, instance, entry, runner, wiring)?)
}

pub fn render_entry_yml(spec: &EntrySpec) -> Result<String> {
    // ponytail: Vec pairs, not a map — yaml_ng::Value has no Ord.
    let mut env: Vec<(serde_yaml_ng::Value, serde_yaml_ng::Value)> = Vec::new();
    if !spec.dll_overrides.is_empty() {
        env.push((
            serde_yaml_ng::Value::String("WINEDLLOVERRIDES".into()),
            serde_yaml_ng::Value::String(spec.dll_overrides.join(";")),
        ));
    }
    for (key, value) in &spec.extra_env {
        if key == "WINEDLLOVERRIDES" {
            continue;
        }
        env.push((
            serde_yaml_ng::Value::String(key.clone()),
            serde_yaml_ng::Value::String(value.clone()),
        ));
    }
    let mut system = vec![
        (
            serde_yaml_ng::Value::String("disable_runtime".into()),
            // System wine is a host artifact: Lutris must not prepend
            // its runtime lib folders (`/lib`, `/usr/lib` resolve to
            // the FHS glibc, breaking the host wrapper with a libc
            // symbol lookup error). Lutris-managed runners keep the
            // runtime they were built against.
            serde_yaml_ng::Value::Bool(spec.runner_version == "system"),
        ),
        (
            serde_yaml_ng::Value::String("env".into()),
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter(env)),
        ),
    ];
    if let Some(prefix) = &spec.command_prefix {
        system.push((
            serde_yaml_ng::Value::String("prefix_command".into()),
            serde_yaml_ng::Value::String(prefix.clone()),
        ));
    }
    let doc = serde_yaml_ng::Mapping::from_iter([
        (
            serde_yaml_ng::Value::String("game".into()),
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter([
                (
                    serde_yaml_ng::Value::String("exe".into()),
                    serde_yaml_ng::Value::String(spec.exe.clone()),
                ),
                (
                    serde_yaml_ng::Value::String("working_dir".into()),
                    serde_yaml_ng::Value::String(spec.dir.clone()),
                ),
                (
                    serde_yaml_ng::Value::String("prefix".into()),
                    serde_yaml_ng::Value::String(
                        spec.prefix
                            .clone()
                            .context("lutris entry needs a declared prefix path")?,
                    ),
                ),
            ])),
        ),
        (
            serde_yaml_ng::Value::String("game_slug".into()),
            serde_yaml_ng::Value::String(spec.game_slug.clone()),
        ),
        (
            serde_yaml_ng::Value::String("name".into()),
            serde_yaml_ng::Value::String(spec.name.clone()),
        ),
        (
            serde_yaml_ng::Value::String("runner".into()),
            serde_yaml_ng::Value::String("wine".into()),
        ),
        (
            serde_yaml_ng::Value::String("slug".into()),
            serde_yaml_ng::Value::String(spec.slug.clone()),
        ),
        (
            serde_yaml_ng::Value::String("version".into()),
            serde_yaml_ng::Value::String("Community".into()),
        ),
        (
            serde_yaml_ng::Value::String("wine".into()),
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter([
                (
                    serde_yaml_ng::Value::String("version".into()),
                    serde_yaml_ng::Value::String(spec.runner_version.clone()),
                ),
                (
                    serde_yaml_ng::Value::String("arch".into()),
                    serde_yaml_ng::Value::String(spec.wine_arch.clone()),
                ),
                (
                    serde_yaml_ng::Value::String("dxvk".into()),
                    serde_yaml_ng::Value::Bool(spec.dxvk),
                ),
                (
                    serde_yaml_ng::Value::String("vkd3d".into()),
                    serde_yaml_ng::Value::Bool(spec.vkd3d),
                ),
                (
                    serde_yaml_ng::Value::String("esync".into()),
                    serde_yaml_ng::Value::Bool(spec.esync),
                ),
                (
                    serde_yaml_ng::Value::String("fsync".into()),
                    serde_yaml_ng::Value::Bool(spec.fsync),
                ),
                // Anti-cheat is always off for this client: explicit false,
                // never absent-by-luck.
                (
                    serde_yaml_ng::Value::String("eac".into()),
                    serde_yaml_ng::Value::Bool(false),
                ),
                (
                    serde_yaml_ng::Value::String("battleye".into()),
                    serde_yaml_ng::Value::Bool(false),
                ),
            ])),
        ),
        (
            serde_yaml_ng::Value::String("system".into()),
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter(system)),
        ),
    ]);
    serde_yaml_ng::to_string(&serde_yaml_ng::Value::Mapping(doc)).context("render lutris yml")
}

static BACKUP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Unique retained backup name: seconds + pid + in-process sequence, so two
/// backups in one apply can never collide or overwrite earlier evidence.
fn backup_name(path: &Path, tag: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let seq = BACKUP_SEQ.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!(
        "bak-modde-{tag}-{stamp}-{}-{seq}",
        std::process::id()
    ))
}

/// Content-aware yml write: identical content is a no-op (repeat apply
/// changes nothing); otherwise the previous file is retained under a unique
/// backup name. Returns (changed, backup taken, if any) so a later failed
/// step can restore the previous content.
fn write_yml(path: &Path, body: &str) -> Result<(bool, Option<PathBuf>)> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if path.is_file() {
        let current = fs::read(path)?;
        if current == body.as_bytes() {
            return Ok((false, None));
        }
        let backup = backup_name(path, "yml");
        fs::copy(path, &backup)?;
        atomic_write_0600(path, body.as_bytes())?;
        return Ok((true, Some(backup)));
    }
    atomic_write_0600(path, body.as_bytes())?;
    Ok((true, None))
}

/// WAL-safe database backup through SQLite itself; a raw file copy can miss
/// WAL state.
fn backup_db(db: &Path) -> Result<PathBuf> {
    let backup = backup_name(db, "db");
    sqlite3(&[
        db.display().to_string(),
        format!(
            ".backup main '{}'",
            backup.display().to_string().replace('\'', "''")
        ),
    ])?;
    Ok(backup)
}

fn pga_row(dirs: &HomeDirs, slug: &str) -> Result<(Option<PathBuf>, String)> {
    let Some(db) = lutris_db_path(dirs) else {
        return Ok((None, String::new()));
    };
    let row = sqlite3(&[
        db.display().to_string(),
        format!(
            "SELECT name || '|' || runner || '|' || platform || '|' || directory || '|' || executable || '|' || configpath || '|' || installed FROM games WHERE slug='{slug}';"
        ),
    ])?;
    Ok((Some(db), row))
}

/// Backup-first row upsert. Returns the action taken; an identical existing
/// row is a no-op (repeat apply changes nothing).
fn upsert_pga_row(dirs: &HomeDirs, entry: &LutrisEntry, exe: &str, dir: &str) -> Result<String> {
    let (Some(db), existing) = pga_row(dirs, &entry.slug)? else {
        bail!("no pga.db found (start Lutris once, then close it)");
    };
    let want = format!("{}|wine|Linux|{}|{}|{}|1", entry.name, dir, exe, entry.slug);
    if existing == want {
        return Ok("unchanged".into());
    }
    let backup = backup_db(&db)?;
    if existing.is_empty() {
        sqlite3(&[
            db.display().to_string(),
            format!(
                "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed) VALUES ('{}', '{}', 'wine', 'Linux', '{}', '{}', '{}', 1);",
                entry.name.replace('\'', "''"),
                entry.slug,
                dir.replace('\'', "''"),
                exe.replace('\'', "''"),
                entry.slug.replace('\'', "''"),
            ),
        ])?;
        println!("db backup retained: {}", backup.display());
        Ok("registered".into())
    } else {
        sqlite3(&[
            db.display().to_string(),
            format!(
                "UPDATE games SET name='{}', runner='wine', platform='Linux', directory='{}', executable='{}', configpath='{}', installed=1 WHERE slug='{}';",
                entry.name.replace('\'', "''"),
                dir.replace('\'', "''"),
                exe.replace('\'', "''"),
                entry.slug.replace('\'', "''"),
                entry.slug,
            ),
        ])?;
        println!("db backup retained: {}", backup.display());
        Ok("updated".into())
    }
}

fn want_prefix_arch(wiring: &Wiring) -> &str {
    mapped_arch(&wiring.runtime.arch)
}

/// Validated registration work for one entry: paired yml target, rendered
/// body, and the live database row. Read-only to construct; `execute_entry`
/// performs the writes.
pub struct EntryPlan {
    pub target: PathBuf,
    pub body: String,
}

pub fn plan_entry(
    dirs: &HomeDirs,
    entry: &LutrisEntry,
    spec: &EntrySpec,
    adopt: bool,
) -> Result<EntryPlan> {
    validate_slug(&entry.slug)?;
    if lutris_running()? {
        bail!("lutris is running; close it completely before onboarding");
    }
    // The database must exist before anything is written: a missing
    // database is a blocker, not something apply works around. The yml
    // target is the config dir paired with that database.
    let (site_config, _) =
        lutris_site(dirs).context("no pga.db found (start Lutris once, then close it)")?;
    let target = site_config.join(format!("{}.yml", entry.slug));
    let body = render_entry_yml(spec)?;
    let (db, row) = pga_row(dirs, &entry.slug)?;
    if db.is_none() {
        bail!("no pga.db found (start Lutris once, then close it)");
    }
    // Ownership up front: a row pointing at another executable or
    // directory needs --adopt; display-name drift is our own update.
    // An orphaned yml (no row at all) needs --adopt too.
    if !row.is_empty() {
        let parts: Vec<&str> = row.split('|').collect();
        let same_identity =
            parts.get(3) == Some(&spec.dir.as_str()) && parts.get(4) == Some(&spec.exe.as_str());
        if !same_identity && !adopt {
            bail!(
                "lutris entry '{}' points elsewhere; pass --adopt to take it over",
                entry.slug
            );
        }
    } else if target.is_file() && fs::read(&target)? != body.as_bytes() && !adopt {
        bail!(
            "orphaned {}.yml with no database row; pass --adopt to take it over",
            entry.slug
        );
    }
    Ok(EntryPlan { target, body })
}

/// Execute a planned entry registration: content-aware yml write plus
/// backup-first row upsert. Returns (yml changed, db action). If the
/// database step fails, the yml is restored from the retained backup — or,
/// for a first registration, the created orphan is removed again (only if
/// still byte-identical to what was written).
pub fn execute_entry(
    dirs: &HomeDirs,
    entry: &LutrisEntry,
    spec: &EntrySpec,
    plan: &EntryPlan,
) -> Result<(bool, String)> {
    let target_existed = plan.target.is_file();
    let (changed, backup) = write_yml(&plan.target, &plan.body)?;
    match upsert_pga_row(dirs, entry, &spec.exe, &spec.dir) {
        Ok(action) => Ok((changed, action)),
        Err(error) => {
            if let Some(backup) = &backup {
                let previous = fs::read(backup)?;
                atomic_write_0600(&plan.target, &previous)?;
                eprintln!("database step failed; yml restored; evidence retained");
            } else if !target_existed
                && fs::read(&plan.target).is_ok_and(|current| current == plan.body.as_bytes())
            {
                let _ = fs::remove_file(&plan.target);
                eprintln!("database step failed; new yml removed; evidence retained");
            } else if !target_existed {
                eprintln!("database step failed; yml changed underneath, left in place");
            }
            Err(error).context("lutris database update failed")
        }
    }
}

/// Validate a prefix path without following anything: the parent chain must
/// anchor-open (fails closed on symlinks), the final component must be an
/// absent path or a real directory — never a symlink. Returns the present
/// arch, if any.
fn validated_prefix(prefix: &Path, root: &Path, wiring: &Wiring) -> Result<Option<String>> {
    if !prefix.is_absolute() {
        bail!("prefix path must be absolute");
    }
    if overlap(prefix, root) {
        bail!("prefix must be a sibling, never inside the game folder");
    }
    let parent = prefix.parent().context("prefix needs a parent")?;
    let anchor = Anchor::open(parent).context("invalid prefix parent")?;
    let name = prefix.file_name().context("prefix needs a name")?;
    let candidate = fd_path(&anchor.file).join(name);
    match fs::symlink_metadata(&candidate) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context(format!("inspect {}", prefix.display())),
        Ok(meta) => {
            if meta.file_type().is_symlink() {
                bail!("prefix path is a symlink; remove it explicitly");
            }
            if !meta.is_dir() {
                bail!("prefix path exists and is not a directory");
            }
            let arch = prefix_arch(prefix)?;
            if let Some(arch) = &arch {
                if arch != want_prefix_arch(wiring) {
                    bail!("incompatible prefix arch={arch}; remove it explicitly");
                }
            }
            Ok(arch)
        }
    }
}

/// Entry names under `Data/` without reading payloads: HD trees are
/// gigabytes, and presence checks need names only. The directory itself is
/// anchor-validated; entries are listed by name via `symlink_metadata`
/// semantics (never followed).
fn data_entry_names(root: &Path) -> Result<Option<Vec<String>>> {
    let data = root.join("Data");
    let anchor = match Anchor::open(&data) {
        Ok(anchor) => anchor,
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    let mut names = Vec::new();
    for entry in fs::read_dir(fd_path(&anchor.file))? {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    Ok(Some(names))
}

/// Classify a `Data/patch-*.mpq` name: stock numbered patches are always
/// accepted; single letters must be declared native; anything else is
/// unexpected (provenance check, not a crash claim).
fn classify_patch(name: &str, native_letters: &[String]) -> Option<String> {
    let core = name
        .to_ascii_lowercase()
        .strip_prefix("patch-")?
        .strip_suffix(".mpq")?
        .to_owned();
    if core.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if core.len() == 1
        && native_letters
            .iter()
            .any(|known| known.eq_ignore_ascii_case(&core))
    {
        return None;
    }
    Some(format!("{name} (undeclared patch file)"))
}

/// Fully validated onboarding operation: every prerequisite (processes,
/// runner, prefix, Lutris config dir, database presence, entry ownership)
/// is established before the first mutation, so apply cannot discover a
/// blocker halfway through. Read-only to construct.
pub struct PreparedWiring {
    wiring: Wiring,
    runner: Runner,
    prefix: Option<PathBuf>,
    prefix_present: bool,
    yml_target: Option<PathBuf>,
    yml_body: String,
    /// Lutris data-dir lock, acquired before the registration is inspected
    /// and held through recovery: concurrent onboard runs (any instance)
    /// cannot invalidate the ownership check.
    _db_guard: Option<Lease>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Best-effort operation journal in the state dir: apply attempts, failures,
/// and recovery actions. Never fails the operation itself.
fn journal_event(instance: &Instance, name: &str, event: serde_json::Value) {
    let Some(state) = &instance.state_dir else {
        return;
    };
    let mut entry = serde_json::Map::new();
    entry.insert("ts".into(), serde_json::Value::from(now_secs()));
    entry.insert("instance".into(), serde_json::Value::from(name));
    if let serde_json::Value::Object(map) = event {
        for (key, value) in map {
            entry.insert(key, value);
        }
    }
    let mut line = serde_json::Value::Object(entry).to_string();
    line.push('\n');
    let path = state.join("wiring-journal.jsonl");
    if path
        .parent()
        .is_some_and(|parent| fs::create_dir_all(parent).is_err())
    {
        return;
    }
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = file.write_all(line.as_bytes());
    }
}

/// Scrubbed process environment for `wineboot --init`: returns the inherited
/// `WINE*` variables to remove plus the explicit values to set. Everything
/// else (Nix runtime paths, HOME, XDG_RUNTIME_DIR, desktop session) is
/// preserved — wineboot exits 0 without initializing when the session
/// environment is missing. Pure over a snapshot for testability.
fn scrub_wine_env(
    current: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    prefix: &Path,
    arch: &str,
) -> (
    Vec<std::ffi::OsString>,
    Vec<(std::ffi::OsString, std::ffi::OsString)>,
) {
    let remove: Vec<std::ffi::OsString> = current
        .iter()
        .filter(|(key, _)| key.to_string_lossy().starts_with("WINE"))
        .map(|(key, _)| key.clone())
        .collect();
    let mut set = vec![
        ("WINEPREFIX".into(), prefix.as_os_str().to_owned()),
        ("WINEDEBUG".into(), "-all".into()),
    ];
    if arch == "win32" {
        set.push(("WINEARCH".into(), "win32".into()));
    }
    (remove, set)
}

pub fn prepare(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    reselect: bool,
    adopt: bool,
) -> Result<PreparedWiring> {
    // NOTE: read-only validation; the caller (apply) holds the root lease
    // across validation, execution, verification, and recovery.
    // Offline declaration shapes fail first, before any filesystem work.
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    Anchor::open(&instance.root)?;
    super::assert_stopped(instance)?;

    let (runner, _) = select_runner(dirs, instance, &wiring, reselect)
        .context("cannot onboard without a resolved runner")?;

    let mut prefix = None;
    let mut prefix_present = false;
    if let Some(declared) = &wiring.prefix {
        match validated_prefix(&declared.path, &instance.root, &wiring)? {
            Some(_) => {
                prefix_present = true;
                prefix = Some(declared.path.clone());
            }
            None => prefix = Some(declared.path.clone()),
        }
    }

    let mut yml_target = None;
    let mut yml_body = String::new();
    let mut db_guard = None;
    if let Some(entry) = &wiring.lutris {
        // The data-dir lock is taken before the row is even read, so a
        // concurrent onboard run cannot invalidate the ownership check.
        let (_, site_data) =
            lutris_site(dirs).context("no pga.db found (start Lutris once, then close it)")?;
        db_guard = Some(
            Anchor::open(&site_data)
                .context("invalid Lutris data dir")
                .and_then(|anchor| {
                    anchor
                        .lock()
                        .context("another onboard run holds the Lutris database")
                })?,
        );
        let spec = game_spec(name, instance, entry, &runner, &wiring)?;
        let entry_plan = plan_entry(dirs, entry, &spec, adopt)?;
        yml_target = Some(entry_plan.target);
        yml_body = entry_plan.body;
    }

    // Structural prerequisites (endpoints, runner) are external
    // maintenance (launcher updates, client install): anything apply does
    // not own must already verify, or no mutation happens at all. Same
    // observations status/plan report. The launcher lifecycle is owned by
    // register-launcher, and launch-readiness findings (required client
    // files, wow-exe digest, HD payload state) gate launch modes — a valid
    // declared entry must not wait for any of them.
    let blockers: Vec<_> = status(name, instance, dirs)?
        .into_iter()
        .filter(|item| item.state != ItemState::Verified && !is_ownable(&item.name) && !is_apply_exempt(item))
        .collect();
    if !blockers.is_empty() {
        bail!(
            "unmet prerequisites: {}",
            blockers
                .iter()
                .map(|item| format!("{}={:?}", item.name, item.state))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    let _ = name;
    Ok(PreparedWiring {
        wiring,
        runner,
        prefix,
        prefix_present,
        yml_target,
        yml_body,
        _db_guard: db_guard,
    })
}

/// The wineserver paired with a runner: the sibling binary when present,
/// otherwise PATH resolution.
fn wineserver_bin(runner: &Runner) -> PathBuf {
    let sibling = runner.path.parent().map(|dir| dir.join("wineserver"));
    match sibling {
        Some(path) if is_executable(&path) => path,
        _ => PathBuf::from("wineserver"),
    }
}

/// Create a declared-but-absent prefix: single-level directory creation
/// under a re-validated parent, then `wineboot --init` with a scrubbed
/// environment. `wineboot` exits before the server flushes the registry,
/// so `wineserver -w` (same prefix) runs before verification. The produced
/// architecture must equal the declared one.
fn create_prefix(prefix: &Path, runner: &Runner, wiring: &Wiring) -> Result<String> {
    let parent = prefix.parent().context("prefix needs a parent")?;
    let anchor = Anchor::open(parent)?;
    let candidate = fd_path(&anchor.file).join(prefix.file_name().context("prefix needs a name")?);
    match fs::create_dir(&candidate) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            bail!("prefix appeared during apply; re-run status")
        }
        Err(e) => return Err(e).context(format!("create {}", prefix.display())),
    }
    let mut cmd = Command::new(&runner.path);
    cmd.arg("wineboot").arg("--init");
    let (remove, set) = scrub_wine_env(std::env::vars_os().collect(), prefix, &wiring.runtime.arch);
    for key in remove {
        cmd.env_remove(key);
    }
    for (key, value) in set {
        cmd.env(key, value);
    }
    let status = cmd.status().context("run wineboot --init")?;
    if !status.success() {
        bail!("wineboot --init failed: {status}");
    }
    // The server flushes the registry after its clients exit; without this
    // wait the prefix looks uninitialized despite exit 0.
    let (remove, set) = scrub_wine_env(std::env::vars_os().collect(), prefix, &wiring.runtime.arch);
    let mut wait = Command::new(wineserver_bin(runner));
    wait.arg("-w");
    for key in remove {
        wait.env_remove(key);
    }
    for (key, value) in set {
        wait.env(key, value);
    }
    let status = wait.status().context("wait for wineserver")?;
    if !status.success() {
        bail!("wineserver -w failed: {status}");
    }
    let want = want_prefix_arch(wiring);
    match prefix_arch(prefix)? {
        Some(arch) if arch == want => Ok(arch),
        Some(arch) => bail!("wineboot produced arch={arch}, want={want}"),
        None => bail!("prefix creation produced no system.reg"),
    }
}

/// Execute a prepared plan. The root lease is held across validation,
/// execution, verification, and recovery; a second lock on the Lutris data
/// dir serializes concurrent onboard runs for other instances. Owns prefix
/// creation, the Lutris yml, and the pga.db row — nothing else.
/// Launch-readiness findings (wow-exe digest, HD payload state) never
/// gate registration: they stay visible in status/plan, missing HD is
/// journaled as deferred, and they gate launch modes instead.
/// Partial state is retained with journaled evidence on failure, never
/// deleted; pre-existing content is restored from retained backups.
pub fn apply(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    adopt: bool,
    reselect: bool,
    expect_runner: Option<&str>,
) -> Result<()> {
    let root_anchor = Anchor::open(&instance.root)?;
    let _lease = root_anchor.lock()?;
    let prepared = prepare(name, instance, dirs, reselect, adopt)?;
    if let Some(expected) = expect_runner {
        if prepared.runner.version != expected {
            bail!(
                "resolved runner '{}' differs from expected '{expected}'; refusing to switch under a reviewed plan",
                prepared.runner.version
            );
        }
    }
    // A repeat apply must be a literal no-op: the journal records mutations
    // and failures only, never routine verifications.
    let mut mutated = false;

    if let Some(prefix) = &prepared.prefix {
        if prepared.prefix_present {
            println!("{name}: prefix already present");
        } else {
            match create_prefix(prefix, &prepared.runner, &prepared.wiring) {
                Ok(arch) => {
                    mutated = true;
                    println!("{name}: prefix created ({arch})");
                }
                Err(error) => {
                    // Never delete: a prefix appearing between validation and
                    // creation is someone else's; a partial one needs explicit
                    // operator recovery. Both stay journaled as evidence.
                    journal_event(
                        instance,
                        name,
                        serde_json::json!({"op": "apply-failed", "step": "prefix", "error": format!("{error:#}")}),
                    );
                    return Err(error);
                }
            }
        }
    }

    // The Lutris data-dir lock travels inside `prepared` (acquired before
    // inspection), so concurrent onboard runs cannot invalidate ownership.
    if let (Some(entry), Some(target)) = (&prepared.wiring.lutris, &prepared.yml_target) {
        let spec = game_spec(name, instance, entry, &prepared.runner, &prepared.wiring)?;
        let entry_plan = EntryPlan {
            target: target.clone(),
            body: prepared.yml_body.clone(),
        };
        match execute_entry(dirs, entry, &spec, &entry_plan) {
            Ok((changed, action)) => {
                mutated |= changed || action != "unchanged";
                println!(
                    "{name}: lutris yml {}",
                    if changed {
                        target.display().to_string()
                    } else {
                        "unchanged".into()
                    }
                );
                println!("{name}: lutris entry {action}");
            }
            Err(error) => {
                journal_event(
                    instance,
                    name,
                    serde_json::json!({"op": "apply-failed", "step": "lutris-db", "error": format!("{error:#}")}),
                );
                return Err(error);
            }
        }
    }

    if record_runner(instance, &prepared.runner)? {
        mutated = true;
        println!("{name}: runtime selection recorded");
    }
    let bad: Vec<_> = status(name, instance, dirs)?
        .into_iter()
        .filter(|item| item.state != ItemState::Verified)
        .collect();
    // Launch-readiness findings stay visible but never fail registration,
    // and register-owned launcher items are verified by register-launcher,
    // not here; anything else unverified is a real failure with retained
    // evidence.
    let fatal: Vec<_> = bad
        .iter()
        .filter(|item| !is_apply_exempt(item))
        .collect();
    let hd_deferred: Vec<_> = bad
        .iter()
        .filter(|item| is_deferred_hd_item(item))
        .map(|item| item.name.clone())
        .collect();
    // Launch-readiness findings observed but deliberately not enforced at
    // registration: journaled so the deferral is auditable afterwards.
    let readiness: Vec<_> = bad
        .iter()
        .filter(|item| is_launch_readiness_item(&item.name))
        .map(|item| format!("{}={:?}", item.name, item.state))
        .collect();
    if !fatal.is_empty() {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "apply-failed", "step": "verify"}),
        );
        bail!(
            "post-apply verification failed: {}",
            fatal
                .iter()
                .map(|item| format!("{}={:?}", item.name, item.state))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    // A repeat apply stays a literal no-op: the journal records mutations
    // and failures only, never routine verifications — the deferral notice
    // below is stdout only.
    if mutated {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "apply-done", "runner": prepared.runner.version, "hd_deferred": hd_deferred, "readiness": readiness}),
        );
    }
    if !hd_deferred.is_empty() {
        println!(
            "{name}: HD deferred (still missing: {}); vanilla entry registered",
            hd_deferred.join(", ")
        );
    }
    println!("{name}: onboard apply complete");
    Ok(())
}

/// What a native launch runs: game, installed maintenance launcher, or the
/// installer as an explicit bootstrap operation. Separate values because
/// each has its own executable, working directory, and prefix — never one
/// evolving entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeTarget {
    Game,
    Launcher,
    Installer,
}

/// Launch mode: a game launch always enforces the declared client files,
/// executable identity, and — when declared — the approved HD set. HD mode
/// additionally requires a declared set with every item verified, and never
/// silently downgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchMode {
    Vanilla,
    Hd,
}

/// A resolved native launch: executable, working directory, Wine prefix,
/// client DLL overrides, and the effective tunings plus runtime arch for
/// the environment. Game targets use the declared tunings; launcher and
/// installer targets use the launcher-effective tunings. Only the `env`
/// map plus DLL overrides affect native execution (see
/// `resolve_launch_env_with`): the Lutris wine toggles
/// (dxvk/vkd3d/esync/fsync) are Lutris-entry scope and are intentionally
/// not translated into native Wine variables.
pub struct LaunchTarget {
    pub exe: PathBuf,
    pub dir: PathBuf,
    pub prefix: PathBuf,
    pub dll_overrides: Vec<String>,
    pub tunings: Tunings,
    pub arch: String,
}

/// Single-descriptor file check shared by registration and native launch:
/// no symlinks, regular file, streaming digest when the declaration pins
/// one.
fn validate_descriptor_file(path: &Path, sha256: Option<&str>, what: &str) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = match fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.raw_os_error() == Some(libc::ELOOP) => {
            bail!("{what} is a symlink: {}", path.display())
        }
        Err(e) => return Err(e).context(format!("{what} unreadable")),
    };
    if !file.metadata()?.is_file() {
        bail!("{what} is not a file: {}", path.display());
    }
    if let Some(digest) = sha256 {
        let (_, actual) = stream_digest_file(&mut file)?;
        if actual != digest.to_ascii_lowercase() {
            bail!("{what} digest mismatch");
        }
    }
    Ok(())
}

/// The executable a native launch will run: must exist as a regular file.
/// Symlinks fail closed (resolved at registration; never followed here).
fn launch_exe(path: &Path, what: &str) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!("{what} is a symlink: {}", path.display())
        }
        Ok(meta) if meta.is_file() => Ok(path.to_owned()),
        Ok(_) => bail!("{what} is not a regular file: {}", path.display()),
        Err(e) => Err(e).context(format!("{what} unreadable: {}", path.display())),
    }
}

/// Installed launcher executable checks shared by registration and native
/// launch: absolute, traversal-free, under its own prefix (never an
/// arbitrary host executable a launcher entry would then run), regular
/// file, no symlinks.
fn validate_launcher_executable(executable: &Path, prefix: &Path) -> Result<()> {
    if !executable.is_absolute() {
        bail!("launcher executable path must be absolute");
    }
    if has_parent_traversal(executable) {
        bail!(
            "launcher executable path must not contain '..': {}",
            executable.display()
        );
    }
    if has_parent_traversal(prefix) {
        bail!(
            "launcher prefix path must not contain '..': {}",
            prefix.display()
        );
    }
    if !executable.starts_with(prefix) {
        bail!(
            "launcher executable must live under the launcher prefix: {}",
            executable.display()
        );
    }
    launch_exe(executable, "launcher executable")?;
    Ok(())
}

/// Resolve what a native launch runs. Prefixes must already exist (prepare
/// and register-launcher own creation); launch itself creates nothing and
/// reconciles nothing.
fn resolve_launch_target(
    instance: &Instance,
    wiring: &Wiring,
    target: NativeTarget,
) -> Result<LaunchTarget> {
    match target {
        NativeTarget::Game => {
            let prefix = wiring.prefix.as_ref().context("no game prefix declared")?;
            validated_prefix(&prefix.path, &instance.root, wiring)?.with_context(|| {
                format!(
                    "game prefix absent: {}; run onboard prepare first",
                    prefix.path.display()
                )
            })?;
            let rel = existing_name(&instance.root, &[&wiring.launch.executable])?.with_context(|| {
                format!(
                    "game executable '{}' missing from {}; install/verify the client first",
                    wiring.launch.executable,
                    instance.root.display()
                )
            })?;
            let exe = launch_exe(&instance.root.join(&rel), "game executable")?;
            Ok(LaunchTarget {
                dir: instance.root.clone(),
                exe,
                prefix: prefix.path.clone(),
                dll_overrides: wiring.dll_overrides.clone(),
                tunings: wiring.tunings.clone(),
                arch: wiring.runtime.arch.clone(),
            })
        }
        NativeTarget::Launcher => {
            let launcher = wiring.launcher.as_ref().context("no launcher block declared")?;
            let prefix = launcher.prefix.as_ref().context("no launcher prefix declared")?;
            validated_prefix(prefix, &instance.root, wiring)?.with_context(|| {
                format!(
                    "launcher prefix absent: {}; run onboard register-launcher first",
                    prefix.display()
                )
            })?;
            let executable = launcher.executable.as_ref().context(
                "no installed launcher executable declared; declare it after installation (the installer stays bootstrap-only)",
            )?;
            validate_launcher_executable(executable, prefix)?;
            let dir = executable
                .parent()
                .context("launcher executable needs a parent directory")?
                .to_owned();
            Ok(LaunchTarget {
                exe: executable.clone(),
                dir,
                prefix: prefix.clone(),
                // Non-game process: no game-client DLL overrides (mirrors
                // the launcher Lutris entry).
                dll_overrides: Vec::new(),
                tunings: launcher_effective_tunings(wiring),
                arch: wiring.runtime.arch.clone(),
            })
        }
        NativeTarget::Installer => {
            let launcher = wiring.launcher.as_ref().context("no launcher block declared")?;
            let installer = launcher
                .installer
                .as_ref()
                .context("no installer declared in launcher block")?;
            if !installer.path.is_absolute() {
                bail!("installer path must be absolute");
            }
            validate_descriptor_file(&installer.path, installer.sha256.as_deref(), "installer")?;
            let prefix = launcher.prefix.as_ref().context("no launcher prefix declared")?;
            validated_prefix(prefix, &instance.root, wiring)?.with_context(|| {
                format!(
                    "launcher prefix absent: {}; run onboard register-launcher first",
                    prefix.display()
                )
            })?;
            let dir = installer
                .path
                .parent()
                .context("installer needs a parent directory")?
                .to_owned();
            Ok(LaunchTarget {
                exe: installer.path.clone(),
                dir,
                prefix: prefix.clone(),
                dll_overrides: Vec::new(),
                tunings: launcher_effective_tunings(wiring),
                arch: wiring.runtime.arch.clone(),
            })
        }
    }
}

/// Native launch environment: the scrubbed Wine base (same helper as
/// prefix creation) plus the declared `env` map — the typed
/// `dll_overrides` win over an `extra_env` `WINEDLLOVERRIDES`, structural
/// `WINEPREFIX`/`WINEARCH` win over tunings, and a declared `WINEDEBUG`
/// wins over the scrubbed default. The Lutris wine toggles
/// (dxvk/vkd3d/esync/fsync) are Lutris-entry scope only and are
/// intentionally not translated into native Wine variables here; native
/// and Lutris executions therefore share env/DLL parity but not toggle
/// parity. Pure for testability; returns (removals, assignments).
fn resolve_launch_env_with(
    tunings: &Tunings,
    arch: &str,
    prefix: &Path,
    dll_overrides: &[String],
) -> (
    Vec<std::ffi::OsString>,
    Vec<(std::ffi::OsString, std::ffi::OsString)>,
) {
    let (remove, set) = scrub_wine_env(std::env::vars_os().collect(), prefix, arch);
    let mut merged: std::collections::BTreeMap<std::ffi::OsString, std::ffi::OsString> =
        set.into_iter().collect();
    for (key, value) in &tunings.env {
        if key == "WINEPREFIX" || key == "WINEARCH" || key == "WINEDLLOVERRIDES" {
            continue;
        }
        merged.insert(key.into(), value.into());
    }
    if !dll_overrides.is_empty() {
        merged.insert("WINEDLLOVERRIDES".into(), dll_overrides.join(";").into());
    }
    (remove, merged.into_iter().collect())
}

/// Launcher/installer native environment: launcher-effective `env` with
/// the same structural wins as the game path. Lutris wine toggles stay
/// Lutris-scoped (see `resolve_launch_env_with`).
fn resolve_launcher_env(
    wiring: &Wiring,
    prefix: &Path,
    dll_overrides: &[String],
) -> (
    Vec<std::ffi::OsString>,
    Vec<(std::ffi::OsString, std::ffi::OsString)>,
) {
    let tunings = launcher_effective_tunings(wiring);
    resolve_launch_env_with(&tunings, &wiring.runtime.arch, prefix, dll_overrides)
}

/// Native runtime preparation without any Lutris touch: resolve and record
/// the runner, ensure the game prefix. No yml, no database row, no Lutris
/// process checks — this works with Lutris never installed. The launcher
/// prefix stays owned by register-launcher.
pub fn prepare_native(name: &str, instance: &Instance, dirs: &HomeDirs, reselect: bool) -> Result<()> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    let root_anchor = Anchor::open(&instance.root)?;
    let _lease = root_anchor.lock()?;
    // Discovery only: runner search dirs are read, nothing Lutris-owned is
    // written here.
    let (runner, _) =
        select_runner(dirs, instance, &wiring, reselect).context("cannot prepare without a resolved runner")?;
    let mut mutated = false;
    if record_runner(instance, &runner)? {
        mutated = true;
        println!("{name}: runtime selection recorded");
    }
    if let Some(prefix) = wiring.prefix.as_ref() {
        match validated_prefix(&prefix.path, &instance.root, &wiring)? {
            Some(arch) => println!("{name}: prefix already present ({arch})"),
            None => match create_prefix(&prefix.path, &runner, &wiring) {
                Ok(arch) => {
                    mutated = true;
                    println!("{name}: prefix created ({arch})");
                }
                Err(error) => {
                    journal_event(
                        instance,
                        name,
                        serde_json::json!({"op": "prepare-failed", "step": "prefix", "error": format!("{error:#}")}),
                    );
                    return Err(error);
                }
            },
        }
    }
    if mutated {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "prepare-done", "runner": runner.version}),
        );
    }
    println!("{name}: onboard prepare complete");
    Ok(())
}

/// The recorded runner only: launching and upgrading never resolve or
/// record, so the executed artifact is always the reviewed one.
fn recorded_runner(name: &str, instance: &Instance) -> Result<Runner> {
    let recorded = read_recorded(instance)?
        .with_context(|| format!("no recorded runner for '{name}'; run onboard prepare first"))?;
    if !is_executable(&recorded.path) {
        bail!(
            "recorded runner '{}' no longer executes; run onboard prepare --reselect",
            recorded.path.display()
        );
    }
    Ok(Runner {
        path: recorded.path,
        version: recorded.version,
    })
}

/// Launch readiness gates execution, never registration: a game launch
/// requires the declared client files, the pinned executable identity, and
/// — when `data_patches` is declared — the full approved HD set. An HD-mode
/// launch additionally requires a declared HD set and every `hd-patch*`
/// item verified. A drifted WoW.exe is never exec'd, native `onboard
/// launch` or through the Lutris entry's `onboard gate` alike (the game
/// loads it through `VanillaFixes` either way). Present HD MPQs load in
/// every mode, so there is no "run without HD mode for the base client"
/// downgrade: restore the operator-confirmed set instead.
fn ensure_launch_readiness(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    target: NativeTarget,
    mode: LaunchMode,
) -> Result<()> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    if mode == LaunchMode::Hd && wiring.data_patches.is_none() {
        bail!(
            "HD launch requires a declared data_patches set (native_letters); declare the operator-confirmed letters, then re-check"
        );
    }
    if mode == LaunchMode::Hd || target == NativeTarget::Game {
        let observations = status(name, instance, dirs)?;
        if mode == LaunchMode::Hd || (target == NativeTarget::Game && wiring.data_patches.is_some()) {
            let missing: Vec<_> = observations
                .iter()
                .filter(|item| {
                    item.name.starts_with("hd-patch") && item.state != ItemState::Verified
                })
                .map(|item| format!("{}={:?}", item.name, item.state))
                .collect();
            if !missing.is_empty() {
                bail!(
                    "HD not ready (restore the operator-confirmed set, then re-check): {}",
                    missing.join(", ")
                );
            }
        }
        if target == NativeTarget::Game {
            let drift: Vec<_> = observations
                .iter()
                .filter(|item| item.name == "wow-exe" && item.state != ItemState::Verified)
                .map(|item| format!("{}={:?}", item.name, item.state))
                .collect();
            if !drift.is_empty() {
                bail!(
                    "client not ready ({}); restore the operator-confirmed WoW.exe or re-pin the declared client digest after confirming provenance",
                    drift.join(", ")
                );
            }
            let files: Vec<_> = observations
                .iter()
                .filter(|item| {
                    item.name.starts_with("client-file:") && item.state != ItemState::Verified
                })
                .map(|item| format!("{}={:?}", item.name, item.state))
                .collect();
            if !files.is_empty() {
                bail!(
                    "client files not ready ({}); restore the operator-confirmed files, then re-check",
                    files.join(", ")
                );
            }
        }
    }
    Ok(())
}

/// Build and run the native command for a resolved target with the
/// target-effective `env` — then wait and propagate the exit status.
/// Game targets use the declared `env`; launcher/installer targets use the
/// launcher-effective `env`. Lutris wine toggles (dxvk/vkd3d/esync/fsync)
/// stay Lutris-scoped and never become native Wine variables.
fn spawn_native(name: &str, _wiring: &Wiring, runner: &Runner, lt: &LaunchTarget) -> Result<()> {
    let mut cmd = Command::new(&runner.path);
    cmd.arg(&lt.exe).current_dir(&lt.dir);
    let (remove, set) = resolve_launch_env_with(&lt.tunings, &lt.arch, &lt.prefix, &lt.dll_overrides);
    for key in remove {
        cmd.env_remove(key);
    }
    for (key, value) in set {
        cmd.env(key, value);
    }
    println!("{name}: launching {} ...", lt.exe.display());
    match cmd
        .status()
        .with_context(|| format!("launch {}", lt.exe.display()))?
    {
        status if status.success() => {
            println!("{name}: process exited {status}");
            Ok(())
        }
        status => bail!("{name}: process exited with {status}"),
    }
}

/// Launch natively without Lutris: the game, the installed maintenance
/// launcher, or the installer as an explicit bootstrap. Reads the recorded
/// runner (prepare first), resolves the target, and execs it with the
/// target-effective `env` (Lutris wine toggles stay Lutris-scoped) — then
/// waits and propagates the exit status.
/// Never installs, updates, reconciles, records, or falls back: a failure
/// surfaces instead of starting something else. Game launches enforce the
/// declared client files, executable identity, and approved HD set; HD mode
/// additionally requires a declared set fully verified.
pub fn launch(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    target: NativeTarget,
    mode: LaunchMode,
) -> Result<()> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    let root_anchor = Anchor::open(&instance.root)?;
    let _lease = root_anchor.lock()?;
    // Quiescence covers every backend at once (game, launcher, Lutris):
    // either side runs alone, never concurrently into one client.
    super::assert_stopped(instance)?;
    let runner = recorded_runner(name, instance)?;
    ensure_launch_readiness(name, instance, dirs, target, mode)?;
    let lt = resolve_launch_target(instance, &wiring, target)?;
    spawn_native(name, &wiring, &runner, &lt)
}

/// The Lutris game entry's launch gate (its synthesized
/// `system.prefix_command`): enforce the same launch readiness a native
/// game launch enforces, then `exec` the appended command unchanged so
/// the game runs exactly as Lutris declared it — same executable, cwd,
/// environment, and stdio; exit status and signals pass through to
/// Lutris. Quiescence cannot apply (Lutris is this process's parent) and
/// no lease is taken: readiness observes client-file state apply never
/// mutates, and a lease FD would be inherited across `exec` onto the
/// game process. Findings therefore gate the Lutris path too: a drifted
/// WoW.exe is never exec'd, native or via Lutris alike.
pub fn gate(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    command: &[std::ffi::OsString],
) -> Result<()> {
    // clap may hand back the `--` separator; it can never be the program.
    let command = if command
        .first()
        .is_some_and(|arg| arg.as_encoded_bytes() == b"--")
    {
        &command[1..]
    } else {
        command
    };
    let (program, args) = command
        .split_first()
        .context("gate: no command after `--`; the Lutris entry prefix_command is misconfigured")?;
    ensure_launch_readiness(
        name,
        instance,
        dirs,
        NativeTarget::Game,
        LaunchMode::Vanilla,
    )?;
    eprintln!(
        "{name}: launch gate passed; exec {}",
        Path::new(program).display()
    );
    use std::os::unix::process::CommandExt;
    let error = Command::new(program).args(args).exec();
    Err(error).with_context(|| format!("exec {}", Path::new(program).display()))
}

/// Render the desktop entry invoking the native game launch. The Lutris
/// entries are untouched; this file is the normal action once native
/// launch is proven, with Lutris kept as fallback. The entry embeds the
/// explicit `--config`: the deployed wrapper replaces argv[0] with the
/// raw binary on exec, so neither argv[0] nor the bare command name would
/// reproduce a working invocation.
fn render_desktop_entry(name: &str, display: &str, exe: &Path, config: &Path) -> Result<String> {
    if name.contains(char::is_whitespace) || name.contains('"') {
        bail!("instance name is not desktop-entry safe: '{name}'");
    }
    Ok(format!(
        "[Desktop Entry]\nType=Application\nVersion=1.0\nName={display}\nComment=Launch {display} natively via modde-manager (Lutris entry stays as fallback).\nExec=\"{}\" --config \"{}\" onboard launch --instance {name} --target game\nTerminal=false\nCategories=Game;\n",
        exe.display(),
        config.display(),
    ))
}

/// Write (or refresh) the desktop entry for native game launch. Self
/// verifying: the file is read back and compared after every write.
pub fn desktop_entry(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    config: &Path,
) -> Result<()> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    let root_anchor = Anchor::open(&instance.root)?;
    let _lease = root_anchor.lock()?;
    let display = wiring
        .lutris
        .as_ref()
        .map(|entry| entry.name.clone())
        .unwrap_or_else(|| name.to_owned());
    let slug = wiring
        .lutris
        .as_ref()
        .map(|entry| entry.slug.clone())
        .unwrap_or_else(|| name.to_owned());
    validate_slug(&slug)?;
    let exe = std::env::current_exe().context("locate the manager binary for the desktop entry")?;
    let body = render_desktop_entry(name, &display, &exe, config)?;
    let dir = dirs.data.join("applications");
    fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    let target = dir.join(format!("{slug}-modde.desktop"));
    let (changed, _) = write_yml(&target, &body)?;
    let actual =
        fs::read_to_string(&target).with_context(|| format!("verify {}", target.display()))?;
    if actual != body {
        bail!("desktop entry failed verification: {}", target.display());
    }
    if changed {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "desktop-entry-done", "path": target.display().to_string()}),
        );
    }
    println!(
        "{name}: desktop entry {}",
        if changed {
            target.display().to_string()
        } else {
            "unchanged".into()
        }
    );
    Ok(())
}

/// Locate the OctoLauncher `settings.json` inside a launcher prefix and
/// return its prefix-relative path: exactly one
/// `drive_c/users/*/AppData/Roaming/octo-launcher/settings.json` must
/// exist. Zero means the launcher never ran (external step, never
/// fabricated); more than one is ambiguous and fails closed. Reads are
/// no-follow throughout, so symlinks fail instead of escaping the prefix.
fn find_launcher_settings(prefix: &Path) -> Result<Option<PathBuf>> {
    let anchor = Anchor::open(prefix)?;
    // Top-level listing only (never a recursive walk: profiles hold
    // shell-folder symlinks like Desktop -> $HOME, and following the
    // `steamuser -> can` alias would count one profile twice).
    let users_dir = fd_path(&anchor.file).join("drive_c/users");
    let mut users = Vec::new();
    match fs::symlink_metadata(&users_dir) {
        Ok(meta) if meta.file_type().is_symlink() => {
            bail!("launcher profile root is a symlink: {}", users_dir.display())
        }
        Ok(meta) if !meta.is_dir() => {
            bail!("launcher prefix has a file where drive_c/users belongs")
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(format!("inspect {}", users_dir.display())),
        Ok(_) => {}
    }
    match fs::read_dir(&users_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry?;
                let name = entry
                    .file_name()
                    .into_string()
                    .map_err(|_| anyhow::anyhow!("non-UTF8 profile name"))?;
                relative(Path::new(&name))?;
                if entry.file_type()?.is_dir() {
                    users.push(name);
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).context(format!("list {}", users_dir.display())),
    }
    let mut found = Vec::new();
    for user in &users {
        let rel = Path::new("drive_c/users")
            .join(user)
            .join("AppData/Roaming/octo-launcher/settings.json");
        match anchor.read(&rel, false)? {
            Image::Missing => {}
            Image::Directory(_) => {
                bail!("launcher settings is a directory: {}", rel.display())
            }
            Image::File(_) => found.push(rel),
        }
    }
    if found.len() > 1 {
        bail!(
            "ambiguous launcher settings ({} candidates); keep exactly one user profile",
            found.len()
        );
    }
    Ok(found.into_iter().next())
}

/// Map a Linux client root to the Wine path the launcher stores: only the
/// default `z: -> /` mapping is accepted, verified from the prefix itself.
/// Any custom drive mapping fails closed rather than guessing letters.
fn wine_client_dir(prefix: &Path, root: &Path) -> Result<String> {
    if !root.is_absolute() {
        bail!("client root must be absolute: {}", root.display());
    }
    let drive = fs::read_link(prefix.join("dosdevices/z:")).context(
        "launcher prefix has no z: drive mapping; cannot derive the launcher client path",
    )?;
    if drive != Path::new("/") {
        bail!(
            "launcher prefix maps z: to {}, not /; cannot derive the launcher client path",
            drive.display()
        );
    }
    Ok(format!(
        "Z:{}",
        root.display().to_string().replace('/', "\\")
    ))
}

/// Read the two client-directory keys the launcher persists. Anything else
/// in the file (sync hashes, window state, mods) is opaque and untouched.
fn read_launcher_client_dirs(body: &str) -> Result<(Option<String>, Option<String>)> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).context("launcher settings is not JSON")?;
    let map = parsed
        .as_object()
        .context("launcher settings is not a JSON object")?;
    let get = |key: &str| -> Result<Option<String>> {
        match map.get(key) {
            None => Ok(None),
            Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
            Some(_) => bail!("launcher settings key '{key}' is not a string"),
        }
    };
    Ok((get("clientDir")?, get("activeClientDir")?))
}

/// Splice a new value into a top-level string literal, preserving every
/// other byte. A full JSON rewrite would reorder keys and reformat
/// launcher-owned state; this touches only the located literal.
fn splice_json_string(
    out: &mut String,
    span: std::ops::Range<usize>,
    want: &str,
    changed: &mut bool,
) -> Result<()> {
    // The span covers the inner literal; the serialized replacement
    // carries its own quotes, so splice the inner text only.
    let replacement = serde_json::to_string(want)?;
    let inner = replacement
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .context("serialized client path is not a JSON string")?;
    if &out[span.clone()] != inner {
        out.replace_range(span, inner);
        *changed = true;
    }
    Ok(())
}

/// Byte span of the inner text of a top-level string member in a JSON
/// object document. Tracks nesting and string escapes structurally, so a
/// same-named key nested inside (e.g. under `mods`) is never mistaken for
/// the top-level setting. Duplicate top-level keys, non-object documents,
/// and malformed input fail closed instead of guessing.
fn json_top_level_string_span(body: &str, key: &str) -> Result<Option<std::ops::Range<usize>>> {
    let bytes = body.as_bytes();
    // Parse a string literal at `pos` (pointing at the opening quote);
    // returns the inner span and the position past the closing quote.
    fn string_at(bytes: &[u8], mut pos: usize) -> Result<(std::ops::Range<usize>, usize)> {
        pos += 1;
        let start = pos;
        while pos < bytes.len() {
            match bytes[pos] {
                b'\\' => {
                    pos += 1;
                    if pos >= bytes.len() {
                        break;
                    }
                    if bytes[pos] == b'u' {
                        pos += 1;
                        for _ in 0..4 {
                            if pos < bytes.len() && bytes[pos].is_ascii_hexdigit() {
                                pos += 1;
                            } else {
                                bail!("launcher settings has a malformed string escape");
                            }
                        }
                    } else {
                        pos += 1;
                    }
                }
                b'"' => return Ok((start..pos, pos + 1)),
                _ => pos += 1,
            }
        }
        bail!("launcher settings has an unterminated string")
    }
    // Skip one balanced value starting at `pos`; returns the position past it.
    fn skip_value(bytes: &[u8], mut pos: usize) -> Result<usize> {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            bail!("launcher settings ends mid-value");
        }
        match bytes[pos] {
            b'"' => Ok(string_at(bytes, pos)?.1),
            b'{' | b'[' => {
                let mut depth = 0usize;
                while pos < bytes.len() {
                    match bytes[pos] {
                        b'"' => pos = string_at(bytes, pos)?.1,
                        b'{' | b'[' => {
                            depth += 1;
                            pos += 1;
                        }
                        b'}' | b']' => {
                            depth -= 1;
                            pos += 1;
                            if depth == 0 {
                                return Ok(pos);
                            }
                        }
                        _ => pos += 1,
                    }
                }
                bail!("launcher settings has unbalanced brackets")
            }
            _ => {
                while pos < bytes.len() && !matches!(bytes[pos], b',' | b'}' | b']') {
                    pos += 1;
                }
                Ok(pos)
            }
        }
    }
    let mut pos = 0;
    while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
        pos += 1;
    }
    if bytes.get(pos) != Some(&b'{') {
        bail!("launcher settings is not a JSON object");
    }
    pos += 1;
    let mut found: Option<std::ops::Range<usize>> = None;
    loop {
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= bytes.len() {
            bail!("launcher settings ends inside its top-level object");
        }
        if bytes[pos] == b'}' {
            pos += 1;
            while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                pos += 1;
            }
            if pos != bytes.len() {
                bail!("launcher settings has trailing data");
            }
            return Ok(found);
        }
        if bytes[pos] != b'"' {
            bail!("launcher settings has a malformed member");
        }
        let (name_span, after_key) = string_at(bytes, pos)?;
        let name = &body[name_span];
        pos = after_key;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if bytes.get(pos) != Some(&b':') {
            bail!("launcher settings has a malformed member");
        }
        pos += 1;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if name == key {
            if found.is_some() {
                bail!("launcher settings has a duplicate top-level key '{key}'");
            }
            if pos >= bytes.len() || bytes[pos] != b'"' {
                bail!("launcher settings key '{key}' is not a string");
            }
            let (span, after) = string_at(bytes, pos)?;
            found = Some(span);
            pos = after;
        } else {
            pos = skip_value(bytes, pos)?;
        }
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos < bytes.len() && bytes[pos] == b',' {
            pos += 1;
        } else if pos >= bytes.len() || bytes[pos] != b'}' {
            bail!("launcher settings has a malformed member separator");
        }
    }
}

/// Reconcile the two client-directory keys with the declared root.
/// `clientDir` always follows the root. `activeClientDir` follows only
/// when it was tracking the previous selection: the installed bundle
/// writes both on folder selection but treats a mismatch as stale sync
/// context, so a foreign value means another folder's sync state and is
/// left alone (reported, never reassigned). Returns the edited body,
/// whether it changed, and a note when the active key was deferred.
fn set_launcher_client_dirs(
    body: &str,
    want: &str,
    client_old: &str,
    active_old: Option<&str>,
) -> Result<(String, bool, Option<String>)> {
    let mut out = body.to_owned();
    let mut changed = false;
    let Some(span) = json_top_level_string_span(&out, "clientDir")? else {
        bail!("launcher settings has no 'clientDir'; set the client folder in the launcher once")
    };
    splice_json_string(&mut out, span, want, &mut changed)?;
    let mut note = None;
    match active_old {
        // Tracking the selection (or already correct): follow it.
        Some(active) if active == client_old || active == want => {
            if let Some(span) = json_top_level_string_span(&out, "activeClientDir")? {
                splice_json_string(&mut out, span, want, &mut changed)?;
            }
        }
        Some(active) => {
            note = Some(format!(
                "activeClientDir tracks '{active}', not the declared client"
            ));
        }
        None => {}
    }
    Ok((out, changed, note))
}

/// Reconcile the launcher's stored client folder with the declared client
/// root. Returns the settings path, whether it changed, and a note when
/// the active key was deliberately left alone — or None when there are no
/// settings to reconcile (the launcher writes them on first run; status
/// tracks that absence separately). Never creates settings, never touches
/// sync hashes or anything else in the file; the previous content is
/// retained under a unique backup on change. The read, backup, and commit
/// all go through the anchored prefix with no-follow opens, and the parent
/// is revalidated before the commit lands.
fn reconcile_launcher_client_dir(
    instance: &Instance,
    prefix: &Path,
) -> Result<Option<(PathBuf, bool, Option<String>)>> {
    let anchor = Anchor::open(prefix)?;
    let Some(rel) = find_launcher_settings(prefix)? else {
        return Ok(None);
    };
    let (parent, target) = anchor.target(&rel, false, &mut Vec::new())?;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&target)
        .with_context(|| format!("read {}", target.display()))?;
    if !file.metadata()?.is_file() {
        bail!("launcher settings changed type during read");
    }
    let mut current = Vec::new();
    std::io::Read::read_to_end(&mut file, &mut current)?;
    drop(file);
    let body =
        String::from_utf8(current.clone()).context("launcher settings is not UTF-8")?;
    let (client_dir, active_dir) = read_launcher_client_dirs(&body)?;
    let want = wine_client_dir(prefix, &instance.root)?;
    let Some(client_old) = client_dir.as_deref() else {
        bail!("launcher settings has no 'clientDir'; set the client folder in the launcher once")
    };
    if client_old == want && active_dir.as_deref() == Some(want.as_str()) {
        return Ok(Some((prefix.join(&rel), false, None)));
    }
    let (updated, changed, note) =
        set_launcher_client_dirs(&body, &want, client_old, active_dir.as_deref())?;
    if !changed {
        return Ok(Some((prefix.join(&rel), false, note)));
    }
    let backup = backup_name(&target, "launcher-settings");
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut backup_file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&backup)?;
        use std::io::Write;
        backup_file.write_all(&current)?;
        backup_file.sync_all()?;
    }
    anchor.revalidate_parent(&rel, &parent)?;
    atomic_write_0600(&target, updated.as_bytes())?;
    Ok(Some((prefix.join(&rel), true, note)))
}

/// Read-only observation of the launcher's stored client folder against the
/// declared client root. Never writes; register-launcher owns the fix.
fn launcher_client_dir_item(instance: &Instance, wiring: &Wiring, prefix: &Path) -> StatusItem {
    match validated_prefix(prefix, &instance.root, wiring) {
        Err(e) => item(
            "launcher-client-dir",
            ItemState::Unverifiable,
            format!("{e:#}"),
            "fix the launcher prefix declaration".into(),
        ),
        Ok(None) => item(
            "launcher-client-dir",
            ItemState::Missing,
            "launcher prefix absent".into(),
            "run: onboard register-launcher".into(),
        ),
        Ok(Some(_)) => match find_launcher_settings(prefix) {
            Err(e) => item(
                "launcher-client-dir",
                ItemState::Unverifiable,
                format!("{e:#}"),
                "keep exactly one launcher user profile".into(),
            ),
            Ok(None) => item(
                "launcher-client-dir",
                ItemState::Missing,
                "launcher settings absent".into(),
                "run the launcher once, then run: onboard register-launcher".into(),
            ),
            Ok(Some(rel)) => {
                let path = prefix.join(&rel);
                let body = match fs::read_to_string(&path) {
                    Ok(body) => body,
                    Err(e) => {
                        return item(
                            "launcher-client-dir",
                            ItemState::Unverifiable,
                            format!("{e}"),
                            "inspect the launcher settings permissions".into(),
                        );
                    }
                };
                let want = match wine_client_dir(prefix, &instance.root) {
                    Ok(want) => want,
                    Err(e) => {
                        return item(
                            "launcher-client-dir",
                            ItemState::Unverifiable,
                            format!("{e:#}"),
                            "restore the default z: drive mapping".into(),
                        );
                    }
                };
                match read_launcher_client_dirs(&body) {
                    Err(e) => item(
                        "launcher-client-dir",
                        ItemState::Mismatched,
                        format!("{e:#}"),
                        "set the client folder in the launcher once, then re-run registration"
                            .into(),
                    ),
                    Ok((client_dir, active_dir))
                        if client_dir.as_deref() == Some(want.as_str())
                            && active_dir.as_deref() == Some(want.as_str()) =>
                    {
                        item(
                            "launcher-client-dir",
                            ItemState::Verified,
                            path.display().to_string(),
                            String::new(),
                        )
                    }
                    Ok((client_dir, active_dir)) if client_dir.as_deref() == Some(want.as_str()) => {
                        item(
                            "launcher-client-dir",
                            ItemState::Mismatched,
                            match active_dir.as_deref() {
                                Some(active) => format!(
                                    "activeClientDir tracks '{active}', not the declared client"
                                ),
                                None => "activeClientDir absent".into(),
                            },
                            "resolve the foreign sync in the launcher, then run: onboard register-launcher"
                                .into(),
                        )
                    }
                    Ok((client_dir, _)) => item(
                        "launcher-client-dir",
                        ItemState::Mismatched,
                        format!(
                            "clientDir is {}, want {want}",
                            client_dir.as_deref().unwrap_or("<absent>")
                        ),
                        "run: onboard register-launcher".into(),
                    ),
                }
            }
        },
    }
}

/// Register the launcher itself as a Lutris entry (the installer first,
/// the installed executable once declared): its own sibling prefix, one
/// yml row, and the launcher's stored client folder (reconciled to the
/// declared client root, nothing else in its settings), through the same
/// plan/execute adapter as game entries. Single call; never launches
/// anything — the operator runs the installer from Lutris afterwards.
pub fn register_launcher(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    adopt: bool,
) -> Result<()> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    let root_anchor = Anchor::open(&instance.root)?;
    let _lease = root_anchor.lock()?;
    super::assert_stopped(instance)?;
    let launcher = wiring
        .launcher
        .clone()
        .context("no launcher block declared")?;
    let installer = launcher
        .installer
        .clone()
        .context("no installer declared in launcher block")?;
    let prefix = launcher
        .prefix
        .clone()
        .context("no launcher prefix declared")?;
    let entry = launcher
        .lutris
        .clone()
        .context("no launcher lutris entry declared")?;
    if !installer.path.is_absolute() {
        bail!("installer path must be absolute");
    }
    validate_descriptor_file(&installer.path, installer.sha256.as_deref(), "installer")?;
    // Installed launcher executable: same single-descriptor discipline as
    // the installer, and it must live under the launcher prefix (never an
    // arbitrary host executable an entry would then run).
    if let Some(executable) = &launcher.executable {
        validate_launcher_executable(executable, &prefix)?;
    }
    validate_slug(&entry.slug)?;
    if lutris_running()? {
        bail!("lutris is running; close it completely before registering");
    }
    let (runner, _) = select_runner(dirs, instance, &wiring, false)
        .context("cannot register without a resolved runner")?;

    // Path validation first (no writes): a bad prefix never gets created
    // on the way to an ownership refusal.
    validated_prefix(&prefix, &instance.root, &wiring)?;
    let spec = launcher_spec(
        &installer,
        &prefix,
        &entry,
        &runner,
        &wiring,
        launcher.executable.as_ref(),
    )?;
    // Same lock-before-inspection discipline as game registration: the
    // ownership check inside plan_entry runs under the data-dir lock.
    let (_, site_data) =
        lutris_site(dirs).context("no pga.db found (start Lutris once, then close it)")?;
    let _db_guard = Anchor::open(&site_data)
        .context("invalid Lutris data dir")
        .and_then(|anchor| {
            anchor
                .lock()
                .context("another onboard run holds the Lutris database")
        })?;
    let entry_plan = plan_entry(dirs, &entry, &spec, adopt)?;

    let mut mutated = false;
    // A prefix created by this run cannot hold launcher settings yet (the
    // launcher writes them on first run): reconciliation only applies to a
    // pre-existing prefix, and fails closed there when settings are absent.
    let prior = validated_prefix(&prefix, &instance.root, &wiring)?;
    // Verification below only covers settings when reconciliation ran
    // (pre-existing prefix); a fresh prefix cannot hold settings yet.
    let mut settings_reconciled = false;
    if prior.is_none() {
        match create_prefix(&prefix, &runner, &wiring) {
            Ok(arch) => {
                mutated = true;
                println!("{name}: launcher prefix created ({arch})");
            }
            Err(error) => {
                journal_event(
                    instance,
                    name,
                    serde_json::json!({"op": "register-launcher-failed", "step": "prefix", "error": format!("{error:#}")}),
                );
                return Err(error);
            }
        }
        println!("{name}: launcher client folder pending (run the launcher once)");
    } else {
        println!(
            "{name}: launcher prefix already present ({})",
            prior.as_deref().unwrap_or("unknown arch")
        );
        // The launcher's stored client folder follows the declared client
        // root. Absent settings mean the launcher never ran: status tracks
        // that, so registration proceeds with the entry. The launcher must
        // be stopped (assert_stopped above): it rewrites its settings on
        // exit and would clobber this edit.
        match reconcile_launcher_client_dir(instance, &prefix) {
            Ok(None) => println!("{name}: launcher client folder pending (run the launcher once)"),
            Ok(Some((path, changed, note))) => {
                mutated |= changed;
                settings_reconciled = true;
                println!(
                    "{name}: launcher client folder {}",
                    if changed {
                        path.display().to_string()
                    } else {
                        "unchanged".into()
                    }
                );
                if let Some(note) = note {
                    println!("{name}: launcher client folder note: {note}");
                    journal_event(
                        instance,
                        name,
                        serde_json::json!({"op": "register-launcher-note", "step": "launcher-settings", "note": note}),
                    );
                }
            }
            Err(error) => {
                journal_event(
                    instance,
                    name,
                    serde_json::json!({"op": "register-launcher-failed", "step": "launcher-settings", "error": format!("{error:#}")}),
                );
                return Err(error);
            }
        }
    }

    match execute_entry(dirs, &entry, &spec, &entry_plan) {
        Ok((changed, action)) => {
            mutated |= changed || action != "unchanged";
            println!("{name}: launcher entry {action}");
        }
        Err(error) => {
            journal_event(
                instance,
                name,
                serde_json::json!({"op": "register-launcher-failed", "step": "lutris-db", "error": format!("{error:#}")}),
            );
            return Err(error);
        }
    }

    if record_runner(instance, &runner)? {
        mutated = true;
        println!("{name}: runtime selection recorded");
    }
    // Verify the effective entry, not just the write calls.
    let recorded = read_recorded(instance).map(|record| record.map(|record| record.version));
    match lutris_yml_path(dirs, &entry.slug) {
        Some(path) => {
            let checked = check_entry_yml(
                &path,
                "launcher-entry",
                "run: onboard register-launcher",
                &spec,
                recorded,
            );
            if checked.state != ItemState::Verified {
                journal_event(
                    instance,
                    name,
                    serde_json::json!({"op": "register-launcher-failed", "step": "verify"}),
                );
                bail!("launcher entry failed verification: {}", checked.detail);
            }
        }
        None => bail!("launcher entry missing after registration"),
    }
    if settings_reconciled
        && launcher_client_dir_item(instance, &wiring, &prefix).state != ItemState::Verified
    {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "register-launcher-failed", "step": "verify-settings"}),
        );
        bail!("launcher client folder failed verification");
    }
    if mutated {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "register-launcher-done", "runner": runner.version}),
        );
    }
    println!("{name}: launcher registered");
    Ok(())
}

/// Installed launcher bundle completeness: the executable plus the payload
/// files the NSIS `app-64.7z` always lays down. Locale packs are upstream-
/// empty (verified against the installer payload), so they are not required.
/// Regular files only; symlinks fail closed like the executable check.
fn launcher_bundle_complete(executable: &Path) -> bool {
    let dir = match executable.parent() {
        Some(dir) => dir,
        None => return false,
    };
    for path in [
        executable.to_path_buf(),
        dir.join("resources/app.asar"),
        dir.join("resources/app-update.yml"),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {}
            _ => return false,
        }
    }
    true
}

/// Install the launcher into its declared persistent prefix via the reviewed
/// installer, unattended (`/S`). Reuses runner selection, prefix creation,
/// locks, and journaling from the onboard flow. Idempotent: a complete
/// bundle is a no-op unless `force` passes. Never touches the game client,
/// addons, Lutris entries, or launcher settings — run register-launcher
/// afterwards to reconcile the entry and client folder.
pub fn install_launcher(
    name: &str,
    instance: &Instance,
    dirs: &HomeDirs,
    force: bool,
) -> Result<()> {
    validate_declaration(name, instance)?;
    let wiring = resolve_wiring(instance)?;
    let root_anchor = Anchor::open(&instance.root)?;
    let _lease = root_anchor.lock()?;
    super::assert_stopped(instance)?;
    let launcher = wiring
        .launcher
        .clone()
        .context("no launcher block declared")?;
    let installer = launcher
        .installer
        .clone()
        .context("no installer declared in launcher block")?;
    let prefix = launcher
        .prefix
        .clone()
        .context("no launcher prefix declared")?;
    let executable = launcher
        .executable
        .clone()
        .context("no installed launcher executable declared; declare it before installing")?;
    if !installer.path.is_absolute() {
        bail!("installer path must be absolute");
    }
    validate_descriptor_file(&installer.path, installer.sha256.as_deref(), "installer")?;
    match validate_launcher_executable(&executable, &prefix) {
        Ok(()) => {
            if launcher_bundle_complete(&executable) && !force {
                println!("{name}: launcher already installed");
                return Ok(());
            }
        }
        Err(error) => {
            // Missing executable proceeds to install; a present-but-invalid
            // path (symlink, outside prefix, wrong type) still fails closed.
            match fs::symlink_metadata(&executable) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(error),
            }
        }
    }
    let (runner, _) = select_runner(dirs, instance, &wiring, false)
        .context("cannot install without a resolved runner")?;
    if validated_prefix(&prefix, &instance.root, &wiring)?.is_none() {
        match create_prefix(&prefix, &runner, &wiring) {
            Ok(arch) => println!("{name}: launcher prefix created ({arch})"),
            Err(error) => {
                journal_event(
                    instance,
                    name,
                    serde_json::json!({"op": "install-launcher-failed", "step": "prefix", "error": format!("{error:#}")}),
                );
                return Err(error);
            }
        }
    }
    let mut cmd = Command::new(&runner.path);
    cmd.arg(&installer.path).arg("/S");
    cmd.current_dir(
        installer
            .path
            .parent()
            .context("installer needs a parent directory")?,
    );
    let (remove, set) = resolve_launcher_env(&wiring, &prefix, &[]);
    for key in remove {
        cmd.env_remove(key);
    }
    for (key, value) in set {
        cmd.env(key, value);
    }
    println!("{name}: installing launcher (silent) ...");
    let status = cmd.status().context("run launcher installer")?;
    if !status.success() {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "install-launcher-failed", "step": "installer", "status": format!("{status}")}),
        );
        bail!("launcher installer failed: {status}");
    }
    let mut wait = Command::new(wineserver_bin(&runner));
    wait.arg("-w");
    let (remove, set) = resolve_launcher_env(&wiring, &prefix, &[]);
    for key in remove {
        wait.env_remove(key);
    }
    for (key, value) in set {
        wait.env(key, value);
    }
    let status = wait.status().context("wait for wineserver")?;
    if !status.success() {
        bail!("wineserver -w failed: {status}");
    }
    if !launcher_bundle_complete(&executable) {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "install-launcher-failed", "step": "verify"}),
        );
        bail!(
            "launcher bundle incomplete after install: {}",
            executable.display()
        );
    }
    if record_runner(instance, &runner)? {
        println!("{name}: runtime selection recorded");
    }
    journal_event(
        instance,
        name,
        serde_json::json!({"op": "install-launcher-done", "runner": runner.version}),
    );
    println!("{name}: launcher installed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal MZ + PE\0\0 + COFF header; `laa` controls the 0x20 bit.
    fn fake_exe(laa: bool) -> Vec<u8> {
        let mut bytes = vec![0u8; 0x80];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x40u32.to_le_bytes());
        bytes[0x40..0x44].copy_from_slice(b"PE\0\0");
        bytes[0x44..0x46].copy_from_slice(&0x14cu16.to_le_bytes()); // i386
        let flags: u16 = if laa { 0x012f } else { 0x010f };
        bytes[0x56..0x58].copy_from_slice(&flags.to_le_bytes()); // characteristics
        bytes
    }

    #[test]
    fn pe_flags_detect_laa() {
        assert_eq!(pe_exe_flags(&fake_exe(true)).unwrap(), (true, true));
        assert_eq!(pe_exe_flags(&fake_exe(false)).unwrap(), (true, false));
        assert!(pe_exe_flags(b"not an exe").is_err());
        assert!(pe_exe_flags(&[0u8; 10]).is_err());
    }

    #[test]
    fn prefix_arch_markers() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(prefix_arch(dir.path()).unwrap(), None);
        fs::write(
            dir.path().join("system.reg"),
            "#arch=win64\nWINE REGISTRY\n",
        )
        .unwrap();
        assert_eq!(prefix_arch(dir.path()).unwrap(), Some("win64".into()));
        fs::write(dir.path().join("system.reg"), "no marker\n").unwrap();
        assert!(prefix_arch(dir.path()).is_err());
    }

    fn fixture_home(runners: &[&str]) -> tempfile::TempDir {
        let home = tempfile::tempdir().unwrap();
        for runner in runners {
            let bin = home
                .path()
                .join(format!(".local/share/lutris/runners/wine/{runner}/bin"));
            fs::create_dir_all(&bin).unwrap();
            fs::write(bin.join("wine"), "#!/bin/sh\nexit 0\n").unwrap();
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(bin.join("wine"), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        home
    }

    fn fixture_instance(root: &Path) -> Instance {
        let state = root.parent().unwrap().join("octo-manager");
        serde_json::from_value(serde_json::json!({
            "root": root,
            "client": "wow-classic",
            "state_dir": state,
            "wiring": {
                "runtime": {"kind": "wine", "version": "latest", "arch": "wow64", "anticheat": false},
                "tunings": {"dxvk": true, "vkd3d": false, "esync": true, "fsync": true,
                    "env": {"WINEDEBUG": "-all"}},
                "dll_overrides": ["d3d9=n,b"],
                "launch": {"executable": "VanillaFixes.exe"},
                "prefix": {"path": root.parent().unwrap().join("octowow-prefix")},
                "lutris": {"slug": "octowow-test", "name": "OctoWoW"},
                "client_integrity": {"require_files": ["VanillaFixes.exe", "d3d9.dll"]},
                "data_patches": {"native_letters": ["A"], "forbid_renames": true},
                "endpoints": {"assert_realmlist": ["set realmlist 185.165.170.6"]},
            }
        }))
        .unwrap()
    }

    /// Fully satisfiable instance for apply tests: dxvk off (d3d9.dll is
    /// bundled), matching realmlist, native patch-A present.
    fn apply_fixture(root: &Path, home: &Path) -> Instance {
        let _ = home;
        let state = root.parent().unwrap().join("octo-manager");
        fs::write(root.join("VanillaFixes.exe"), "fake").unwrap();
        fs::write(root.join("d3d9.dll"), "fake").unwrap();
        fs::write(
            root.join("realmlist.wtf"),
            "set realmlist 185.165.170.6\nset patchlist 185.165.170.6\n",
        )
        .unwrap();
        fs::write(root.join("Data/patch-A.mpq"), "hd-bytes").unwrap();
        serde_json::from_value(serde_json::json!({
            "root": root,
            "client": "wow-classic",
            "state_dir": state,
            "wiring": {
                "runtime": {"kind": "wine", "version": "latest", "arch": "wow64", "anticheat": false},
                "tunings": {"dxvk": false, "vkd3d": false, "esync": true, "fsync": true,
                    "env": {"WINEDEBUG": "-all"}},
                "dll_overrides": ["d3d9=n,b"],
                "launch": {"executable": "VanillaFixes.exe"},
                "prefix": {"path": root.parent().unwrap().join("octowow-prefix")},
                "lutris": {"slug": "octowow-test", "name": "OctoWoW"},
                "client_integrity": {"require_files": ["VanillaFixes.exe", "d3d9.dll"]},
                "data_patches": {"native_letters": ["A"], "forbid_renames": true},
                "endpoints": {"assert_realmlist": [
                    "set realmlist 185.165.170.6", "set patchlist 185.165.170.6"]},
            }
        }))
        .unwrap()
    }

    /// Per-test envelope: the game lives at `<envelope>/game`, so the
    /// derived prefix/state sibling paths are unique per test. (Deriving
    /// them from a bare tempdir's parent shares $TMPDIR across parallel
    /// tests and races.)
    fn fixture_root() -> (tempfile::TempDir, PathBuf) {
        let envelope = tempfile::tempdir().unwrap();
        let game = envelope.path().join("game");
        fs::create_dir_all(game.join("Data")).unwrap();
        fs::create_dir_all(game.join("Interface/AddOns")).unwrap();
        // realmlist deliberately wrong to exercise the mismatched path.
        fs::write(
            game.join("realmlist.wtf"),
            "set realmlist \"play.octowow.st\"\n",
        )
        .unwrap();
        (envelope, game)
    }

    #[test]
    fn runner_version_keys_ignore_name_prefixes() {
        assert_eq!(major_version("wine-ge-9-2"), Some(9));
        assert_eq!(major_version("wine-7-0"), Some(7));
        assert_eq!(major_version("wine"), None);
        assert!(version_key("wine-ge-9-10") > version_key("wine-ge-9-2"));
    }

    #[test]
    fn runner_discovery_picks_latest_numerically_and_rejects_old() {
        let home = fixture_home(&["wine-ge-8-1", "wine-ge-9-2", "wine-ge-9-10", "wine-7-0"]);
        let found = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        assert_eq!(found.version, "wine-ge-9-10");
        assert!(discover_runner(&dirs(&home), "wine", "wine-7-0").is_err());
        assert!(discover_runner(&dirs(&home), "proton", "latest").is_err());
        let empty = tempfile::tempdir().unwrap();
        assert!(discover_runner(&dirs(&empty), "wine", "latest").is_err());
    }

    #[test]
    fn preset_expansion_is_single_path_with_explicit_replace_rules() {
        // Preset-only declaration resolves to reusable Octo defaults. The HD
        // patch set stays consumer-owned: no implicit approval.
        let preset_only: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
        }))
        .unwrap();
        let wiring = resolve_wiring(&preset_only).unwrap();
        assert_eq!(wiring.launch.executable, "VanillaFixes.exe");
        assert!(!wiring.tunings.dxvk); // bundled d3d9.dll, no second layer
        assert!(!wiring.runtime.anticheat);
        assert_eq!(wiring.lutris.as_ref().unwrap().slug, "octowow-community");
        assert!(wiring.data_patches.is_none());
        assert!(wiring.prefix.is_none()); // site path stays consumer-owned
        assert!(wiring.endpoints.is_none());
        // An explicit consumer set survives preset expansion.
        let declared: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
            "wiring": {"data_patches": {"native_letters": ["B", "U"], "forbid_renames": true}},
        }))
        .unwrap();
        let with_hd = resolve_wiring(&declared).unwrap();
        assert!(with_hd.data_patches.as_ref().unwrap().native_letters.contains(&"U".to_string()));
        // User lists replace wholesale; env merges with user winning.
        let custom: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
            "wiring": {
                "dll_overrides": ["d3d9=n,b", "winmm=n,b"],
                "tunings": {"env": {"WINEDEBUG": "+fps", "MESA_GLTHREAD": "true"}},
            },
        }))
        .unwrap();
        let merged = resolve_wiring(&custom).unwrap();
        assert_eq!(merged.dll_overrides, vec!["d3d9=n,b", "winmm=n,b"]);
        assert_eq!(merged.tunings.env.get("WINEDEBUG").unwrap(), "+fps");
        assert!(!merged.tunings.dxvk); // untouched preset scalar survives
        // Unknown presets and empty declarations fail, never synthesize.
        let bad: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "nope",
        }))
        .unwrap();
        assert!(resolve_wiring(&bad).is_err());
        let empty: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic",
        }))
        .unwrap();
        assert!(resolve_wiring(&empty).is_err());
    }

    #[test]
    fn status_reports_blockers_read_only() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let before_root: Vec<_> = walk_paths(&root);
        let before_home: Vec<_> = walk_paths(home.path());
        let items = status("test", &instance, &dirs(&home)).unwrap();
        // Neither game nor Lutris state may change under read-only status.
        assert_eq!(walk_paths(&root), before_root);
        assert_eq!(walk_paths(home.path()), before_home);
        let state = |name: &str| items.iter().find(|i| i.name == name).unwrap().state;
        assert_eq!(state("runner"), ItemState::Verified);
        assert_eq!(state("runner-pinned"), ItemState::Verified);
        assert_eq!(state("client-file:VanillaFixes.exe"), ItemState::Missing);
        assert_eq!(state("hd-patch:A"), ItemState::Missing);
        assert_eq!(state("endpoints"), ItemState::Mismatched);
        assert_eq!(state("prefix"), ItemState::Missing);
        assert_eq!(state("lutris-yml"), ItemState::Missing);
        assert_eq!(state("lutris-db"), ItemState::Missing);
        // plan mirrors every non-verified item, still read-only.
        let changes = plan("test", &instance, &dirs(&home)).unwrap();
        assert!(!changes.is_empty());
        assert_eq!(walk_paths(&root), before_root);
        assert_eq!(walk_paths(home.path()), before_home);
    }

    #[test]
    fn endpoints_item_reports_verified_mismatched_and_missing() {
        let (_envelope, root) = fixture_root();
        let realmlist = root.join("realmlist.wtf");
        let before = snapshot_tree(&root);

        // Matching assertions verify.
        let endpoints = Endpoints {
            assert_realmlist: vec!["set realmlist \"play.octowow.st\"".into()],
        };
        let found = endpoints_item(&root, &endpoints);
        assert_eq!(found.name, "endpoints");
        assert_eq!(found.state, ItemState::Verified);
        assert_eq!(found.detail, "realmlist matches");
        assert!(found.fix.is_empty());

        // Multiple gaps report in declaration order.
        let endpoints = Endpoints {
            assert_realmlist: vec![
                "set realmlist 1.2.3.4".into(),
                "set realmlist \"play.octowow.st\"".into(),
                "set patchlist 5.6.7.8".into(),
            ],
        };
        let found = endpoints_item(&root, &endpoints);
        assert_eq!(found.name, "endpoints");
        assert_eq!(found.state, ItemState::Mismatched);
        assert_eq!(
            found.detail,
            "missing lines: set realmlist 1.2.3.4, set patchlist 5.6.7.8"
        );
        assert_eq!(
            found.fix,
            "update realmlist.wtf out-of-band (unmanaged file)"
        );

        // No assertions is vacuously verified.
        let endpoints = Endpoints {
            assert_realmlist: Vec::new(),
        };
        let found = endpoints_item(&root, &endpoints);
        assert_eq!(found.name, "endpoints");
        assert_eq!(found.state, ItemState::Verified);
        assert_eq!(found.detail, "realmlist matches");
        assert!(found.fix.is_empty());

        // The inspection produces no observable state change: identity,
        // nanosecond timestamps, and bytes all match afterwards.
        assert_eq!(snapshot_tree(&root), before);

        // Absent realmlist is missing, never an error.
        fs::remove_file(&realmlist).unwrap();
        let endpoints = Endpoints {
            assert_realmlist: vec!["set realmlist 1.2.3.4".into()],
        };
        let found = endpoints_item(&root, &endpoints);
        assert_eq!(found.name, "endpoints");
        assert_eq!(found.state, ItemState::Missing);
        assert_eq!(found.detail, "missing client file: realmlist.wtf");
        assert_eq!(found.fix, "restore realmlist.wtf from backup");
        assert!(!realmlist.exists());
    }

    #[test]
    fn rename_dodge_and_dxvk_double_layer_are_mismatches() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        // patch-F.mpq is outside native letters ["A"]: a rename dodge.
        fs::write(root.join("Data/patch-F.mpq"), "renamed").unwrap();
        // d3d9.dll bundled while Lutris-managed DXVK is on: two layers.
        fs::write(root.join("d3d9.dll"), "bundled").unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let state = |name: &str| items.iter().find(|i| i.name == name).unwrap().state;
        assert_eq!(state("hd-patch-letters"), ItemState::Mismatched);
        assert_eq!(state("dxvk"), ItemState::Mismatched);
    }

    #[test]
    fn yml_render_carries_prefix_runtime_overrides_and_anticheat_off() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let wiring = resolve_wiring(&instance).unwrap();
        let runner = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let yml = render_lutris_yml(
            "test",
            &instance,
            wiring.lutris.as_ref().unwrap(),
            &runner,
            &wiring,
        )
        .unwrap();
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        assert_eq!(parsed["runner"].as_str(), Some("wine"));
        assert_eq!(parsed["wine"]["version"].as_str(), Some("wine-ge-9-2"));
        assert_eq!(parsed["wine"]["arch"].as_str(), Some("win64")); // wow64 mapping
        assert_eq!(parsed["wine"]["vkd3d"].as_bool(), Some(false));
        assert_eq!(parsed["wine"]["eac"].as_bool(), Some(false));
        assert_eq!(parsed["wine"]["battleye"].as_bool(), Some(false));
        // Lutris-managed runner: the Lutris runtime stays enabled.
        assert_eq!(parsed["system"]["disable_runtime"].as_bool(), Some(false));
        let prefix = wiring.prefix.as_ref().unwrap().path.display().to_string();
        assert_eq!(parsed["game"]["prefix"].as_str(), Some(prefix.as_str()));
        assert!(
            parsed["system"]["env"]["WINEDLLOVERRIDES"]
                .as_str()
                .unwrap()
                .contains("d3d9=n,b")
        );
        // The effective entry verifies against the same declaration.
        let dir = home.path().join(".config/lutris/games");
        fs::create_dir_all(&dir).unwrap();
        let yml_path = dir.join("octowow-test.yml");
        fs::write(&yml_path, &yml).unwrap();
        let checked = check_lutris_yml(
            "test",
            &yml_path,
            &instance,
            &wiring,
            wiring.lutris.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(checked.state, ItemState::Verified);
    }

    #[test]
    fn system_runner_disables_lutris_runtime_and_check_enforces_it() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let wiring = resolve_wiring(&instance).unwrap();
        let entry = wiring.lutris.as_ref().unwrap();
        let system = Runner {
            path: PathBuf::from("/usr/bin/wine"),
            version: "system".into(),
        };
        let yml = render_lutris_yml("test", &instance, entry, &system, &wiring).unwrap();
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        assert_eq!(parsed["wine"]["version"].as_str(), Some("system"));
        assert_eq!(parsed["system"]["disable_runtime"].as_bool(), Some(true));
        // Recorded system selection: the rendered entry verifies.
        let state = _envelope.path().join("octo-manager");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("wiring-runtime.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": "system",
                "path": "/usr/bin/wine",
            }))
            .unwrap(),
        )
        .unwrap();
        let dir = home.path().join(".config/lutris/games");
        fs::create_dir_all(&dir).unwrap();
        let yml_path = dir.join("octowow-test.yml");
        fs::write(&yml_path, &yml).unwrap();
        let checked = check_lutris_yml("test", &yml_path, &instance, &wiring, entry).unwrap();
        assert_eq!(checked.state, ItemState::Verified);
        // Missing key (pre-fix yml): mismatched, not silently accepted.
        let without: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        let mut map = without.as_mapping().unwrap().clone();
        let system_key = serde_yaml_ng::Value::String("system".into());
        let mut system_map = map
            .remove(&system_key)
            .unwrap()
            .as_mapping()
            .unwrap()
            .clone();
        system_map.remove(&serde_yaml_ng::Value::String("disable_runtime".into()));
        map.insert(system_key, serde_yaml_ng::Value::Mapping(system_map));
        fs::write(
            &yml_path,
            serde_yaml_ng::to_string(&serde_yaml_ng::Value::Mapping(map)).unwrap(),
        )
        .unwrap();
        let checked = check_lutris_yml("test", &yml_path, &instance, &wiring, entry).unwrap();
        assert_eq!(checked.state, ItemState::Mismatched);
        assert!(checked.detail.contains("disable_runtime"));
        // Managed runner with the runtime disabled: mismatched the other way.
        let managed = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let managed_yml = render_lutris_yml("test", &instance, entry, &managed, &wiring).unwrap();
        let flipped = managed_yml.replacen("disable_runtime: false", "disable_runtime: true", 1);
        assert_ne!(managed_yml, flipped);
        fs::write(&yml_path, &flipped).unwrap();
        let checked = check_lutris_yml("test", &yml_path, &instance, &wiring, entry).unwrap();
        assert_eq!(checked.state, ItemState::Mismatched);
        assert!(checked.detail.contains("disable_runtime"));
    }

    fn sqlite3_available() -> bool {
        Command::new("sqlite3")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// sqlite3 is a required test dependency (flake check inputs); a missing
    /// binary fails loudly instead of silently skipping coverage.
    fn require_sqlite() {
        assert!(sqlite3_available(), "sqlite3 required for wiring tests");
    }

    /// Hermetic Lutris locations for fixtures: ambient XDG_* must never
    /// leak into tests.
    fn dirs(home: &tempfile::TempDir) -> HomeDirs {
        HomeDirs::isolated(home.path().to_owned())
    }

    /// Full state capture (names, device, inode, mode, mtime with
    /// nanosecond precision, bytes) for observable-state comparisons: an
    /// unexpected write changes identity, timestamps, or bytes and fails
    /// the comparison. Matching snapshots establish unchanged captured
    /// state, not proof that no write occurred.
    fn snapshot_tree(root: &Path) -> Vec<(PathBuf, u64, u64, u32, i64, i64, Option<Vec<u8>>)> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let mut out = Vec::new();
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                let meta = fs::symlink_metadata(&path).unwrap();
                let bytes = if meta.is_file() {
                    Some(fs::read(&path).unwrap())
                } else {
                    None
                };
                if meta.is_dir() {
                    stack.push(path.clone());
                }
                out.push((
                    path,
                    meta.dev(),
                    meta.ino(),
                    meta.permissions().mode(),
                    meta.mtime(),
                    meta.mtime_nsec(),
                    bytes,
                ));
            }
        }
        out.sort();
        out
    }

    #[test]
    fn wineboot_env_is_scrubbed_and_arch_pinned() {
        use std::ffi::OsString;
        let prefix = Path::new("/games/octo-prefix");
        let current = vec![
            (OsString::from("WINEARCH"), OsString::from("win32")),
            (OsString::from("WINEPREFIX"), OsString::from("/evil")),
            (OsString::from("HOME"), OsString::from("/home/u")),
            (OsString::from("DISPLAY"), OsString::from(":0")),
            (OsString::from("PATH"), OsString::from("/nix/store/x/bin")),
        ];
        let (remove, set) = scrub_wine_env(current, prefix, "wow64");
        // Every inherited WINE* is removed; session env is untouched (absent
        // from both lists, hence inherited).
        assert!(remove.contains(&OsString::from("WINEARCH")));
        assert!(remove.contains(&OsString::from("WINEPREFIX")));
        assert!(!remove.contains(&OsString::from("HOME")));
        let set: BTreeMap<_, _> = set.into_iter().collect();
        assert_eq!(
            set[&OsString::from("WINEPREFIX")],
            OsString::from("/games/octo-prefix")
        );
        assert!(!set.contains_key(&OsString::from("WINEARCH")));
        let (_, set) = scrub_wine_env(vec![], prefix, "win32");
        let set: BTreeMap<_, _> = set.into_iter().collect();
        assert_eq!(set[&OsString::from("WINEARCH")], OsString::from("win32"));
    }

    #[test]
    fn pe_header_at_later_offset_truncated_and_absurd() {
        // Realistic layout: DOS stub then PE at 0x200.
        let mut bytes = vec![0u8; 0x200 + 24];
        bytes[0..2].copy_from_slice(b"MZ");
        bytes[0x3c..0x40].copy_from_slice(&0x200u32.to_le_bytes());
        bytes[0x200..0x204].copy_from_slice(b"PE\0\0");
        bytes[0x204..0x206].copy_from_slice(&0x14cu16.to_le_bytes());
        bytes[0x200 + 22..0x200 + 24].copy_from_slice(&0x012fu16.to_le_bytes());
        assert_eq!(pe_exe_flags(&bytes).unwrap(), (true, true));
        // Same, through the bounded file reader.
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("a.exe");
        fs::write(&exe, &bytes).unwrap();
        let mut file = fs::File::open(&exe).unwrap();
        assert_eq!(pe_exe_flags_file(&mut file).unwrap(), (true, true));
        // Truncated COFF fails.
        fs::write(&exe, &bytes[..0x200 + 10]).unwrap();
        let mut file = fs::File::open(&exe).unwrap();
        assert!(pe_exe_flags_file(&mut file).is_err());
        // Absurd offset fails without seeking into nowhere.
        let mut bad = vec![0u8; 0x40];
        bad[0..2].copy_from_slice(b"MZ");
        bad[0x3c..0x40].copy_from_slice(&0xdead_beefu32.to_le_bytes());
        fs::write(&exe, &bad).unwrap();
        let mut file = fs::File::open(&exe).unwrap();
        assert!(pe_exe_flags_file(&mut file).is_err());
    }

    #[test]
    fn prefix_appearing_during_creation_survives() {
        // create_prefix on an existing directory fails without deleting it.
        let dir = tempfile::tempdir().unwrap();
        let prefix = dir.path().join("prefix");
        fs::create_dir(&prefix).unwrap();
        fs::write(prefix.join("keep.me"), "user data").unwrap();
        let runner = Runner {
            path: PathBuf::from("/nonexistent/wine"),
            version: "test".into(),
        };
        let wiring = octowow_hd_defaults();
        let err = create_prefix(&prefix, &runner, &wiring).unwrap_err();
        assert!(
            format!("{err:#}").contains("appeared"),
            "unexpected: {err:#}"
        );
        assert_eq!(fs::read(prefix.join("keep.me")).unwrap(), b"user data");
    }

    #[test]
    fn oversized_text_stops_at_the_bound() {
        let (_envelope, root) = fixture_root();
        fs::write(root.join("realmlist.wtf"), vec![b'x'; (1 << 20) + 1]).unwrap();
        assert!(read_capped(&root, "realmlist.wtf", 1 << 20).is_err());
        fs::write(root.join("realmlist.wtf"), b"small").unwrap();
        assert_eq!(
            read_capped(&root, "realmlist.wtf", 1 << 20).unwrap(),
            b"small"
        );
    }

    #[test]
    fn native_and_flatpak_installations_do_not_mix() {
        let (_envelope, _root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        // Database in Flatpak, yml only in native: the yml does not count.
        let flat_data = home.path().join(".var/app/net.lutris.Lutris/data/lutris");
        fs::create_dir_all(&flat_data).unwrap();
        fs::write(flat_data.join("pga.db"), "fake").unwrap();
        let native_yml = home.path().join(".config/lutris/games/octowow-test.yml");
        fs::create_dir_all(native_yml.parent().unwrap()).unwrap();
        fs::write(&native_yml, "game:\n  exe: elsewhere\n").unwrap();
        assert!(lutris_yml_path(&dirs(&home), "octowow-test").is_none());
    }

    #[test]
    fn contended_database_fails_before_inspection() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        // Another onboard run holds the data-dir lock: fail fast, no writes.
        let data = home.path().join(".local/share/lutris");
        let anchor = Anchor::open(&data).unwrap();
        let _held = anchor.lock().unwrap();
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("another onboard run"),
            "unexpected: {err:#}"
        );
        assert!(
            !home
                .path()
                .join(".config/lutris/games/octowow-test.yml")
                .exists()
        );
        assert!(!_envelope.path().join("octowow-prefix").exists());
    }

    #[test]
    fn xdg_resolution_honors_explicit_absolute_and_falls_back() {
        let home = PathBuf::from("/home/u");
        let dirs = HomeDirs::resolve(
            home.clone(),
            Some(PathBuf::from("/data/xdg")),
            Some(PathBuf::from("relative/ignored")),
        );
        assert_eq!(dirs.data, PathBuf::from("/data/xdg"));
        assert_eq!(dirs.config, PathBuf::from("/home/u/.config"));
        let dirs = HomeDirs::resolve(home.clone(), None, None);
        assert_eq!(dirs.data, PathBuf::from("/home/u/.local/share"));
        assert_eq!(dirs.config, PathBuf::from("/home/u/.config"));
        // Paired sites derive from the resolved roots, not $HOME joins.
        assert_eq!(
            lutris_sites(&dirs)[0],
            (
                PathBuf::from("/home/u/.config/lutris/games"),
                PathBuf::from("/home/u/.local/share/lutris"),
            )
        );
    }

    #[test]
    fn pgrep_exit_codes_map_fail_closed() {
        let exited = |code: i32| {
            Command::new("sh")
                .args(["-c", &format!("exit {code}")])
                .status()
                .unwrap()
        };
        assert!(interpret_pgrep(exited(0)).unwrap());
        assert!(!interpret_pgrep(exited(1)).unwrap());
        assert!(interpret_pgrep(exited(2)).is_err());
    }

    #[test]
    fn wineboot_arch_mismatch_fails_and_retains_prefix() {
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        // Fake wineboot claims a win32 prefix while wow64 is declared.
        let bin = home
            .path()
            .join(".local/share/lutris/runners/wine/wine-ge-9-2/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(
            bin.join("wine"),
            "#!/bin/sh\nprintf '#arch=win32\\n' > \"$WINEPREFIX/system.reg\"\n",
        )
        .unwrap();
        fs::write(bin.join("wineserver"), "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            for binary in ["wine", "wineserver"] {
                fs::set_permissions(bin.join(binary), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        lutris_home(home.path());
        let instance = apply_fixture(&root, home.path());
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("want=win64"),
            "unexpected: {err:#}"
        );
        // The partial prefix is retained (never deleted) with journaled evidence.
        assert!(_envelope.path().join("octowow-prefix").exists());
        let journal =
            fs::read_to_string(_envelope.path().join("octo-manager/wiring-journal.jsonl")).unwrap();
        assert!(journal.contains("apply-failed") && journal.contains("prefix"));
    }

    #[test]
    fn wineboot_failure_retains_prefix_with_evidence() {
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        let bin = home
            .path()
            .join(".local/share/lutris/runners/wine/wine-ge-9-2/bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("wine"), "#!/bin/sh\nexit 1\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(bin.join("wine"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        lutris_home(home.path());
        let instance = apply_fixture(&root, home.path());
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("wineboot"),
            "unexpected: {err:#}"
        );
        // Retained, never deleted, with journaled evidence.
        assert!(_envelope.path().join("octowow-prefix").exists());
        let journal =
            fs::read_to_string(_envelope.path().join("octo-manager/wiring-journal.jsonl")).unwrap();
        assert!(journal.contains("apply-failed") && journal.contains("prefix"));
    }

    /// Instance with a launcher block (installer + own prefix + entry).
    /// Returns the instance; the installer file digest is computed live.
    /// An installed executable path can be declared for the post-install
    /// entry; the fake launcher settings point at the wrong folder so
    /// reconciliation has something to fix (created only when asked).
    fn launcher_fixture(
        root: &Path,
        installer_sha: Option<String>,
        executable: bool,
        settings: bool,
    ) -> Instance {
        let state = root.parent().unwrap().join("octo-manager");
        let installer = root.parent().unwrap().join("OctoLauncher_Installer.exe");
        fs::write(&installer, "fake-installer-bytes").unwrap();
        let prefix = root.parent().unwrap().join("octowow-launcher-prefix");
        let mut launcher = serde_json::json!({
            "installer": {"path": installer},
            "prefix": prefix,
            "lutris": {"slug": "octowow-launcher-test", "name": "OctoWoW Launcher"},
        });
        if executable {
            let exe = prefix.join("drive_c/users/testuser/AppData/Local/Programs/OctoLauncher/OctoLauncher.exe");
            fs::create_dir_all(exe.parent().unwrap()).unwrap();
            fs::write(&exe, "fake-launcher-exe").unwrap();
            launcher["executable"] = serde_json::Value::String(exe.display().to_string());
        }
        if settings {
            // Default Wine mapping (z: -> /) plus a settings file pointing
            // at the wrong folder; unrelated keys must survive untouched.
            // The prefix already exists (win64 marker) so registration
            // reconciles instead of creating it.
            fs::create_dir_all(&prefix).unwrap();
            fs::write(prefix.join("system.reg"), "#arch=win64\n").unwrap();
            fs::create_dir_all(prefix.join("dosdevices")).unwrap();
            std::os::unix::fs::symlink("/", prefix.join("dosdevices/z:")).unwrap();
            let dir = prefix.join("drive_c/users/testuser/AppData/Roaming/octo-launcher");
            fs::create_dir_all(&dir).unwrap();
            fs::write(
                dir.join("settings.json"),
                "{\n  \"server\": \"live\",\n  \"clientDir\": \"Z:\\\\wrong\\\\folder\",\n  \"activeClientDir\": \"Z:\\\\wrong\\\\folder\",\n  \"windowPosition\": {\"x\": 1}\n}\n",
            )
            .unwrap();
        }
        let mut value = serde_json::json!({
            "root": root,
            "client": "wow-classic",
            "state_dir": state,
            "preset": "octowow-hd",
            "wiring": {
                "prefix": {"path": root.parent().unwrap().join("octowow-prefix")},
                "lutris": {"slug": "octowow-test", "name": "OctoWoW"},
                "launcher": launcher,
            },
        });
        if let Some(sha) = installer_sha {
            value["wiring"]["launcher"]["installer"]["sha256"] = serde_json::Value::String(sha);
        }
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn launcher_spec_has_no_game_overrides_and_parses() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = launcher_fixture(&root, None, false, false);
        let wiring = resolve_wiring(&instance).unwrap();
        let launcher = wiring.launcher.clone().unwrap();
        let runner = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let spec = launcher_spec(
            launcher.installer.as_ref().unwrap(),
            &launcher.prefix.clone().unwrap(),
            launcher.lutris.as_ref().unwrap(),
            &runner,
            &wiring,
            launcher.executable.as_ref(),
        )
        .unwrap();
        assert!(spec.dll_overrides.is_empty());
        assert_eq!(spec.wine_arch, "win64");
        let yml = render_entry_yml(&spec).unwrap();
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        assert_eq!(
            parsed["game"]["prefix"].as_str().unwrap(),
            spec.prefix.as_deref().unwrap()
        );
        assert_eq!(parsed["wine"]["eac"].as_bool(), Some(false));
        assert!(parsed["system"]["env"].get("WINEDLLOVERRIDES").is_none());
    }

    #[test]
    fn launcher_spec_prefers_declared_executable() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = launcher_fixture(&root, None, true, false);
        let wiring = resolve_wiring(&instance).unwrap();
        let launcher = wiring.launcher.clone().unwrap();
        let runner = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let spec = launcher_spec(
            launcher.installer.as_ref().unwrap(),
            &launcher.prefix.clone().unwrap(),
            launcher.lutris.as_ref().unwrap(),
            &runner,
            &wiring,
            launcher.executable.as_ref(),
        )
        .unwrap();
        let exe = launcher.executable.clone().unwrap();
        assert_eq!(spec.exe, exe.display().to_string());
        assert_eq!(spec.dir, exe.parent().unwrap().display().to_string());
    }

    #[test]
    fn launcher_prefix_command_renders_and_verifies() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let mut instance = launcher_fixture(&root, None, true, false);
        instance.wiring.as_mut().unwrap().launcher.as_mut().unwrap().lutris.as_mut().unwrap().command_prefix =
            Some("/bin/octowow-stdio-redir".into());
        let wiring = resolve_wiring(&instance).unwrap();
        let launcher = wiring.launcher.clone().unwrap();
        let runner = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let spec = launcher_spec(
            launcher.installer.as_ref().unwrap(),
            &launcher.prefix.clone().unwrap(),
            launcher.lutris.as_ref().unwrap(),
            &runner,
            &wiring,
            launcher.executable.as_ref(),
        )
        .unwrap();
        assert_eq!(
            spec.command_prefix.as_deref(),
            Some("/bin/octowow-stdio-redir")
        );
        let yml = render_entry_yml(&spec).unwrap();
        assert!(yml.contains("prefix_command: /bin/octowow-stdio-redir"));
        // Structural check enforces the declared prefix against a file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("entry.yml");
        fs::write(&path, &yml).unwrap();
        let recorded: Result<Option<String>> = Ok(Some(runner.version.clone()));
        let checked = check_entry_yml(&path, "launcher-entry", "fix", &spec, recorded);
        assert_eq!(
            checked.state,
            ItemState::Verified,
            "detail: {}",
            checked.detail
        );
        // Drift (prefix removed) mismatches instead of silently passing.
        let drifted = yml.replace("prefix_command: /bin/octowow-stdio-redir\n", "");
        fs::write(&path, &drifted).unwrap();
        let recorded: Result<Option<String>> = Ok(Some(runner.version.clone()));
        let checked = check_entry_yml(&path, "launcher-entry", "fix", &spec, recorded);
        assert_eq!(checked.state, ItemState::Mismatched);
    }

    #[test]
    fn launcher_bundle_completeness_requires_payload() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("OctoLauncher.exe");
        fs::write(&exe, "fake-exe").unwrap();
        assert!(!launcher_bundle_complete(&exe));
        fs::create_dir_all(dir.path().join("resources")).unwrap();
        fs::write(dir.path().join("resources/app.asar"), "fake-asar").unwrap();
        assert!(!launcher_bundle_complete(&exe));
        fs::write(
            dir.path().join("resources/app-update.yml"),
            "provider: generic\n",
        )
        .unwrap();
        assert!(launcher_bundle_complete(&exe));
        // Symlinked payload fails closed.
        fs::remove_file(dir.path().join("resources/app.asar")).unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("resources/app-update.yml"),
            dir.path().join("resources/app.asar"),
        )
        .unwrap();
        assert!(!launcher_bundle_complete(&exe));
    }

    #[test]
    fn launcher_settings_skips_profile_aliases() {
        let prefix = tempfile::tempdir().unwrap();
        let users = prefix.path().join("drive_c/users");
        let settings = "testuser/AppData/Roaming/octo-launcher/settings.json";
        fs::create_dir_all(users.join("testuser/AppData/Roaming/octo-launcher")).unwrap();
        fs::write(users.join(settings), "{\"clientDir\": \"Z:\\\\x\"}").unwrap();
        // A profile alias and a shell-folder symlink must neither count as
        // profiles nor explode the scan.
        std::os::unix::fs::symlink(users.join("testuser"), users.join("steamuser")).unwrap();
        std::os::unix::fs::symlink("/nonexistent-templates", users.join("Templates")).unwrap();
        let found = find_launcher_settings(prefix.path()).unwrap().unwrap();
        assert_eq!(
            found,
            Path::new("drive_c/users/testuser/AppData/Roaming/octo-launcher/settings.json")
        );
    }

    #[test]
    fn launcher_client_dir_mapping_needs_default_z_drive() {        let prefix = tempfile::tempdir().unwrap();
        // No dosdevices at all.
        assert!(wine_client_dir(prefix.path(), Path::new("/games/octo")).is_err());
        // A custom mapping fails closed.
        fs::create_dir_all(prefix.path().join("dosdevices")).unwrap();
        std::os::unix::fs::symlink("/elsewhere", prefix.path().join("dosdevices/z:")).unwrap();
        let err = wine_client_dir(prefix.path(), Path::new("/games/octo")).unwrap_err();
        assert!(
            format!("{err:#}").contains("not /"),
            "unexpected: {err:#}"
        );
        // The default mapping converts slashes to backslashes.
        fs::remove_file(prefix.path().join("dosdevices/z:")).unwrap();
        std::os::unix::fs::symlink("/", prefix.path().join("dosdevices/z:")).unwrap();
        assert_eq!(
            wine_client_dir(prefix.path(), Path::new("/games/octo")).unwrap(),
            "Z:\\games\\octo"
        );
    }

    #[test]
    fn launcher_settings_edit_touches_only_the_two_keys() {
        let before = "{\n  \"server\": \"live\",\n  \"clientDir\": \"Z:\\\\old\",\n  \"activeClientDir\": \"Z:\\\\old\",\n  \"mods\": {\"dxvk\": {\"enabled\": true}}\n}\n";
        let (after, changed, note) =
            set_launcher_client_dirs(before, "Z:\\new", "Z:\\old", Some("Z:\\old")).unwrap();
        assert!(changed);
        assert!(note.is_none());
        assert!(after.contains("\"clientDir\": \"Z:\\\\new\""));
        assert!(after.contains("\"activeClientDir\": \"Z:\\\\new\""));
        // Unrelated state survives byte-identical.
        assert!(after.contains("\"server\": \"live\""));
        assert!(after.contains("\"mods\": {\"dxvk\": {\"enabled\": true}}"));
        // Already reconciled: no change.
        let (_, changed, _) =
            set_launcher_client_dirs(&after, "Z:\\new", "Z:\\new", Some("Z:\\new")).unwrap();
        assert!(!changed);
        // Missing client key fails closed instead of inventing placement.
        assert!(set_launcher_client_dirs("{\"a\": 1}", "Z:\\new", "Z:\\old", None).is_err());
    }

    #[test]
    fn launcher_settings_edit_ignores_nested_same_name_keys() {
        let before = "{\n  \"mods\": {\"clientDir\": \"Z:\\\\nested\"},\n  \"clientDir\": \"Z:\\\\old\",\n  \"activeClientDir\": \"Z:\\\\old\"\n}\n";
        let (after, changed, _) =
            set_launcher_client_dirs(before, "Z:\\new", "Z:\\old", Some("Z:\\old")).unwrap();
        assert!(changed);
        assert!(after.contains("\"mods\": {\"clientDir\": \"Z:\\\\nested\"}"));
        assert!(after.contains("\"clientDir\": \"Z:\\\\new\""));
        assert!(after.contains("\"activeClientDir\": \"Z:\\\\new\""));
    }

    #[test]
    fn launcher_settings_edit_rejects_duplicates_and_malformed() {
        // Duplicate top-level key: ambiguous, fail closed.
        assert!(set_launcher_client_dirs(
            "{\"clientDir\": \"Z:\\\\a\", \"clientDir\": \"Z:\\\\b\", \"activeClientDir\": \"Z:\\\\a\"}",
            "Z:\\new",
            "Z:\\a",
            Some("Z:\\a"),
        )
        .is_err());
        // Unterminated strings and malformed \u escapes fail closed
        // (unknown backslash escapes stay lenient here; the serde parse
        // upstream rejects invalid JSON before editing starts).
        assert!(set_launcher_client_dirs(
            "{\"clientDir\": \"Z:\\\\old",
            "Z:\\new",
            "Z:\\old",
            None,
        )
        .is_err());
        assert!(set_launcher_client_dirs(
            "{\"clientDir\": \"Z:\\\\x\\u12zz\", \"activeClientDir\": \"Z:\\\\x\"}",
            "Z:\\new",
            "Z:\\x",
            Some("Z:\\x"),
        )
        .is_err());
        // Non-object document fails closed.
        assert!(set_launcher_client_dirs("[\"clientDir\"]", "Z:\\new", "Z:\\old", None).is_err());
        // Trailing data fails closed.
        assert!(set_launcher_client_dirs(
            "{\"clientDir\": \"Z:\\\\old\", \"activeClientDir\": \"Z:\\\\old\"} trailing",
            "Z:\\new",
            "Z:\\old",
            Some("Z:\\old"),
        )
        .is_err());
    }

    #[test]
    fn launcher_settings_edit_defers_foreign_active_dir() {
        // activeClientDir tracks another folder's sync state: clientDir
        // still follows the root, but the foreign value is reported, never
        // reassigned.
        let before = "{\n  \"clientDir\": \"Z:\\\\old\",\n  \"activeClientDir\": \"Z:\\\\elsewhere\"\n}\n";
        let (after, changed, note) =
            set_launcher_client_dirs(before, "Z:\\new", "Z:\\old", Some("Z:\\elsewhere")).unwrap();
        assert!(changed);
        assert!(after.contains("\"clientDir\": \"Z:\\\\new\""));
        assert!(after.contains("\"activeClientDir\": \"Z:\\\\elsewhere\""));
        assert_eq!(
            note.as_deref(),
            Some("activeClientDir tracks 'Z:\\elsewhere', not the declared client")
        );
    }

    #[test]
    fn register_launcher_repoints_entry_and_client_dir() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = launcher_fixture(&root, None, true, true);
        // Status before: entry missing, stored client folder wrong.
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let state_of = |name: &str| {
            items
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("no item {name}"))
                .state
        };
        assert_eq!(state_of("launcher-entry"), ItemState::Missing);
        assert_eq!(state_of("launcher-client-dir"), ItemState::Mismatched);
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        // Lutris entry launches the installed exe from its own directory.
        let wiring = resolve_wiring(&instance).unwrap();
        let launcher = wiring.launcher.clone().unwrap();
        let exe = launcher.executable.clone().unwrap();
        let yml = fs::read_to_string(
            home.path()
                .join(".config/lutris/games/octowow-launcher-test.yml"),
        )
        .unwrap();
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        assert_eq!(
            parsed["game"]["exe"].as_str().unwrap(),
            exe.display().to_string()
        );
        assert_eq!(
            parsed["game"]["working_dir"].as_str().unwrap(),
            exe.parent().unwrap().display().to_string()
        );
        // Settings: both keys name the game root; the rest is untouched.
        let settings = launcher.prefix.clone().unwrap().join(
            "drive_c/users/testuser/AppData/Roaming/octo-launcher/settings.json",
        );
        let body = fs::read_to_string(&settings).unwrap();
        let want = format!(
            "Z:{}",
            root.display().to_string().replace('/', "\\")
        );
        let want_json = serde_json::to_string(&want).unwrap();
        assert!(body.contains(&format!("\"clientDir\": {want_json}")));
        assert!(body.contains(&format!("\"activeClientDir\": {want_json}")));
        assert!(body.contains("\"server\": \"live\""));
        assert!(body.contains("\"windowPosition\": {\"x\": 1}"));
        assert!(!body.contains("wrong"));
        // Previous settings retained under a unique backup.
        let backups: Vec<_> = fs::read_dir(settings.parent().unwrap())
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.file_name()))
            .filter(|name| {
                name.to_string_lossy()
                    .starts_with("settings.bak-modde-launcher-settings-")
            })
            .collect();
        assert_eq!(backups.len(), 1);
        // Repeat call is a full no-op and both items verify.
        let before_home = snapshot_tree(home.path());
        let before_game = snapshot_tree(_envelope.path());
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        assert_eq!(snapshot_tree(home.path()), before_home);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let state_of = |name: &str| {
            items
                .iter()
                .find(|i| i.name == name)
                .unwrap_or_else(|| panic!("no item {name}"))
                .state
        };
        assert_eq!(state_of("launcher-entry"), ItemState::Verified);
        assert_eq!(state_of("launcher-client-dir"), ItemState::Verified);
    }

    #[test]
    fn register_launcher_refuses_ambiguous_settings_before_writes() {
        let (_guard, _envelope, root, home) = apply_test_env();
        // Prefix exists with a win64 marker, but two user profiles hold
        // settings: fail before any yml or row exists, with evidence.
        let prefix = root.parent().unwrap().join("octowow-launcher-prefix");
        fs::create_dir_all(&prefix).unwrap();
        fs::write(prefix.join("system.reg"), "#arch=win64\n").unwrap();
        for user in ["alice", "bob"] {
            let dir = prefix.join(format!("drive_c/users/{user}/AppData/Roaming/octo-launcher"));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("settings.json"), "{\"clientDir\": \"Z:\\\\x\"}").unwrap();
        }
        let instance = launcher_fixture(&root, None, false, false);
        let err = register_launcher("test", &instance, &dirs(&home), false).unwrap_err();
        assert!(
            format!("{err:#}").contains("ambiguous launcher settings"),
            "unexpected: {err:#}"
        );
        assert!(
            !home
                .path()
                .join(".config/lutris/games/octowow-launcher-test.yml")
                .exists()
        );
        let journal =
            fs::read_to_string(_envelope.path().join("octo-manager/wiring-journal.jsonl")).unwrap();
        assert!(journal.contains("launcher-settings"));
    }

    #[test]
    fn register_launcher_creates_prefix_and_entry_then_noops() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = launcher_fixture(&root, None, false, false);
        // Status reports the missing launcher entry before registration.
        let items = status("test", &instance, &dirs(&home)).unwrap();
        assert_eq!(
            items
                .iter()
                .find(|i| i.name == "launcher-entry")
                .unwrap()
                .state,
            ItemState::Missing
        );
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        let yml_path = home
            .path()
            .join(".config/lutris/games/octowow-launcher-test.yml");
        let yml_before = fs::read(&yml_path).unwrap();
        assert!(db_dump(home.path()).contains("octowow-launcher-test|"));
        assert!(
            _envelope
                .path()
                .join("octowow-launcher-prefix/system.reg")
                .exists()
        );
        // Effective entry verifies; repeat call is a full no-op.
        let before_home = snapshot_tree(home.path());
        let before_game = snapshot_tree(_envelope.path());
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        assert_eq!(fs::read(&yml_path).unwrap(), yml_before);
        assert_eq!(snapshot_tree(home.path()), before_home);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
        let items = status("test", &instance, &dirs(&home)).unwrap();
        assert_eq!(
            items
                .iter()
                .find(|i| i.name == "launcher-entry")
                .unwrap()
                .state,
            ItemState::Verified
        );
    }

    #[test]
    fn register_launcher_refuses_bad_installer_before_writes() {
        let (_guard, _envelope, root, home) = apply_test_env();
        // Wrong digest fails before prefix, yml, or row exist.
        let instance = launcher_fixture(&root, Some("0".repeat(64)), false, false);
        let err = register_launcher("test", &instance, &dirs(&home), false).unwrap_err();
        assert!(format!("{err:#}").contains("digest"), "unexpected: {err:#}");
        assert!(!_envelope.path().join("octowow-launcher-prefix").exists());
        assert!(
            !home
                .path()
                .join(".config/lutris/games/octowow-launcher-test.yml")
                .exists()
        );
        // Missing installer file fails the same way (fixture recreates it;
        // remove once for the missing case).
        let instance = launcher_fixture(&root, None, false, false);
        fs::remove_file(root.parent().unwrap().join("OctoLauncher_Installer.exe")).unwrap();
        assert!(register_launcher("test", &instance, &dirs(&home), false).is_err());
        assert!(!_envelope.path().join("octowow-launcher-prefix").exists());
    }

    #[test]
    fn register_launcher_conflict_needs_adopt() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[
            db.display().to_string(),
            "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed) VALUES ('Other', 'octowow-launcher-test', 'wine', 'Linux', '/games/other', '/games/other/run.exe', 'octowow-launcher-test', 1);".into(),
        ])
        .unwrap();
        let instance = launcher_fixture(&root, None, false, false);
        let err = register_launcher("test", &instance, &dirs(&home), false).unwrap_err();
        assert!(
            format!("{err:#}").contains("--adopt"),
            "unexpected: {err:#}"
        );
        assert!(!_envelope.path().join("octowow-launcher-prefix").exists());
        register_launcher("test", &instance, &dirs(&home), true).unwrap();
        assert!(db_dump(home.path()).contains("octowow-launcher-test|"));
    }

    #[test]
    fn first_registration_orphan_removed_on_database_failure() {
        let (_guard, _envelope, root, home) = apply_test_env();
        // Abort INSERTs: the yml is created new, then the row fails.
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[
            db.display().to_string(),
            "CREATE TRIGGER lock_ins BEFORE INSERT ON games BEGIN SELECT RAISE(ABORT, 'locked'); END;"
                .into(),
        ])
        .unwrap();
        let instance = apply_fixture(&root, home.path());
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("database"),
            "unexpected: {err:#}"
        );
        // No prior yml existed, so nothing is restored — the orphan is gone.
        assert!(
            !home
                .path()
                .join(".config/lutris/games/octowow-test.yml")
                .exists()
        );
    }

    #[test]
    fn plan_separates_changes_from_blockers() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let changes = plan("test", &instance, &dirs(&home)).unwrap();
        let kind = |name: &str| {
            changes
                .iter()
                .find(|c| c.resource.ends_with(name))
                .unwrap()
                .kind
        };
        assert_eq!(kind("prefix"), WiringChangeKind::Change);
        assert_eq!(kind("lutris-yml"), WiringChangeKind::Change);
        assert_eq!(kind("endpoints"), WiringChangeKind::Blocker);
        assert_eq!(kind("hd-patch:A"), WiringChangeKind::Blocker);
    }

    #[test]
    fn system_wine_is_recorded_not_bypassed() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let mut instance = fixture_instance(&root);
        let fake = home
            .path()
            .join(".local/share/lutris/runners/wine/wine-ge-9-2/bin/wine");
        // Declaration says system; the record binds the resolved artifact.
        instance.wiring.as_mut().unwrap().runtime.version = Some("system".into());
        let wiring = resolve_wiring(&instance).unwrap();
        let state = _envelope.path().join("octo-manager");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("wiring-runtime.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": "system",
                "path": fake,
            }))
            .unwrap(),
        )
        .unwrap();
        let (runner, recorded) = select_runner(&dirs(&home), &instance, &wiring, false).unwrap();
        assert!(recorded);
        assert_eq!(runner.version, "system");
    }

    fn lutris_home(home: &Path) {
        fs::create_dir_all(home.join(".config/lutris/games")).unwrap();
        let db = home.join(".local/share/lutris/pga.db");
        fs::create_dir_all(db.parent().unwrap()).unwrap();
        sqlite3(&[
            db.display().to_string(),
            "CREATE TABLE IF NOT EXISTS games (id INTEGER PRIMARY KEY, name TEXT, slug TEXT UNIQUE, runner TEXT, platform TEXT, directory TEXT, executable TEXT, configpath TEXT, installed INTEGER);".into(),
        ])
        .unwrap();
    }

    /// Fake wine whose wineboot creates a win64 system.reg (WOW64-capable),
    /// plus a sibling fake wineserver that exits 0 (the creation wait).
    fn wineboot_home(home: &tempfile::TempDir, version: &str) {
        let bin = home
            .path()
            .join(format!(".local/share/lutris/runners/wine/{version}/bin"));
        fs::create_dir_all(&bin).unwrap();
        fs::write(
            bin.join("wine"),
            "#!/bin/sh\nif [ \"$1\" = \"wineboot\" ]; then printf '#arch=win64\\n' > \"$WINEPREFIX/system.reg\"; fi\n",
        )
        .unwrap();
        fs::write(bin.join("wineserver"), "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            for binary in ["wine", "wineserver"] {
                fs::set_permissions(bin.join(binary), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    #[test]
    fn wineserver_prefers_runner_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join("wine"), "#!/bin/sh\n").unwrap();
        fs::write(bin.join("wineserver"), "#!/bin/sh\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(bin.join("wineserver"), fs::Permissions::from_mode(0o755)).unwrap();
        }
        let runner = Runner {
            path: bin.join("wine"),
            version: "test".into(),
        };
        assert_eq!(wineserver_bin(&runner), bin.join("wineserver"));
        let runner = Runner {
            path: PathBuf::from("/nonexistent/wine"),
            version: "test".into(),
        };
        assert_eq!(wineserver_bin(&runner), PathBuf::from("wineserver"));
    }

    fn db_dump(home: &Path) -> String {
        let db = home.join(".local/share/lutris/pga.db");
        sqlite3(&[
            db.display().to_string(),
            "SELECT slug, executable FROM games ORDER BY slug;".into(),
        ])
        .unwrap()
    }

    /// Full-row dump for preservation checks: every owned column, so an
    /// unrelated row cannot change unnoticed behind an identical slug.
    fn db_full_dump(home: &Path) -> String {
        let db = home.join(".local/share/lutris/pga.db");
        sqlite3(&[
            db.display().to_string(),
            "SELECT id, name, slug, runner, platform, directory, executable, configpath, installed FROM games ORDER BY slug;".into(),
        ])
        .unwrap()
    }

    // Apply tests spawn sqlite3 with lutris paths in argv, which a sibling
    // test's `pgrep -f lutris` would mistake for a running Lutris. Serialize.
    static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Shared apply-test preamble: lock + sqlite gate + game envelope +
    /// logging fake wine runner + empty Lutris db. The fake wine handles
    /// `wineboot` (prefix creation, no log) and appends every other
    /// invocation to `home/wine.log`. Absence of that log proves no native
    /// wine exec (launch path). Gate refusal is proven by the readiness
    /// error surfacing before the final `exec`; gate tests use a
    /// nonexistent sentinel command so a mis-ordered gate would ENOENT
    /// instead of replacing the test process.
    fn apply_test_env() -> (
        std::sync::MutexGuard<'static, ()>,
        tempfile::TempDir,
        PathBuf,
        tempfile::TempDir,
    ) {
        let guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        lutris_home(home.path());
        (guard, envelope, root, home)
    }

    #[test]
    fn conflict_rejected_before_any_write() {
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        lutris_home(home.path());
        // Foreign entry: yml + db row point at another game entirely.
        let yml_path = home.path().join(".config/lutris/games/octowow-test.yml");
        fs::write(&yml_path, "game:\n  exe: /games/other/Game.exe\n").unwrap();
        let yml_before = fs::read(&yml_path).unwrap();
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[
            db.display().to_string(),
            "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed) VALUES ('Other', 'octowow-test', 'wine', 'Linux', '/games/other', '/games/other/Game.exe', 'octowow-test', 1);".into(),
        ])
        .unwrap();
        let instance = apply_fixture(&root, home.path());
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("--adopt"),
            "unexpected: {err:#}"
        );
        // Nothing written: yml bytes identical, no backups, no prefix.
        assert_eq!(fs::read(&yml_path).unwrap(), yml_before);
        assert_eq!(db_dump(home.path()), "octowow-test|/games/other/Game.exe");
        let parent = _envelope.path();
        assert!(!parent.join("octowow-prefix").exists());
        assert!(parent.read_dir().unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("bak-modde")
        }));
    }

    #[test]
    fn apply_is_idempotent_and_pins_runtime() {
        let (_guard, _envelope, root, home) = apply_test_env();
        // WAL mode: the backup path must stay consistent, unrelated rows kept.
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[db.display().to_string(), "PRAGMA journal_mode=WAL;".into()]).unwrap();
        sqlite3(&[
            db.display().to_string(),
            "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed) VALUES ('Unrelated', 'other-game', 'wine', 'Linux', '/games/other', '/games/other/g.exe', 'other-game', 1);".into(),
        ])
        .unwrap();
        // Snapshot the unrelated row BEFORE any registration (including its
        // id): the first apply must preserve it while adding the managed row.
        let unrelated_before = db_full_dump(home.path());
        assert!(unrelated_before.contains("Unrelated|other-game|wine|Linux|/games/other|/games/other/g.exe|other-game|1"));
        let instance = apply_fixture(&root, home.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        // First apply preserved the unrelated row byte-identical (ids included).
        let unrelated_after_first = db_full_dump(home.path());
        assert!(
            unrelated_after_first.contains(unrelated_before.trim()),
            "unrelated row changed across first apply: {unrelated_after_first}"
        );
        let yml_path = home.path().join(".config/lutris/games/octowow-test.yml");
        let yml_before = fs::read(&yml_path).unwrap();
        let dump_before = db_dump(home.path());
        assert!(dump_before.contains("other-game|/games/other/g.exe"));
        // Full-row preservation: an unrelated row cannot change behind an
        // identical slug (all owned columns compared).
        let full_before = db_full_dump(home.path());
        assert!(full_before.contains("Unrelated|other-game|wine|Linux|/games/other|/games/other/g.exe|other-game|1"));
        // A newer runner appears: the recorded selection must not move, and
        // the repeat apply must not change any game or Lutris state at all.
        wineboot_home(&home, "wine-ge-10-1");
        let before_game = snapshot_tree(_envelope.path());
        let before_home = snapshot_tree(home.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert_eq!(fs::read(&yml_path).unwrap(), yml_before);
        assert_eq!(db_dump(home.path()), dump_before);
        assert_eq!(db_full_dump(home.path()), full_before);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
        // sqlite's WAL bookkeeping touches the database directory on every
        // new connection — including the status reads inside apply and this
        // test's own db_dump — while every entry and byte stays identical.
        // Compare the full snapshot (nanosecond timestamps included) but
        // ignore that one directory's own mtime, so the observable-state
        // comparison tracks managed state rather than sqlite's lifecycle.
        let db_dir = db.parent().unwrap().to_owned();
        let stable = |tree: Vec<(PathBuf, u64, u64, u32, i64, i64, Option<Vec<u8>>)>| {
            tree.into_iter()
                .map(|(path, dev, ino, mode, mtime, mtime_nsec, bytes)| {
                    let timestamp = if path == db_dir {
                        (0, 0)
                    } else {
                        (mtime, mtime_nsec)
                    };
                    (path, dev, ino, mode, timestamp, bytes)
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(stable(snapshot_tree(home.path())), stable(before_home));
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(_envelope.path().join("octo-manager/wiring-runtime.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(record["version"], "wine-ge-9-2");
        // Effective entry verifies: prefix, recorded runtime, anticheat off.
        let wiring = resolve_wiring(&instance).unwrap();
        let checked = check_lutris_yml(
            "test",
            &yml_path,
            &instance,
            &wiring,
            wiring.lutris.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(checked.state, ItemState::Verified);
        assert!(
            status("test", &instance, &dirs(&home))
                .unwrap()
                .iter()
                .all(|item| item.state == ItemState::Verified)
        );
    }

    #[test]
    fn unrelated_rows_survive_game_and_launcher_registration() {
        // Two Ascension-like unrelated rows plus one unmanaged column the
        // manager never owns. Every mutating registration path (game INSERT,
        // launcher INSERT, managed UPDATEs, repeat no-ops) must leave both
        // rows byte-identical, ids and unmanaged values included.
        let (_guard, _envelope, root, home) = apply_test_env();
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[db.display().to_string(), "PRAGMA journal_mode=WAL;".into()]).unwrap();
        // Unmanaged column: real pga.db carries columns outside the owned
        // set (lastplayed, service, ...); the upsert uses explicit column
        // lists so it must never touch this.
        sqlite3(&[
            db.display().to_string(),
            "ALTER TABLE games ADD COLUMN lastplayed INTEGER NOT NULL DEFAULT 0;".into(),
        ])
        .unwrap();
        sqlite3(&[
            db.display().to_string(),
            "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed, lastplayed) VALUES ('Ascension', 'ascension', 'wine', 'Linux', '/games/ascension', '/games/ascension/WoW.exe', 'ascension', 1, 1720000000);".into(),
        ])
        .unwrap();
        sqlite3(&[
            db.display().to_string(),
            "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed, lastplayed) VALUES ('Ascension Launcher', 'ascension-launcher', 'wine', 'Linux', '/games/ascension-launcher', '/games/ascension-launcher/Launcher.exe', 'ascension-launcher', 1, 1730000000);".into(),
        ])
        .unwrap();
        let unrelated = |home: &Path| {
            let db = home.join(".local/share/lutris/pga.db");
            sqlite3(&[
                db.display().to_string(),
                "SELECT id, name, slug, runner, platform, directory, executable, configpath, installed, lastplayed FROM games WHERE slug IN ('ascension', 'ascension-launcher') ORDER BY slug;".into(),
            ])
            .unwrap()
        };
        // Snapshot BEFORE any registration: ids, all owned columns, and the
        // unmanaged value for both rows.
        let unrelated_before = unrelated(home.path());
        assert!(unrelated_before.contains("Ascension|ascension|wine|Linux|/games/ascension|/games/ascension/WoW.exe|ascension|1|1720000000"));
        assert!(unrelated_before.contains("Ascension Launcher|ascension-launcher|wine|Linux|/games/ascension-launcher|/games/ascension-launcher/Launcher.exe|ascension-launcher|1|1730000000"));
        // Game instance plus a launcher block (installer on disk, sibling
        // prefix, own Lutris slug) so one instance exercises both adapters.
        let mut instance = apply_fixture(&root, home.path());
        let installer = _envelope.path().join("OctoLauncher_Installer.exe");
        fs::write(&installer, "fake-installer-bytes").unwrap();
        instance.wiring.as_mut().unwrap().launcher = Some(
            serde_json::from_value(serde_json::json!({
                "installer": {"path": installer},
                "prefix": root.parent().unwrap().join("octowow-launcher-prefix"),
                "lutris": {"slug": "octowow-launcher-test", "name": "OctoWoW Launcher"},
            }))
            .unwrap(),
        );
        // 1. Game INSERT: unrelated rows survive the first managed write.
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert_eq!(unrelated(home.path()), unrelated_before);
        assert!(db_dump(home.path()).contains("octowow-test|"));
        // 2. Launcher INSERT: a second managed slug, same preservation.
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        assert_eq!(unrelated(home.path()), unrelated_before);
        assert!(db_dump(home.path()).contains("octowow-launcher-test|"));
        // 3. Forced managed UPDATE (game): rename the managed entry so the
        // upsert takes the UPDATE branch; unrelated rows must not move.
        instance.wiring.as_mut().unwrap().lutris.as_mut().unwrap().name = "OctoWoW Renamed".into();
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert_eq!(unrelated(home.path()), unrelated_before);
        assert!(db_full_dump(home.path()).contains("OctoWoW Renamed|octowow-test|"));
        // 4. Forced managed UPDATE (launcher): same UPDATE-branch coverage
        // through the launcher adapter.
        instance.wiring.as_mut().unwrap().launcher.as_mut().unwrap().lutris.as_mut().unwrap().name =
            "OctoWoW Launcher Renamed".into();
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        assert_eq!(unrelated(home.path()), unrelated_before);
        assert!(db_full_dump(home.path()).contains("OctoWoW Launcher Renamed|octowow-launcher-test|"));
        // 5. Repeat no-ops: converged game + launcher registrations change
        // nothing, unrelated or managed.
        let full_before = db_full_dump(home.path());
        let unmanaged_before = unrelated(home.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        register_launcher("test", &instance, &dirs(&home), false).unwrap();
        assert_eq!(db_full_dump(home.path()), full_before);
        assert_eq!(unrelated(home.path()), unmanaged_before);
    }

    #[test]
    fn apply_registers_entry_with_missing_hd_payloads() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let mut instance = apply_fixture(&root, home.path());
        // A second HD letter with no payload: registration must not wait
        // for HD maintenance, while the absence stays visible.
        instance
            .wiring
            .as_mut()
            .unwrap()
            .data_patches
            .as_mut()
            .unwrap()
            .native_letters
            .push("B".into());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        // The vanilla entry is registered...
        assert!(home
            .path()
            .join(".config/lutris/games/octowow-test.yml")
            .is_file());
        assert!(db_dump(home.path()).contains("octowow-test|"));
        // ...the HD absence stays visible and journaled as deferred...
        let items = status("test", &instance, &dirs(&home)).unwrap();
        assert_eq!(
            items
                .iter()
                .find(|item| item.name == "hd-patch:B")
                .unwrap()
                .state,
            ItemState::Missing
        );
        let journal =
            fs::read_to_string(_envelope.path().join("octo-manager/wiring-journal.jsonl")).unwrap();
        assert!(journal.contains("apply-done") && journal.contains("hd-patch:B"));
        // ...and a repeat apply is still a full no-op.
        let before_home = snapshot_tree(home.path());
        let before_game = snapshot_tree(_envelope.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert_eq!(snapshot_tree(home.path()), before_home);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
    }

    #[test]
    fn apply_registers_despite_launch_readiness_drift() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let mut instance = apply_fixture(&root, home.path());
        // Client digest identity drifted (size mismatch) and a rename-dodge
        // letter sits outside the declared set: launch readiness, both.
        fs::write(root.join("WoW.exe"), "fake-wow").unwrap();
        instance
            .wiring
            .as_mut()
            .unwrap()
            .client_integrity
            .as_mut()
            .unwrap()
            .wow_exe = Some(
            serde_json::from_value(serde_json::json!({
                "size": 999,
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }))
            .unwrap(),
        );
        fs::write(root.join("Data/patch-F.mpq"), "renamed").unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let state_of = |name: &str| items.iter().find(|i| i.name == name).unwrap().state;
        assert_eq!(state_of("wow-exe"), ItemState::Mismatched);
        assert_eq!(state_of("hd-patch-letters"), ItemState::Mismatched);
        // Both stay blockers in the read-only plan, visible to the operator.
        let changes = plan("test", &instance, &dirs(&home)).unwrap();
        assert!(changes.iter().any(|c| c.resource.ends_with("wow-exe")));
        assert!(
            changes
                .iter()
                .any(|c| c.resource.ends_with("hd-patch-letters"))
        );
        // ...but neither refuses writing the declared entry.
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert!(
            home.path()
                .join(".config/lutris/games/octowow-test.yml")
                .is_file()
        );
        assert!(db_dump(home.path()).contains("octowow-test|"));
        // The journal records the readiness observation on apply-done...
        let journal =
            fs::read_to_string(_envelope.path().join("octo-manager/wiring-journal.jsonl")).unwrap();
        assert!(journal.contains("apply-done") && journal.contains("wow-exe"));
        // ...and a repeat apply stays a full no-op.
        let before_home = snapshot_tree(home.path());
        let before_game = snapshot_tree(_envelope.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert_eq!(snapshot_tree(home.path()), before_home);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
    }

    #[test]
    fn apply_ignores_register_owned_launcher_items() {
        let (_guard, _envelope, root, home) = apply_test_env();
        // Game block is fully satisfiable, but the declared launcher was
        // never registered: its entry, prefix, and settings stay missing.
        // Apply must still register the vanilla game entry — the launcher
        // lifecycle belongs to register-launcher, which is verified here
        // to have touched nothing.
        let mut instance = apply_fixture(&root, home.path());
        let launcher = serde_json::json!({
            "installer": {"path": root.parent().unwrap().join("OctoLauncher_Installer.exe")},
            "prefix": root.parent().unwrap().join("octowow-launcher-prefix"),
            "lutris": {"slug": "octowow-launcher-test", "name": "OctoWoW Launcher"},
        });
        fs::write(
            root.parent().unwrap().join("OctoLauncher_Installer.exe"),
            "fake-installer-bytes",
        )
        .unwrap();
        instance.wiring.as_mut().unwrap().launcher =
            serde_json::from_value(launcher).unwrap();
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert!(home
            .path()
            .join(".config/lutris/games/octowow-test.yml")
            .is_file());
        assert!(db_dump(home.path()).contains("octowow-test|"));
        assert!(!_envelope.path().join("octowow-launcher-prefix").exists());
        assert!(!home
            .path()
            .join(".config/lutris/games/octowow-launcher-test.yml")
            .exists());
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let state_of = |name: &str| {
            items
                .iter()
                .find(|item| item.name == name)
                .unwrap_or_else(|| panic!("no item {name}"))
                .state
        };
        assert_eq!(state_of("launcher-entry"), ItemState::Missing);
        assert_eq!(state_of("launcher-client-dir"), ItemState::Missing);
    }

    /// Fake wine that answers wineboot like `wineboot_home` and otherwise
    /// logs argv, working directory, and Wine environment, then exits 0.
    fn logging_wine(home: &tempfile::TempDir, version: &str, log: &Path) {
        let bin = home
            .path()
            .join(format!(".local/share/lutris/runners/wine/{version}/bin"));
        fs::create_dir_all(&bin).unwrap();
        fs::write(
            bin.join("wine"),
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"wineboot\" ]; then printf '#arch=win64\\n' > \"$WINEPREFIX/system.reg\"; exit 0; fi\n{{ printf 'pwd=%s\\n' \"$PWD\"; printf 'arg=%s\\n' \"$@\"; printenv | grep -E '^(WINE|WINEDLLO)' | sort; }} >> \"{}\" 2>/dev/null || true\n",
                log.display(),
            ),
        )
        .unwrap();
        fs::write(bin.join("wineserver"), "#!/bin/sh\nexit 0\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            for binary in ["wine", "wineserver"] {
                fs::set_permissions(bin.join(binary), fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
    }

    #[test]
    fn launch_env_mirrors_lutris_render_rules() {
        let (_envelope, root) = fixture_root();
        let instance = fixture_instance(&root);
        let wiring = resolve_wiring(&instance).unwrap();
        let prefix = root.parent().unwrap().join("octowow-prefix");
        let (_, set) = resolve_launch_env_with(&wiring.tunings, &wiring.runtime.arch, &prefix, &wiring.dll_overrides);
        let map: std::collections::BTreeMap<String, String> = set
            .into_iter()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(map["WINEPREFIX"], prefix.display().to_string());
        assert_eq!(map["WINEDEBUG"], "-all");
        assert_eq!(map["WINEDLLOVERRIDES"], "d3d9=n,b");
        // Structural keys win over tunings; the typed overrides win over
        // the extra_env impostor; other tunings pass through.
        let mut tuned = wiring.clone();
        tuned.tunings.env.insert("WINEPREFIX".into(), "/evil".into());
        tuned.tunings.env.insert("WINEDEBUG".into(), "+relay".into());
        tuned
            .tunings
            .env
            .insert("WINEDLLOVERRIDES".into(), "d3d11=n".into());
        tuned.tunings.env.insert("FOO".into(), "bar".into());
        let (_, set) = resolve_launch_env_with(&tuned.tunings, &tuned.runtime.arch, &prefix, &[]);
        let map: std::collections::BTreeMap<String, String> = set
            .into_iter()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
            .collect();
        assert_eq!(map["WINEPREFIX"], prefix.display().to_string());
        assert_eq!(map["WINEDEBUG"], "+relay");
        assert!(!map.contains_key("WINEDLLOVERRIDES"));
        assert_eq!(map["FOO"], "bar");
    }

    #[test]
    fn prepare_needs_no_lutris_database() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        // Deliberately no lutris_home: no pga.db anywhere.
        let home = tempfile::tempdir().unwrap();
        let log = home.path().join("wine.log");
        logging_wine(&home, "wine-ge-9-2", &log);
        let instance = apply_fixture(&root, home.path());
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        // Prefix created, runner recorded, Lutris never touched.
        assert!(_envelope.path().join("octowow-prefix/system.reg").is_file());
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(_envelope.path().join("octo-manager/wiring-runtime.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(record["version"], "wine-ge-9-2");
        assert!(!home.path().join(".local/share/lutris/pga.db").exists());
        assert!(!home.path().join(".config/lutris").exists());
        // Repeat prepare is a full no-op.
        let before_home = snapshot_tree(home.path());
        let before_game = snapshot_tree(_envelope.path());
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        assert_eq!(snapshot_tree(home.path()), before_home);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
    }

    #[test]
    fn launch_runs_game_with_declared_environment() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        let log = home.path().join("wine.log");
        logging_wine(&home, "wine-ge-9-2", &log);
        let instance = apply_fixture(&root, home.path());
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        launch("test", &instance, &dirs(&home), NativeTarget::Game, LaunchMode::Vanilla).unwrap();
        let logged = fs::read_to_string(&log).unwrap();
        let prefix = root.parent().unwrap().join("octowow-prefix");
        assert!(logged.contains(&format!("pwd={}", root.display())));
        assert!(logged.contains(&format!("arg={}", root.join("VanillaFixes.exe").display())));
        assert!(logged.contains(&format!("WINEPREFIX={}", prefix.display())));
        assert!(logged.contains("WINEDEBUG=-all"));
        assert!(logged.contains("WINEDLLOVERRIDES=d3d9=n,b"));
    }

    #[test]
    fn launch_requires_preparation_first() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        let instance = apply_fixture(&root, home.path());
        // No prepare ran: no recorded runner, no prefix.
        let err = launch(
            "test",
            &instance,
            &dirs(&home),
            NativeTarget::Game,
            LaunchMode::Vanilla,
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("prepare first"),
            "unexpected: {err:#}"
        );
    }

    #[test]
    fn launch_game_requires_declared_hd_set_in_every_mode() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        let instance = apply_fixture(&root, home.path());
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        // patch-A.mpq is present in the fixture: both modes launch.
        launch("test", &instance, &dirs(&home), NativeTarget::Game, LaunchMode::Hd).unwrap();
        launch(
            "test",
            &instance,
            &dirs(&home),
            NativeTarget::Game,
            LaunchMode::Vanilla,
        )
        .unwrap();
        // Without the payload, both modes fail closed: present HD MPQs load
        // in every mode, so there is no vanilla downgrade.
        fs::remove_file(root.join("Data/patch-A.mpq")).unwrap();
        for mode in [LaunchMode::Hd, LaunchMode::Vanilla] {
            let err =
                launch("test", &instance, &dirs(&home), NativeTarget::Game, mode).unwrap_err();
            assert!(
                format!("{err:#}").contains("HD not ready") && format!("{err:#}").contains("hd-patch:A"),
                "unexpected for {mode:?}: {err:#}"
            );
        }
    }

    #[test]
    fn launch_hd_mode_without_declared_set_fails_actionably() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        let mut instance = apply_fixture(&root, home.path());
        instance.wiring.as_mut().unwrap().data_patches = None;
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        let err = launch("test", &instance, &dirs(&home), NativeTarget::Game, LaunchMode::Hd)
            .unwrap_err();
        assert!(
            format!("{err:#}").contains("declared data_patches"),
            "unexpected: {err:#}"
        );
        // Vanilla with no declared set still launches (nothing approved to require).
        launch(
            "test",
            &instance,
            &dirs(&home),
            NativeTarget::Game,
            LaunchMode::Vanilla,
        )
        .unwrap();
    }

    #[test]
    fn launch_missing_executable_fails_before_exec() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        let instance = apply_fixture(&root, home.path());
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        fs::remove_file(root.join("VanillaFixes.exe")).unwrap();
        let err = launch(
            "test",
            &instance,
            &dirs(&home),
            NativeTarget::Game,
            LaunchMode::Vanilla,
        )
        .unwrap_err();
        let text = format!("{err:#}");
        assert!(
            text.contains("client files not ready") && text.contains("client-file:VanillaFixes.exe"),
            "unexpected: {text}"
        );
        assert!(!home.path().join("wine.log").exists());
    }

    #[test]
    fn launch_gates_drifted_client_but_native_preparation_does_not() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        let log = home.path().join("wine.log");
        logging_wine(&home, "wine-ge-9-2", &log);
        let mut instance = apply_fixture(&root, home.path());
        fs::write(root.join("WoW.exe"), "fake-wow").unwrap();
        instance
            .wiring
            .as_mut()
            .unwrap()
            .client_integrity
            .as_mut()
            .unwrap()
            .wow_exe = Some(
            serde_json::from_value(serde_json::json!({
                "size": 999,
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }))
            .unwrap(),
        );
        // Preparation never gated on the digest: the native pipeline
        // records the runner and creates the prefix.
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        // Execution does: neither launch mode runs a drifted client...
        for mode in [LaunchMode::Vanilla, LaunchMode::Hd] {
            let err =
                launch("test", &instance, &dirs(&home), NativeTarget::Game, mode).unwrap_err();
            assert!(
                format!("{err:#}").contains("wow-exe"),
                "unexpected for {mode:?}: {err:#}"
            );
        }
        // ...so wine never executed the game.
        assert!(!log.exists());
    }

    #[test]
    fn game_yml_carries_launch_gate_prefix() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let mut instance = fixture_instance(&root);
        let wiring = resolve_wiring(&instance).unwrap();
        let runner = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let yml = render_lutris_yml(
            "test",
            &instance,
            wiring.lutris.as_ref().unwrap(),
            &runner,
            &wiring,
        )
        .unwrap();
        assert!(
            yml.contains("prefix_command: modde-manager onboard gate --instance test --"),
            "gate prefix missing from rendered yml:\n{yml}"
        );
        // The effective entry verifies against the same synthesized spec.
        let dir = home.path().join(".config/lutris/games");
        fs::create_dir_all(&dir).unwrap();
        let yml_path = dir.join("octowow-test.yml");
        fs::write(&yml_path, &yml).unwrap();
        let checked = check_lutris_yml(
            "test",
            &yml_path,
            &instance,
            &wiring,
            wiring.lutris.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(checked.state, ItemState::Verified);
        // Dropping the gate is drift the check names explicitly.
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        let mut map = parsed.as_mapping().unwrap().clone();
        let system_key = serde_yaml_ng::Value::String("system".into());
        let mut system_map = map
            .remove(&system_key)
            .unwrap()
            .as_mapping()
            .unwrap()
            .clone();
        let prefix_key = serde_yaml_ng::Value::String("prefix_command".into());
        system_map.remove(&prefix_key);
        map.insert(system_key, serde_yaml_ng::Value::Mapping(system_map));
        fs::write(
            &yml_path,
            serde_yaml_ng::to_string(&serde_yaml_ng::Value::Mapping(map)).unwrap(),
        )
        .unwrap();
        let checked = check_lutris_yml(
            "test",
            &yml_path,
            &instance,
            &wiring,
            wiring.lutris.as_ref().unwrap(),
        )
        .unwrap();
        assert_eq!(checked.state, ItemState::Mismatched);
        assert!(checked.detail.contains("prefix_command"));
        // A declared wrapper stays in front of the synthesized gate.
        instance
            .wiring
            .as_mut()
            .unwrap()
            .lutris
            .as_mut()
            .unwrap()
            .command_prefix = Some("/bin/octowow-stdio-redir".into());
        let wrapped = resolve_wiring(&instance).unwrap();
        let wrapped_yml = render_lutris_yml(
            "test",
            &instance,
            wrapped.lutris.as_ref().unwrap(),
            &runner,
            &wrapped,
        )
        .unwrap();
        assert!(
            wrapped_yml.contains(
                "prefix_command: /bin/octowow-stdio-redir modde-manager onboard gate --instance test --"
            ),
            "declared wrapper not composed in front of the gate:\n{wrapped_yml}"
        );
    }

    #[test]
    fn gate_prefix_rejects_unsafe_instance_names() {
        assert_eq!(
            game_gate_prefix("octowow", None).unwrap(),
            "modde-manager onboard gate --instance octowow --"
        );
        assert_eq!(
            game_gate_prefix("octowow", Some("/bin/wrap")).unwrap(),
            "/bin/wrap modde-manager onboard gate --instance octowow --"
        );
        assert!(game_gate_prefix("has space", None).is_err());
        assert!(game_gate_prefix("a\"b", None).is_err());
        assert!(game_gate_prefix("", None).is_err());
    }

    #[test]
    fn gate_requires_a_command_before_readiness() {
        // Fixture-independent: an empty command is a wiring error reported
        // before any readiness observation, and never reaches exec.
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let err = gate("test", &instance, &dirs(&home), &[]).unwrap_err();
        assert!(
            format!("{err:#}").contains("no command"),
            "unexpected: {err:#}"
        );
    }

    #[test]
    fn gate_blocks_drifted_client_without_exec() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        let mut instance = apply_fixture(&root, home.path());
        fs::write(root.join("WoW.exe"), "fake-wow").unwrap();
        instance
            .wiring
            .as_mut()
            .unwrap()
            .client_integrity
            .as_mut()
            .unwrap()
            .wow_exe = Some(
            serde_json::from_value(serde_json::json!({
                "size": 999,
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            }))
            .unwrap(),
        );
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        // A nonexistent program keeps a mis-ordered gate fail-safe: exec
        // would ENOENT instead of replacing the test process, and the
        // readiness refusal still surfaces first.
        let err = gate(
            "test",
            &instance,
            &dirs(&home),
            &[std::ffi::OsString::from(
                "/nonexistent/modde-gate-must-not-exec",
            )],
        )
        .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("wow-exe"), "unexpected: {text}");
        assert!(
            text.contains("operator-confirmed"),
            "hint missing: {text}"
        );
    }

    #[test]
    fn mpq_lookup_is_case_insensitive_without_renames() {
        let (_envelope, root) = fixture_root();
        // Mixed-case basenames satisfy their letters; nothing is renamed.
        fs::write(root.join("Data/Patch-E.mpq"), "hd-e").unwrap();
        fs::write(root.join("Data/patch-F.MPQ"), "hd-f").unwrap();
        let names = data_entry_names(&root).unwrap().unwrap();
        assert_eq!(
            resolve_hd_letter(&root, &names, "E").unwrap(),
            Some("Patch-E.mpq".into())
        );
        assert_eq!(
            resolve_hd_letter(&root, &names, "F").unwrap(),
            Some("patch-F.MPQ".into())
        );
        // Letter case is insignificant too: `e` satisfies the same file.
        assert_eq!(
            resolve_hd_letter(&root, &names, "e").unwrap(),
            Some("Patch-E.mpq".into())
        );
        // Status reports the on-disk spelling, and the tree is untouched.
        let mut instance = fixture_instance(&root);
        instance.wiring.as_mut().unwrap().data_patches =
            Some(serde_json::from_value(serde_json::json!({
                "native_letters": ["E", "F"], "forbid_renames": true,
            })).unwrap());
        let home = fixture_home(&["wine-ge-9-2"]);
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let detail = items.iter().find(|i| i.name == "hd-patch:E").unwrap().detail.clone();
        assert!(detail.contains("Patch-E.mpq"), "unexpected: {detail}");
        assert_eq!(
            items.iter().find(|i| i.name == "hd-patch:F").unwrap().state,
            ItemState::Verified
        );
        assert!(root.join("Data/Patch-E.mpq").is_file());
        assert!(root.join("Data/patch-F.MPQ").is_file());
    }

    #[test]
    fn mpq_case_collision_and_symlink_fail_closed() {
        let (_envelope, root) = fixture_root();
        fs::write(root.join("Data/patch-E.mpq"), "lower").unwrap();
        fs::write(root.join("Data/Patch-E.mpq"), "upper").unwrap();
        let names = data_entry_names(&root).unwrap().unwrap();
        let err = find_mpq_actual(&names, "E").unwrap_err();
        assert!(format!("{err:#}").contains("ambiguous"), "unexpected: {err:#}");
        // Status surfaces the same ambiguity as a Mismatched readiness
        // finding (registration proceeds, execution refuses) instead of a
        // hard error or picking a spelling.
        let mut instance = fixture_instance(&root);
        instance.wiring.as_mut().unwrap().data_patches =
            Some(serde_json::from_value(serde_json::json!({
                "native_letters": ["E"], "forbid_renames": true,
            })).unwrap());
        let home = fixture_home(&["wine-ge-9-2"]);
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let found = items.iter().find(|i| i.name == "hd-patch:E").unwrap();
        assert_eq!(found.state, ItemState::Mismatched);
        assert!(found.detail.contains("ambiguous"), "unexpected: {}", found.detail);
        // A symlink with the right name never counts as present: same
        // itemized treatment.
        let (_envelope2, root2) = fixture_root();
        fs::write(root2.join("Data/real-E.mpq"), "hd").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root2.join("Data/real-E.mpq"),
            root2.join("Data/patch-E.mpq"),
        )
        .unwrap();
        let names2 = data_entry_names(&root2).unwrap().unwrap();
        assert!(resolve_hd_letter(&root2, &names2, "E").is_err());
        let mut sym_instance = fixture_instance(&root2);
        sym_instance.wiring.as_mut().unwrap().data_patches =
            Some(serde_json::from_value(serde_json::json!({
                "native_letters": ["E"], "forbid_renames": true,
            })).unwrap());
        let sym_items = status("test", &sym_instance, &dirs(&home)).unwrap();
        let sym_found = sym_items.iter().find(|i| i.name == "hd-patch:E").unwrap();
        assert_eq!(sym_found.state, ItemState::Mismatched);
    }

    #[test]
    fn registration_proceeds_despite_readiness_but_launch_refuses() {
        // Missing required file: status reports Missing, registration
        // still writes the declared entry, launch/gate refuse without exec.
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        fs::remove_file(root.join("VanillaFixes.exe")).unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let missing = items
            .iter()
            .find(|i| i.name == "client-file:VanillaFixes.exe")
            .unwrap();
        assert_eq!(missing.state, ItemState::Missing);
        // Registration owns yml+row only: readiness findings never block it.
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        // Execution refuses before wine runs (logging_wine would have
        // created wine.log on exec; its absence proves no exec).
        let log = home.path().join("wine.log");
        let err = launch(
            "test",
            &instance,
            &dirs(&home),
            NativeTarget::Game,
            LaunchMode::Vanilla,
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("client files not ready"),
            "unexpected: {err:#}"
        );
        assert!(!log.exists());
        let gate_err = gate(
            "test",
            &instance,
            &dirs(&home),
            &[std::ffi::OsString::from(
                "/nonexistent/modde-gate-must-not-exec",
            )],
        )
        .unwrap_err();
        assert!(
            format!("{gate_err:#}").contains("client files not ready"),
            "unexpected: {gate_err:#}"
        );
    }

    #[test]
    fn symlinked_required_file_registers_but_blocks_launch() {
        // Symlinked required file: Mismatched item, registration proceeds,
        // launch refuses without exec.
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        fs::remove_file(root.join("d3d9.dll")).unwrap();
        fs::write(root.join("real-d3d9.dll"), "hd").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.join("real-d3d9.dll"),
            root.join("d3d9.dll"),
        )
        .unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        assert_eq!(
            items
                .iter()
                .find(|i| i.name == "client-file:d3d9.dll")
                .unwrap()
                .state,
            ItemState::Mismatched
        );
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        let log = home.path().join("wine.log");
        assert!(
            format!(
                "{:#}",
                launch(
                    "test",
                    &instance,
                    &dirs(&home),
                    NativeTarget::Game,
                    LaunchMode::Vanilla,
                )
                .unwrap_err()
            )
            .contains("client files not ready")
        );
        assert!(!log.exists());
    }

    #[test]
    fn ambiguous_mpq_registers_but_blocks_launch_and_gate() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        // apply_fixture declares patch-A; collide on that same letter so
        // both registration and HD launch observe the ambiguity.
        fs::write(root.join("Data/Patch-A.mpq"), "upper").unwrap();
        fs::write(root.join("Data/patch-A.mpq"), "lower").unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        assert_eq!(
            items
                .iter()
                .find(|i| i.name == "hd-patch:A")
                .unwrap()
                .state,
            ItemState::Mismatched
        );
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        for mode in [LaunchMode::Hd, LaunchMode::Vanilla] {
            let err =
                launch("test", &instance, &dirs(&home), NativeTarget::Game, mode).unwrap_err();
            assert!(
                format!("{err:#}").contains("HD not ready"),
                "unexpected for {mode:?}: {err:#}"
            );
        }
        let gate_err = gate(
            "test",
            &instance,
            &dirs(&home),
            &[std::ffi::OsString::from(
                "/nonexistent/modde-gate-must-not-exec",
            )],
        )
        .unwrap_err();
        assert!(
            format!("{gate_err:#}").contains("HD not ready"),
            "unexpected: {gate_err:#}"
        );
        // wine.log absence proves the native launch loop above exec'd no
        // wine; the gate refusal itself is proven by the readiness error
        // (the nonexistent sentinel command would ENOENT on mis-order).
        assert!(!home.path().join("wine.log").exists());
    }

    #[test]
    fn unreadable_data_inventory_registers_but_blocks_launch() {
        // Data/ replaced by a regular file: Anchor::open requires
        // O_DIRECTORY, so the single shared inventory fails with ENOTDIR
        // (root-proof, unlike permission bits under root). Status reports
        // Unverifiable readiness, registration proceeds, execution refuses.
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        fs::remove_dir_all(root.join("Data")).unwrap();
        fs::write(root.join("Data"), "not-a-directory").unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let letter = items
            .iter()
            .find(|i| i.name == "hd-patch:A")
            .unwrap();
        assert_eq!(letter.state, ItemState::Unverifiable);
        assert!(letter.detail.contains("unreadable"), "unexpected: {}", letter.detail);
        let strays = items
            .iter()
            .find(|i| i.name == "hd-patch-letters")
            .unwrap();
        assert_eq!(strays.state, ItemState::Unverifiable);
        // Registration owns yml+row only: inventory failure never blocks it.
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        for mode in [LaunchMode::Hd, LaunchMode::Vanilla] {
            let err =
                launch("test", &instance, &dirs(&home), NativeTarget::Game, mode).unwrap_err();
            assert!(
                format!("{err:#}").contains("HD not ready"),
                "unexpected for {mode:?}: {err:#}"
            );
        }
        let gate_err = gate(
            "test",
            &instance,
            &dirs(&home),
            &[std::ffi::OsString::from(
                "/nonexistent/modde-gate-must-not-exec",
            )],
        )
        .unwrap_err();
        assert!(
            format!("{gate_err:#}").contains("HD not ready"),
            "unexpected: {gate_err:#}"
        );
        // Same split as above: wine.log covers the native launch loop, the
        // readiness error covers the gate (nonexistent command is fail-safe).
        assert!(!home.path().join("wine.log").exists());
    }

    #[test]
    fn launcher_tunings_override_game_without_coupling() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        // Settings fixture creates the launcher prefix; the game executable
        // and prefix markers are added below for target resolution.
        let mut instance = launcher_fixture(&root, None, true, true);
        fs::write(root.join("VanillaFixes.exe"), "fake").unwrap();
        // Game keeps the bundled-d3d9 policy (DXVK off); the launcher opts
        // into DXVK explicitly plus its own debug value.
        instance.wiring.as_mut().unwrap().launcher.as_mut().unwrap().tunings =
            serde_json::from_value(serde_json::json!({
                "dxvk": true,
                "env": {"WINEDEBUG": "+fps"},
            }))
            .unwrap();
        let wiring = resolve_wiring(&instance).unwrap();
        assert!(!wiring.tunings.dxvk);
        let effective = launcher_effective_tunings(&wiring);
        assert!(effective.dxvk);
        assert!(!effective.vkd3d);
        assert_eq!(effective.env.get("WINEDEBUG").unwrap(), "+fps");
        // Explicit false overrides an inherited true (field-level, so the
        // dxvk override above survives).
        instance.wiring.as_mut().unwrap().tunings.esync = Some(true);
        instance.wiring.as_mut().unwrap().launcher.as_mut().unwrap().tunings.esync = Some(false);
        let rewired = resolve_wiring(&instance).unwrap();
        assert!(rewired.tunings.esync);
        assert!(!launcher_effective_tunings(&rewired).esync);
        // Rendered entries carry their own target's values through the same
        // spec the check verifies.
        let runner = discover_runner(&dirs(&home), "wine", "latest").unwrap();
        let launcher = rewired.launcher.clone().unwrap();
        let spec = launcher_spec(
            launcher.installer.as_ref().unwrap(),
            &launcher.prefix.clone().unwrap(),
            launcher.lutris.as_ref().unwrap(),
            &runner,
            &rewired,
            launcher.executable.as_ref(),
        )
        .unwrap();
        assert!(spec.dxvk);
        assert!(!spec.esync);
        let yml = render_entry_yml(&spec).unwrap();
        assert!(yml.contains("dxvk: true"));
        let game = game_spec(
            "test",
            &instance,
            rewired.lutris.as_ref().unwrap(),
            &runner,
            &rewired,
        )
        .unwrap();
        assert!(!game.dxvk);
        assert!(game.esync);
        // Native targets share env parity only: Lutris wine toggles stay
        // Lutris-scoped and never become native Wine variables (game prefix
        // marker only; the launcher prefix already exists via the settings
        // fixture).
        let game_prefix = root.parent().unwrap().join("octowow-prefix");
        fs::create_dir_all(&game_prefix).unwrap();
        fs::write(game_prefix.join("system.reg"), "#arch=win64\n").unwrap();
        let game_lt = resolve_launch_target(&instance, &rewired, NativeTarget::Game).unwrap();
        let launcher_lt =
            resolve_launch_target(&instance, &rewired, NativeTarget::Launcher).unwrap();
        // The stored tunings still differ per target (Lutris scope)...
        assert!(!game_lt.tunings.dxvk);
        assert!(launcher_lt.tunings.dxvk);
        // ...but the native environments differ only by env map, never by
        // Lutris toggles: flipping dxvk alone changes no native variable.
        let game_env = resolve_launch_env_with(
            &game_lt.tunings,
            &game_lt.arch,
            &game_lt.prefix,
            &game_lt.dll_overrides,
        );
        let launcher_env = resolve_launch_env_with(
            &launcher_lt.tunings,
            &launcher_lt.arch,
            &launcher_lt.prefix,
            &launcher_lt.dll_overrides,
        );
        let to_map = |set: Vec<(std::ffi::OsString, std::ffi::OsString)>| {
            set.into_iter()
                .map(|(k, v)| {
                    (
                        k.to_string_lossy().into_owned(),
                        v.to_string_lossy().into_owned(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>()
        };
        // Launcher carries its own WINEDEBUG; game keeps the preset default.
        assert_eq!(
            to_map(game_env.1).get("WINEDEBUG").map(String::as_str),
            Some("-all")
        );
        assert_eq!(
            to_map(launcher_env.1).get("WINEDEBUG").map(String::as_str),
            Some("+fps")
        );
        // Toggling only dxvk/vkd3d/esync/fsync leaves the native env
        // byte-identical: proof the toggles are Lutris-scoped.
        let mut toggled = rewired.clone();
        toggled.tunings.dxvk = !toggled.tunings.dxvk;
        toggled.tunings.vkd3d = !toggled.tunings.vkd3d;
        toggled.tunings.esync = !toggled.tunings.esync;
        toggled.tunings.fsync = !toggled.tunings.fsync;
        let base_set = resolve_launch_env_with(
            &rewired.tunings,
            &rewired.runtime.arch,
            &game_lt.prefix,
            &rewired.dll_overrides,
        )
        .1;
        let toggled_set = resolve_launch_env_with(
            &toggled.tunings,
            &toggled.runtime.arch,
            &game_lt.prefix,
            &toggled.dll_overrides,
        )
        .1;
        assert_eq!(to_map(base_set), to_map(toggled_set));
    }

    #[test]
    fn gate_enforces_full_game_readiness_without_exec() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        logging_wine(&home, "wine-ge-9-2", &home.path().join("wine.log"));
        let instance = apply_fixture(&root, home.path());
        prepare_native("test", &instance, &dirs(&home), false).unwrap();
        // Missing HD blocks the gate even though the client digest is fine.
        // Refusal is proven by the readiness error (nonexistent command is
        // fail-safe); wine.log absence is consistency (gate execs the
        // supplied command, never wine directly).
        fs::remove_file(root.join("Data/patch-A.mpq")).unwrap();
        let err = gate(
            "test",
            &instance,
            &dirs(&home),
            &[std::ffi::OsString::from(
                "/nonexistent/modde-gate-must-not-exec",
            )],
        )
        .unwrap_err();
        let text = format!("{err:#}");
        assert!(text.contains("HD not ready"), "unexpected: {text}");
        assert!(!home.path().join("wine.log").exists());
    }

    #[test]
    fn validate_declaration_accepts_good_and_rejects_bad_shapes() {
        let (_envelope, root) = fixture_root();
        let instance = apply_fixture(&root, root.parent().unwrap());
        validate_declaration("test", &instance).unwrap();
        // Duplicate letters case-insensitively, path escapes, structural
        // env keys, and bad digests all fail with actionable errors.
        let mut bad = instance.clone();
        bad.wiring.as_mut().unwrap().data_patches =
            Some(serde_json::from_value(serde_json::json!({
                "native_letters": ["E", "e"], "forbid_renames": true,
            })).unwrap());
        assert!(validate_declaration("test", &bad).is_err());
        let mut bad = instance.clone();
        bad.wiring.as_mut().unwrap().launch = serde_json::from_value(serde_json::json!({
            "executable": "../evil.exe",
        })).unwrap();
        assert!(validate_declaration("test", &bad).is_err());
        let mut bad = instance.clone();
        bad.wiring.as_mut().unwrap().tunings.env = Some(
            [("WINEPREFIX".to_string(), Some("/evil".to_string()))]
                .into_iter()
                .collect(),
        );
        assert!(validate_declaration("test", &bad).is_err());
        let mut bad = instance.clone();
        bad.wiring.as_mut().unwrap().client_integrity.as_mut().unwrap().wow_exe =
            Some(serde_json::from_value(serde_json::json!({
                "size": 10, "sha256": "not-hex",
            })).unwrap());
        assert!(validate_declaration("test", &bad).is_err());
        assert!(validate_declaration("has space", &apply_fixture(&root, root.parent().unwrap())).is_err());
        // Parent traversal hidden inside an absolute path: lexical
        // containment would otherwise approve `/prefix/../outside`.
        // Uses the launcher fixture (apply_fixture declares no launcher).
        let (_lenvelope, lroot) = fixture_root();
        let linstance = launcher_fixture(&lroot, None, true, false);
        validate_declaration("test", &linstance).unwrap();
        let mut bad = linstance.clone();
        let evil_prefix = lroot.parent().unwrap().join("octo-launcher/prefix");
        let evil_exe = evil_prefix.join("../outside.exe");
        bad.wiring.as_mut().unwrap().launcher.as_mut().unwrap().prefix =
            Some(evil_prefix);
        bad.wiring.as_mut().unwrap().launcher.as_mut().unwrap().executable =
            Some(evil_exe);
        let err = validate_declaration("test", &bad).unwrap_err();
        assert!(
            format!("{err:#}").contains(".."),
            "unexpected: {err:#}"
        );
        // Game and launcher entries share the yml/pga slug namespace.
        let mut bad = linstance.clone();
        let game_slug = bad
            .wiring
            .as_ref()
            .unwrap()
            .lutris
            .as_ref()
            .unwrap()
            .slug
            .clone();
        bad.wiring.as_mut().unwrap().launcher.as_mut().unwrap().lutris =
            Some(serde_json::from_value(serde_json::json!({
                "slug": game_slug, "name": "Collision",
            })).unwrap());
        let err = validate_declaration("test", &bad).unwrap_err();
        assert!(
            format!("{err:#}").contains("must differ"),
            "unexpected: {err:#}"
        );
        // An explicitly empty HD approval carries no operator meaning.
        let mut bad = instance.clone();
        bad.wiring.as_mut().unwrap().data_patches =
            Some(serde_json::from_value(serde_json::json!({
                "native_letters": [], "forbid_renames": true,
            })).unwrap());
        let err = validate_declaration("test", &bad).unwrap_err();
        assert!(
            format!("{err:#}").contains("declares no letters"),
            "unexpected: {err:#}"
        );
    }

    #[test]
    fn resolve_launch_target_separates_installer_and_launcher() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = launcher_fixture(&root, None, true, true);
        let wiring = resolve_wiring(&instance).unwrap();
        // The fixture prefix exists (win64 marker), so both resolve.
        let installer = resolve_launch_target(&instance, &wiring, NativeTarget::Installer).unwrap();
        let installer_path = root.parent().unwrap().join("OctoLauncher_Installer.exe");
        assert_eq!(installer.exe, installer_path);
        assert_eq!(installer.dir, root.parent().unwrap().to_path_buf());
        assert!(installer.dll_overrides.is_empty());
        let launcher = resolve_launch_target(&instance, &wiring, NativeTarget::Launcher).unwrap();
        let exe = wiring.launcher.as_ref().unwrap().executable.clone().unwrap();
        assert_eq!(launcher.exe, exe);
        assert_eq!(launcher.dir, exe.parent().unwrap().to_path_buf());
        assert!(launcher.dll_overrides.is_empty());
    }

    #[test]
    fn desktop_entry_render_quotes_invocation_and_args() {
        let body = render_desktop_entry(
            "test",
            "OctoWoW",
            Path::new("/games/modde-manager"),
            Path::new("/games/manager-config.json"),
        )
        .unwrap();
        let exec = body
            .lines()
            .find(|line| line.starts_with("Exec="))
            .unwrap();
        // Quoted binary and config (never split on spaces), then the exact
        // native-launch arguments — a working invocation without the wrapper.
        assert_eq!(
            exec,
            "Exec=\"/games/modde-manager\" --config \"/games/manager-config.json\" onboard launch --instance test --target game"
        );
        assert!(
            render_desktop_entry("has space", "X", Path::new("/b"), Path::new("/c")).is_err()
        );
    }

    #[test]
    fn desktop_entry_writes_and_verifies() {        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        let instance = fixture_instance(&root);
        let config = _envelope.path().join("manager-config.json");
        fs::write(&config, "{}").unwrap();
        desktop_entry("test", &instance, &dirs(&home), &config).unwrap();
        let target = home
            .path()
            .join(".local/share/applications/octowow-test-modde.desktop");
        let body = fs::read_to_string(&target).unwrap();
        assert!(body.contains("[Desktop Entry]"));
        assert!(body.contains("Name=OctoWoW"));
        assert!(body.contains("onboard launch --instance test --target game"));
        // Repeat is a full no-op.
        let before_home = snapshot_tree(home.path());
        let before_game = snapshot_tree(_envelope.path());
        desktop_entry("test", &instance, &dirs(&home), &config).unwrap();
        assert_eq!(fs::read(&target).unwrap().as_slice(), body.as_bytes());
        assert_eq!(snapshot_tree(home.path()), before_home);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
    }

    #[test]
    fn slug_validation_rejects_injection() {
        assert!(validate_slug("octowow-test").is_ok());
        assert!(validate_slug("a'; DROP TABLE games;--").is_err());
        assert!(validate_slug("../../etc").is_err());
        assert!(validate_slug("").is_err());
    }

    #[test]
    fn prefix_inside_root_is_refused() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let mut instance = fixture_instance(&root);
        instance.wiring.as_mut().unwrap().prefix = Some(Prefix {
            path: root.join("octowow-prefix"),
        });
        assert!(status("test", &instance, &dirs(&home)).is_err());
        assert!(apply("test", &instance, &dirs(&home), false, false, None).is_err());
    }

    #[test]
    fn symlink_prefix_is_refused_before_any_write() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let real = _envelope.path().join("real-prefix");
        fs::create_dir_all(&real).unwrap();
        let link = _envelope.path().join("octowow-prefix");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let instance = fixture_instance(&root);
        assert!(status("test", &instance, &dirs(&home)).is_err());
        assert!(apply("test", &instance, &dirs(&home), false, false, None).is_err());
        // The symlink itself is untouched.
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    #[test]
    fn overlay_keeps_explicit_empty_false_and_independent_tunings() {
        let base: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
        }))
        .unwrap();
        // Explicit empty list clears the preset overrides.
        let cleared: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
            "wiring": {"dll_overrides": []},
        }))
        .unwrap();
        let resolved = resolve_wiring(&cleared).unwrap();
        assert!(resolved.dll_overrides.is_empty());
        // Explicit default-valued override is honored, siblings untouched.
        let flipped: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
            "wiring": {"tunings": {"dxvk": true}},
        }))
        .unwrap();
        let resolved = resolve_wiring(&flipped).unwrap();
        assert!(resolved.tunings.dxvk);
        assert!(resolved.tunings.esync); // preset sibling survives
        assert_eq!(resolved.tunings.env.get("WINEDEBUG").unwrap(), "-all");
        // One tuning alone never drags the others along.
        let single: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
            "wiring": {"tunings": {"esync": false}},
        }))
        .unwrap();
        let resolved = resolve_wiring(&single).unwrap();
        assert!(!resolved.tunings.esync);
        assert!(!resolved.tunings.dxvk); // preset value, not a default
        // Null env value deletes a preset key; new keys merge in.
        let env: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
            "wiring": {"tunings": {"env": {"WINEDEBUG": null, "FOO": "bar"}}},
        }))
        .unwrap();
        let resolved = resolve_wiring(&env).unwrap();
        assert!(!resolved.tunings.env.contains_key("WINEDEBUG"));
        assert_eq!(resolved.tunings.env.get("FOO").unwrap(), "bar");
        let _ = base;
    }

    #[test]
    fn missing_database_blocks_before_any_write() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        // Home has a runner and a games dir, but no pga.db at all.
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        fs::create_dir_all(home.path().join(".config/lutris/games")).unwrap();
        let instance = apply_fixture(&root, home.path());
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("no pga.db"),
            "unexpected: {err:#}"
        );
        assert!(
            !home
                .path()
                .join(".config/lutris/games/octowow-test.yml")
                .exists()
        );
        assert!(!_envelope.path().join("octowow-prefix").exists());
    }

    #[test]
    fn orphan_yml_requires_adopt() {
        let (_guard, _envelope, root, home) = apply_test_env();
        // Yml exists with foreign content, but no database row: orphan.
        let yml_path = home.path().join(".config/lutris/games/octowow-test.yml");
        fs::write(&yml_path, "game:\n  exe: /games/other/Game.exe\n").unwrap();
        let instance = apply_fixture(&root, home.path());
        let err = apply("test", &instance, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("--adopt"),
            "unexpected: {err:#}"
        );
        assert!(!_envelope.path().join("octowow-prefix").exists());
        // With adopt, the orphan is taken over and the full apply succeeds.
        apply("test", &instance, &dirs(&home), true, false, None).unwrap();
        assert!(
            status("test", &instance, &dirs(&home))
                .unwrap()
                .iter()
                .all(|item| item.state == ItemState::Verified)
        );
    }

    #[test]
    fn corrupt_record_fails_closed() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let state = _envelope.path().join("octo-manager");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("wiring-runtime.json"), "not json").unwrap();
        let items = status("test", &instance, &dirs(&home)).unwrap();
        let runner = items.iter().find(|i| i.name == "runner").unwrap();
        assert_eq!(runner.state, ItemState::Missing);
        assert!(
            runner.detail.contains("corrupt"),
            "unexpected: {}",
            runner.detail
        );
        assert!(apply("test", &instance, &dirs(&home), false, false, None).is_err());
    }

    #[test]
    fn missing_recorded_binary_blocks_until_reselect() {
        let _guard = APPLY_LOCK.lock().unwrap();
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        let wiring = resolve_wiring(&instance).unwrap();
        // Record points at a binary that no longer exists.
        let state = _envelope.path().join("octo-manager");
        fs::create_dir_all(&state).unwrap();
        fs::write(
            state.join("wiring-runtime.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": "wine-ge-9-2",
                "path": "/nonexistent/wine",
            }))
            .unwrap(),
        )
        .unwrap();
        let err = select_runner(&dirs(&home), &instance, &wiring, false).unwrap_err();
        assert!(
            format!("{err:#}").contains("--reselect"),
            "unexpected: {err:#}"
        );
        let (runner, recorded) = select_runner(&dirs(&home), &instance, &wiring, true).unwrap();
        assert_eq!(runner.version, "wine-ge-9-2");
        assert!(!recorded);
    }

    #[test]
    fn numbered_stock_patches_are_accepted() {
        let (_envelope, root) = fixture_root();
        let home = fixture_home(&["wine-ge-9-2"]);
        let instance = fixture_instance(&root);
        for name in ["patch-1.mpq", "patch-5.mpq", "patch.MPQ", "patch-A.mpq"] {
            fs::write(root.join("Data").join(name), "bytes").unwrap();
        }
        let items = status("test", &instance, &dirs(&home)).unwrap();
        assert!(items.iter().all(|i| i.name != "hd-patch-letters"));
    }

    #[test]
    fn yml_restored_when_database_step_fails() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        let yml_path = home.path().join(".config/lutris/games/octowow-test.yml");
        let good = fs::read(&yml_path).unwrap();
        // Rename the entry so the row update is attempted, then arm a
        // trigger that aborts the UPDATE after the yml already changed.
        // (File permissions would not stop a root test runner.)
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[
            db.display().to_string(),
            "CREATE TRIGGER lock_row BEFORE UPDATE ON games BEGIN SELECT RAISE(ABORT, 'locked'); END;"
                .into(),
        ])
        .unwrap();
        let mut renamed = instance.clone();
        renamed
            .wiring
            .as_mut()
            .unwrap()
            .lutris
            .as_mut()
            .unwrap()
            .name = "Renamed".into();
        let err = apply("test", &renamed, &dirs(&home), false, false, None).unwrap_err();
        assert!(
            format!("{err:#}").contains("database"),
            "unexpected: {err:#}"
        );
        assert_eq!(fs::read(&yml_path).unwrap(), good);
    }

    #[test]
    fn expect_runner_binds_plan_to_apply() {
        let (_guard, _envelope, root, home) = apply_test_env();
        let instance = apply_fixture(&root, home.path());
        let err = apply(
            "test",
            &instance,
            &dirs(&home),
            false,
            false,
            Some("wine-ge-9-9"),
        )
        .unwrap_err();
        assert!(
            format!("{err:#}").contains("differs from expected"),
            "unexpected: {err:#}"
        );
        assert!(!_envelope.path().join("octowow-prefix").exists());
        apply(
            "test",
            &instance,
            &dirs(&home),
            false,
            false,
            Some("wine-ge-9-2"),
        )
        .unwrap();
    }

    fn walk_paths(root: &Path) -> Vec<PathBuf> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path.clone());
                }
                out.push(path);
            }
        }
        out.sort();
        out
    }
}
