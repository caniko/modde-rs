//! Declarative runtime wiring for launchable game instances (OctoWoW HD/Lutris).
//!
//! `status` and `plan` are read-only: they report verified/missing/mismatched/
//! unverifiable items and never execute Wine, Lutris, or the launcher.
//! `apply` owns exactly three idempotent mutations: Wine prefix creation via
//! `wineboot`, the Lutris game yml, and the Lutris `pga.db` row (backup-first,
//! Lutris-closed gate). It never touches client binaries, MPQs, or game data.

use super::*;
use crate::transaction::overlap;
use files::{Anchor, Image, Lease, fd_path};
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Optional per-instance runtime wiring. All fields optional so existing
/// configs keep parsing; bumping `managerSchemaVersion` to 3 advertises it.
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
/// addon pins, user settings, endpoints) stays in the consumer declaration;
/// only reusable launch behavior lives here.
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
        data_patches: Some(DataPatches {
            native_letters: [
                "A", "B", "C", "D", "E", "G", "I", "L", "M", "N", "P", "S", "T", "U",
            ]
            .iter()
            .map(|s| s.to_string())
            .collect(),
            patch_a: None,
            forbid_renames: true,
        }),
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
/// exe, working dir, prefix, wine version/arch/toggles, DLL overrides, and
/// explicitly disabled anti-cheat runtimes.
fn check_lutris_yml(
    path: &Path,
    instance: &Instance,
    wiring: &Wiring,
    entry: &LutrisEntry,
) -> StatusItem {
    let fix = "run: onboard apply (rewrites this entry only)".to_string();
    let mismatch = |detail: String| item("lutris-yml", ItemState::Mismatched, detail, fix.clone());
    let body = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) => {
            return item(
                "lutris-yml",
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
    if let Some(problem) = expect_str(&["slug"], &entry.slug, "slug") {
        problems.push(problem);
    }
    let exe = instance
        .root
        .join(&wiring.launch.executable)
        .display()
        .to_string();
    let working_dir = instance.root.display().to_string();
    for (keys, want) in [
        (["game", "exe"], exe.as_str()),
        (["game", "working_dir"], working_dir.as_str()),
    ] {
        if let Some(problem) = expect_str(&keys, want, keys[1]) {
            problems.push(problem);
        }
    }
    if let Some(prefix) = &wiring.prefix {
        let want = prefix.path.display().to_string();
        if let Some(problem) = expect_str(&["game", "prefix"], &want, "prefix") {
            problems.push(problem);
        }
    }
    if let Some(problem) = expect_str(&["runner"], "wine", "runner") {
        problems.push(problem);
    }
    // No record yet: discovery decides, so the version check waits for apply.
    // A corrupt record fails the whole check (fail-closed, like selection).
    match read_recorded(instance) {
        Ok(Some(recorded)) => {
            if let Some(problem) =
                expect_str(&["wine", "version"], &recorded.version, "wine version")
            {
                problems.push(problem);
            }
        }
        Ok(None) => {}
        Err(e) => problems.push(format!("runtime record unreadable: {e:#}")),
    }
    let want_arch = if wiring.runtime.arch == "wow64" {
        "win64"
    } else {
        wiring.runtime.arch.as_str()
    };
    if let Some(problem) = expect_str(&["wine", "arch"], want_arch, "wine arch") {
        problems.push(problem);
    }
    for (key, want) in [
        ("dxvk", wiring.tunings.dxvk),
        ("vkd3d", wiring.tunings.vkd3d),
        ("esync", wiring.tunings.esync),
        ("fsync", wiring.tunings.fsync),
        ("eac", false),
        ("battleye", false),
    ] {
        match get(&["wine", key]).and_then(|v| v.as_bool()) {
            Some(have) if have == want => {}
            Some(have) => problems.push(format!("wine.{key} is {have}, want {want}")),
            None => problems.push(format!("wine.{key} missing, want {want}")),
        }
    }
    let overrides = get(&["system", "env", "WINEDLLOVERRIDES"]).and_then(|v| v.as_str());
    match overrides {
        Some(have) => {
            for dll in &wiring.dll_overrides {
                if !have.split(';').any(|entry| entry.trim() == dll) {
                    problems.push(format!("dll override missing: {dll}"));
                }
            }
        }
        None if !wiring.dll_overrides.is_empty() => {
            problems.push("WINEDLLOVERRIDES missing".into());
        }
        _ => {}
    }
    if problems.is_empty() {
        item(
            "lutris-yml",
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

/// Read-only readiness report. Never executes wine, Lutris, or the launcher.
pub fn status(name: &str, instance: &Instance, dirs: &HomeDirs) -> Result<Vec<StatusItem>> {
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
    // the executable.
    if let Some(integrity) = &wiring.client_integrity {
        for file in &integrity.require_files {
            match root_file_meta(&instance.root, file)? {
                Some(meta) => items.push(item(
                    &format!("client-file:{file}"),
                    ItemState::Verified,
                    format!("present ({} bytes)", meta.len()),
                    String::new(),
                )),
                None => items.push(item(
                    &format!("client-file:{file}"),
                    ItemState::Missing,
                    format!("{file} absent"),
                    "run the launcher Install/Verify, then re-check".into(),
                )),
            }
        }
        if let Some(expected) = &integrity.wow_exe {
            let rel = "WoW.exe";
            match pinned_file(&instance.root, rel)? {
                None => items.push(item(
                    "wow-exe",
                    ItemState::Missing,
                    format!("{rel} absent"),
                    "run the launcher Install/Verify".into(),
                )),
                Some((_, meta)) if meta.len() != expected.size => items.push(item(
                    "wow-exe",
                    ItemState::Mismatched,
                    format!("size={} want={}", meta.len(), expected.size),
                    "run the launcher Install/Verify for a clean LAA exe".into(),
                )),
                Some((mut file, _)) => {
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
                            "run the launcher Install/Verify for a clean LAA exe".into()
                        },
                    ));
                }
            }
        }
    }

    // HD data patches: metadata presence at native letters (payloads never
    // loaded); declared patch-A identity verified by streaming digest.
    if let Some(patches) = &wiring.data_patches {
        for letter in &patches.native_letters {
            if letter.len() != 1 || !letter.bytes().all(|c| c.is_ascii_alphanumeric()) {
                bail!("unsafe patch letter: {letter}");
            }
            let rel_lower = format!("Data/patch-{letter}.mpq");
            let rel_upper = format!("Data/patch-{letter}.MPQ");
            let found = match existing_name(&instance.root, &[&rel_lower, &rel_upper])? {
                Some(_) => true,
                None => false,
            };
            items.push(item(
                &format!("hd-patch:{letter}"),
                if found {
                    ItemState::Verified
                } else {
                    ItemState::Missing
                },
                if found {
                    "present at native letter".into()
                } else {
                    format!("{rel_lower} absent")
                },
                if found {
                    String::new()
                } else {
                    "enable the HD set in the launcher, then re-check".into()
                },
            ));
        }
        if let Some(expected) = &patches.patch_a {
            match existing_name(&instance.root, &["Data/patch-A.mpq", "Data/patch-A.MPQ"])? {
                None => items.push(item(
                    "hd-patch-A",
                    ItemState::Missing,
                    "no patch-A at all".into(),
                    "enable the HD set in the launcher".into(),
                )),
                Some(rel) => {
                    let Some((mut file, meta)) = pinned_file(&instance.root, &rel)? else {
                        bail!("patch vanished during check: {rel}")
                    };
                    // Size first (cheap reject), then a streaming digest.
                    let ok = meta.len() == expected.size
                        && stream_digest_file(&mut file)
                            .map(|(_, digest)| digest == expected.sha256.to_ascii_lowercase())
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

    // Rename dodge detection: single-letter patches outside the declared
    // native letters are known character-screen crash causes. Numbered
    // stock patches (patch-1..5.mpq) are always accepted. Names only —
    // payloads are never read (HD trees are gigabytes).
    if let Some(patches) = &wiring.data_patches {
        if patches.forbid_renames {
            match data_entry_names(&instance.root)? {
                Some(names) => {
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
                None => items.push(item(
                    "hd-patch-letters",
                    ItemState::Missing,
                    "no Data/ directory".into(),
                    "install the client before onboarding".into(),
                )),
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
        match read_capped(&instance.root, "realmlist.wtf", 1 << 20) {
            Ok(bytes) => {
                let body = String::from_utf8_lossy(&bytes);
                let missing: Vec<_> = endpoints
                    .assert_realmlist
                    .iter()
                    .filter(|line| !body.contains(line.as_str()))
                    .collect();
                items.push(item(
                    "endpoints",
                    if missing.is_empty() {
                        ItemState::Verified
                    } else {
                        ItemState::Mismatched
                    },
                    if missing.is_empty() {
                        "realmlist matches".into()
                    } else {
                        format!(
                            "missing lines: {}",
                            missing
                                .iter()
                                .map(|s| s.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    },
                    if missing.is_empty() {
                        String::new()
                    } else {
                        "update realmlist.wtf out-of-band (unmanaged file)".into()
                    },
                ));
            }
            Err(e) => items.push(item(
                "endpoints",
                ItemState::Missing,
                format!("{e:#}"),
                "restore realmlist.wtf from backup".into(),
            )),
        }
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
                items.push(check_lutris_yml(&path, instance, &wiring, entry));
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

    let _ = name;
    Ok(items)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WiringChangeKind {
    /// Onboard apply owns this transition.
    Change,
    /// External action (launcher update, client install) must happen first.
    Blocker,
}

#[derive(Debug, Clone, Serialize)]
pub struct WiringChange {
    pub resource: String,
    pub kind: WiringChangeKind,
    pub summary: String,
}

/// Items onboard apply owns; everything else non-verified is an external
/// blocker (client install, launcher maintenance) that must resolve first.
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

/// Render the deterministic Lutris game yml for this instance. Anti-cheat is
/// always absent: no EAC/BattleEye keys are ever emitted.
pub fn render_lutris_yml(
    instance: &Instance,
    entry: &LutrisEntry,
    runner: &Runner,
    wiring: &Wiring,
) -> Result<String> {
    let exe = instance.root.join(&wiring.launch.executable);
    let prefix = wiring
        .prefix
        .as_ref()
        .context("lutris entry needs a declared prefix path")?
        .path
        .display()
        .to_string();
    // ponytail: Vec pairs, not a map — yaml_ng::Value has no Ord.
    let mut env: Vec<(serde_yaml_ng::Value, serde_yaml_ng::Value)> = Vec::new();
    if !wiring.dll_overrides.is_empty() {
        env.push((
            serde_yaml_ng::Value::String("WINEDLLOVERRIDES".into()),
            serde_yaml_ng::Value::String(wiring.dll_overrides.join(";")),
        ));
    }
    for (key, value) in &wiring.tunings.env {
        if key == "WINEDLLOVERRIDES" {
            continue;
        }
        env.push((
            serde_yaml_ng::Value::String(key.clone()),
            serde_yaml_ng::Value::String(value.clone()),
        ));
    }
    // Lutris wine arch is win32|win64; WOW64-capable setups use a win64 prefix.
    let arch = if wiring.runtime.arch == "wow64" {
        "win64"
    } else {
        wiring.runtime.arch.as_str()
    };
    let doc = serde_yaml_ng::Mapping::from_iter([
        (
            serde_yaml_ng::Value::String("game".into()),
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter([
                (
                    serde_yaml_ng::Value::String("exe".into()),
                    serde_yaml_ng::Value::String(exe.display().to_string()),
                ),
                (
                    serde_yaml_ng::Value::String("working_dir".into()),
                    serde_yaml_ng::Value::String(instance.root.display().to_string()),
                ),
                (
                    serde_yaml_ng::Value::String("prefix".into()),
                    serde_yaml_ng::Value::String(prefix),
                ),
            ])),
        ),
        (
            serde_yaml_ng::Value::String("game_slug".into()),
            serde_yaml_ng::Value::String(entry.game_slug.clone()),
        ),
        (
            serde_yaml_ng::Value::String("name".into()),
            serde_yaml_ng::Value::String(entry.name.clone()),
        ),
        (
            serde_yaml_ng::Value::String("runner".into()),
            serde_yaml_ng::Value::String("wine".into()),
        ),
        (
            serde_yaml_ng::Value::String("slug".into()),
            serde_yaml_ng::Value::String(entry.slug.clone()),
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
                    serde_yaml_ng::Value::String(runner.version.clone()),
                ),
                (
                    serde_yaml_ng::Value::String("arch".into()),
                    serde_yaml_ng::Value::String(arch.into()),
                ),
                (
                    serde_yaml_ng::Value::String("dxvk".into()),
                    serde_yaml_ng::Value::Bool(wiring.tunings.dxvk),
                ),
                (
                    serde_yaml_ng::Value::String("vkd3d".into()),
                    serde_yaml_ng::Value::Bool(wiring.tunings.vkd3d),
                ),
                (
                    serde_yaml_ng::Value::String("esync".into()),
                    serde_yaml_ng::Value::Bool(wiring.tunings.esync),
                ),
                (
                    serde_yaml_ng::Value::String("fsync".into()),
                    serde_yaml_ng::Value::Bool(wiring.tunings.fsync),
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
            serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter([(
                serde_yaml_ng::Value::String("env".into()),
                serde_yaml_ng::Value::Mapping(serde_yaml_ng::Mapping::from_iter(env)),
            )])),
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
    if wiring.runtime.arch == "wow64" {
        "win64"
    } else {
        wiring.runtime.arch.as_str()
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
    exe: String,
    dir: String,
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
    let mut exe = String::new();
    let dir = instance.root.display().to_string();
    if let Some(entry) = &wiring.lutris {
        validate_slug(&entry.slug)?;
        if lutris_running()? {
            bail!("lutris is running; close it completely before onboarding");
        }
        exe = instance
            .root
            .join(&wiring.launch.executable)
            .display()
            .to_string();
        // The database must exist before anything is written: a missing
        // database is a blocker, not something apply works around. The yml
        // target is the config dir paired with that database. The data-dir
        // lock is taken before the row is even read, so a concurrent
        // onboard run cannot invalidate the ownership check below.
        let (site_config, site_data) =
            lutris_site(dirs).context("no pga.db found (start Lutris once, then close it)")?;
        let target = site_config.join(format!("{}.yml", entry.slug));
        let yml = render_lutris_yml(instance, entry, &runner, &wiring)?;
        db_guard = Some(
            Anchor::open(&site_data)
                .context("invalid Lutris data dir")
                .and_then(|anchor| {
                    anchor
                        .lock()
                        .context("another onboard run holds the Lutris database")
                })?,
        );
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
                parts.get(3) == Some(&dir.as_str()) && parts.get(4) == Some(&exe.as_str());
            if !same_identity && !adopt {
                bail!(
                    "lutris entry '{}' points elsewhere; pass --adopt to take it over",
                    entry.slug
                );
            }
        } else if target.is_file() && fs::read(&target)? != yml.as_bytes() && !adopt {
            bail!(
                "orphaned {}.yml with no database row; pass --adopt to take it over",
                entry.slug
            );
        }
        yml_target = Some(target);
        yml_body = yml;
    }

    // Client/HD prerequisites are external maintenance (launcher updates,
    // client install): anything apply does not own must already verify, or
    // no mutation happens at all. Same observations status/plan report.
    let blockers: Vec<_> = status(name, instance, dirs)?
        .into_iter()
        .filter(|item| item.state != ItemState::Verified && !is_ownable(&item.name))
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
        exe,
        dir,
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
/// creation, the Lutris yml, and the pga.db row — nothing else. Partial
/// state is retained with journaled evidence on failure, never deleted;
/// pre-existing content is restored from retained backups.
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
        let target_existed = target.is_file();
        let (changed, yml_backup) = write_yml(target, &prepared.yml_body)?;
        mutated |= changed;
        println!(
            "{name}: lutris yml {}",
            if changed {
                target.display().to_string()
            } else {
                "unchanged".into()
            }
        );
        let dir = prepared.dir.clone();
        match upsert_pga_row(dirs, entry, &prepared.exe, &dir) {
            Ok(action) => {
                mutated |= action != "unchanged";
                println!("{name}: lutris entry {action}");
            }
            Err(error) => {
                if let Some(backup) = &yml_backup {
                    let previous = fs::read(backup)?;
                    atomic_write_0600(target, &previous)?;
                    eprintln!("{name}: database step failed; yml restored; evidence retained");
                } else if !target_existed {
                    // First registration left an orphan: remove only what this
                    // apply created, and only if untouched since.
                    if fs::read(target).is_ok_and(|current| current == prepared.yml_body.as_bytes())
                    {
                        let _ = fs::remove_file(target);
                        eprintln!(
                            "{name}: database step failed; new yml removed; evidence retained"
                        );
                    } else {
                        eprintln!(
                            "{name}: database step failed; yml changed underneath, left in place"
                        );
                    }
                }
                journal_event(
                    instance,
                    name,
                    serde_json::json!({"op": "apply-failed", "step": "lutris-db", "error": format!("{error:#}")}),
                );
                return Err(error).context("lutris database update failed");
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
    if !bad.is_empty() {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "apply-failed", "step": "verify"}),
        );
        bail!(
            "post-apply verification failed: {}",
            bad.iter()
                .map(|item| format!("{}={:?}", item.name, item.state))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if mutated {
        journal_event(
            instance,
            name,
            serde_json::json!({"op": "apply-done", "runner": prepared.runner.version}),
        );
    }
    println!("{name}: onboard apply complete");
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
        // Preset-only declaration resolves to reusable Octo defaults.
        let preset_only: Instance = serde_json::from_value(serde_json::json!({
            "root": "/games/octo", "client": "wow-classic", "preset": "octowow-hd",
        }))
        .unwrap();
        let wiring = resolve_wiring(&preset_only).unwrap();
        assert_eq!(wiring.launch.executable, "VanillaFixes.exe");
        assert!(!wiring.tunings.dxvk); // bundled d3d9.dll, no second layer
        assert!(!wiring.runtime.anticheat);
        assert_eq!(wiring.lutris.as_ref().unwrap().slug, "octowow-community");
        assert!(
            wiring
                .data_patches
                .as_ref()
                .unwrap()
                .native_letters
                .contains(&"U".to_string())
        );
        assert!(wiring.prefix.is_none()); // site path stays consumer-owned
        assert!(wiring.endpoints.is_none());
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
        let yml = render_lutris_yml(&instance, wiring.lutris.as_ref().unwrap(), &runner, &wiring)
            .unwrap();
        let parsed: serde_yaml_ng::Value = serde_yaml_ng::from_str(&yml).unwrap();
        assert_eq!(parsed["runner"].as_str(), Some("wine"));
        assert_eq!(parsed["wine"]["version"].as_str(), Some("wine-ge-9-2"));
        assert_eq!(parsed["wine"]["arch"].as_str(), Some("win64")); // wow64 mapping
        assert_eq!(parsed["wine"]["vkd3d"].as_bool(), Some(false));
        assert_eq!(parsed["wine"]["eac"].as_bool(), Some(false));
        assert_eq!(parsed["wine"]["battleye"].as_bool(), Some(false));
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
            &yml_path,
            &instance,
            &wiring,
            wiring.lutris.as_ref().unwrap(),
        );
        assert_eq!(checked.state, ItemState::Verified);
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

    /// Full state capture (names, device, inode, mode, mtime, bytes) for
    /// no-write proofs: replacing a file with identical content still
    /// changes identity or timestamps and fails the comparison.
    fn snapshot_tree(root: &Path) -> Vec<(PathBuf, u64, u64, u32, i64, Option<Vec<u8>>)> {
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
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        lutris_home(home.path());
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

    #[test]
    fn first_registration_orphan_removed_on_database_failure() {
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        lutris_home(home.path());
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

    // Apply tests spawn sqlite3 with lutris paths in argv, which a sibling
    // test's `pgrep -f lutris` would mistake for a running Lutris. Serialize.
    static APPLY_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        lutris_home(home.path());
        // WAL mode: the backup path must stay consistent, unrelated rows kept.
        let db = home.path().join(".local/share/lutris/pga.db");
        sqlite3(&[db.display().to_string(), "PRAGMA journal_mode=WAL;".into()]).unwrap();
        sqlite3(&[
            db.display().to_string(),
            "INSERT INTO games (name, slug, runner, platform, directory, executable, configpath, installed) VALUES ('Unrelated', 'other-game', 'wine', 'Linux', '/games/other', '/games/other/g.exe', 'other-game', 1);".into(),
        ])
        .unwrap();
        let instance = apply_fixture(&root, home.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        let yml_path = home.path().join(".config/lutris/games/octowow-test.yml");
        let yml_before = fs::read(&yml_path).unwrap();
        let dump_before = db_dump(home.path());
        assert!(dump_before.contains("other-game|/games/other/g.exe"));
        // A newer runner appears: the recorded selection must not move, and
        // the repeat apply must not change any game or Lutris state at all.
        wineboot_home(&home, "wine-ge-10-1");
        let before_game = snapshot_tree(_envelope.path());
        let before_home = snapshot_tree(home.path());
        apply("test", &instance, &dirs(&home), false, false, None).unwrap();
        assert_eq!(fs::read(&yml_path).unwrap(), yml_before);
        assert_eq!(db_dump(home.path()), dump_before);
        assert_eq!(snapshot_tree(_envelope.path()), before_game);
        assert_eq!(snapshot_tree(home.path()), before_home);
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(_envelope.path().join("octo-manager/wiring-runtime.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(record["version"], "wine-ge-9-2");
        // Effective entry verifies: prefix, recorded runtime, anticheat off.
        let wiring = resolve_wiring(&instance).unwrap();
        let checked = check_lutris_yml(
            &yml_path,
            &instance,
            &wiring,
            wiring.lutris.as_ref().unwrap(),
        );
        assert_eq!(checked.state, ItemState::Verified);
        assert!(
            status("test", &instance, &dirs(&home))
                .unwrap()
                .iter()
                .all(|item| item.state == ItemState::Verified)
        );
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
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        lutris_home(home.path());
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
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        lutris_home(home.path());
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
        let _guard = APPLY_LOCK.lock().unwrap();
        require_sqlite();
        let (_envelope, root) = fixture_root();
        let home = tempfile::tempdir().unwrap();
        wineboot_home(&home, "wine-ge-9-2");
        lutris_home(home.path());
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
