//! Standalone declarative reconciliation for game-client state.
//!
//! This binary intentionally runs outside Home Manager activation. Home
//! Manager installs modde and exports its runtime settings; this manager is a
//! user-invoked post-setup reconciler for game installations, addon sources,
//! sparse client settings, and character-profile state.

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use nix_manager_core::fs::atomic_write_0600;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

mod files;
mod transaction;
mod wiring;

#[derive(Parser)]
#[command(name = "modde-manager", about = "Declarative post-setup game manager")]
struct Cli {
    #[arg(long, env = "MODDE_MANAGER_CONFIG")]
    config: PathBuf,
    #[command(subcommand)]
    command: CommandKind,
}

#[derive(Subcommand)]
enum CommandKind {
    Check {
        #[arg(long)]
        json: bool,
    },
    Plan {
        #[arg(long)]
        json: bool,
    },
    Apply {
        #[arg(long)]
        prune: bool,
    },
    Update,
    /// Import reviewed exact local commits without fetching or advancing branches.
    Import,
    /// Create a private verified snapshot of the two approved account namespaces.
    Snapshot {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        destination: PathBuf,
    },
    /// Verify a snapshot root against its manifest without changing anything.
    VerifySnapshot {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        manifest: Option<PathBuf>,
        #[arg(long)]
        expected_manifest_sha256: Option<String>,
    },
    /// Declarative runtime onboarding (Wine prefix, Lutris entry, native launch).
    Onboard {
        #[command(subcommand)]
        action: OnboardAction,
    },
    Capture,
}

#[derive(Subcommand)]
enum OnboardAction {
    /// Read-only readiness report; never executes wine, Lutris, or the launcher.
    Status {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        json: bool,
    },
    /// Read-only wiring plan; never writes or executes anything.
    Plan {
        #[arg(long)]
        instance: String,
        #[arg(long)]
        json: bool,
    },
    /// Create the Wine prefix, write the Lutris yml, register the Lutris row.
    Apply {
        #[arg(long)]
        instance: String,
        /// Take over an existing Lutris entry that points elsewhere.
        #[arg(long)]
        adopt: bool,
        /// Re-resolve the runner even when a selection is recorded.
        #[arg(long)]
        reselect: bool,
        /// Fail unless the executed runner version equals this value.
        #[arg(long)]
        expect_runner: Option<String>,
    },
    /// Register the launcher itself (usually the installer first) as its
    /// own Lutris entry with its own prefix. Never launches anything.
    RegisterLauncher {
        #[arg(long)]
        instance: String,
        /// Take over an existing Lutris entry that points elsewhere.
        #[arg(long)]
        adopt: bool,
    },
    /// Resolve/record the runner and ensure the game prefix. No Lutris
    /// writes of any kind — the native path works with Lutris absent.
    Prepare {
        #[arg(long)]
        instance: String,
        /// Re-resolve the runner even when a selection is recorded.
        #[arg(long)]
        reselect: bool,
    },
    /// Launch natively without Lutris and wait for exit. Never installs,
    /// updates, reconciles, records, or falls back — failures surface.
    Launch {
        #[arg(long)]
        instance: String,
        #[arg(long, value_enum)]
        target: NativeTargetArg,
        /// Vanilla runs the declared client as-is; HD requires the full
        /// HD set verified first.
        #[arg(long, value_enum, default_value = "vanilla")]
        mode: LaunchModeArg,
    },
    /// Write the desktop entry invoking the native game launch. The Lutris
    /// entries are untouched.
    DesktopEntry {
        #[arg(long)]
        instance: String,
    },
}

/// Native launch target: the game, the installed maintenance launcher, or
/// the installer as an explicit bootstrap operation.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum NativeTargetArg {
    Game,
    Launcher,
    Installer,
}

/// Launch mode: vanilla never waits for HD; HD requires it verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum LaunchModeArg {
    Vanilla,
    Hd,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    #[serde(default = "default_config_version")]
    version: u32,
    #[serde(default)]
    instances: BTreeMap<String, Instance>,
}

fn default_config_version() -> u32 {
    1
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Instance {
    root: PathBuf,
    #[serde(default = "default_client_kind")]
    client: String,
    #[serde(default)]
    interface: Option<u32>,
    #[serde(default)]
    processes: Vec<String>,
    #[serde(default)]
    addons: Vec<AddonRepo>,
    #[serde(default)]
    config: Vec<ConfigFile>,
    #[serde(default)]
    profiles: Vec<CharacterProfile>,
    #[serde(default)]
    saved_variables: Vec<SavedVariables>,
    #[serde(default)]
    seed_trees: Vec<SeedTree>,
    #[serde(default)]
    lock_file: Option<PathBuf>,
    #[serde(default)]
    state_dir: Option<PathBuf>,
    #[serde(default)]
    wiring: Option<wiring::WiringPatch>,
    /// Named preset expanded once in Rust (`octowow-hd`); user `wiring`
    /// replaces preset lists, merges `tunings.env`, overrides scalars.
    #[serde(default)]
    preset: Option<String>,
}

fn default_client_kind() -> String {
    "wow-wotlk".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddonRepo {
    id: String,
    #[serde(default = "default_branch")]
    branch: String,
    /// Git repository containing the addon. Ascension's repositories default
    /// to the official GitHub organisation, while compatibility addons may be
    /// maintained elsewhere (for example GitLab).
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    directories: Vec<AddonDirectory>,
    #[serde(default)]
    local_source: Option<PathBuf>,
    #[serde(default)]
    revision: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SeedTree {
    source: PathBuf,
    destination: PathBuf,
}

fn default_branch() -> String {
    "main".to_owned()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct AddonDirectory {
    source: String,
    target: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    path: PathBuf,
    #[serde(default)]
    settings: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct CharacterProfile {
    name: String,
    account: String,
    realm: String,
    character: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedVariables {
    path: PathBuf,
    #[serde(default = "default_seed_mode")]
    mode: String,
    #[serde(default)]
    source: Option<PathBuf>,
}

fn default_seed_mode() -> String {
    "seed".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct LockFile {
    #[serde(default)]
    repositories: BTreeMap<String, LockedRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LockedRepository {
    branch: String,
    revision: String,
    #[serde(default)]
    repository: Option<String>,
    #[serde(default)]
    imported_from: Option<PathBuf>,
    #[serde(default)]
    committed_at: Option<u64>,
    #[serde(default)]
    content_sha256: Option<String>,
}

// The pinned nix-manager-core supplies atomic writes, but predates its reconcile API.
// Retain the manager's existing JSON types until that upstream API is published/pinned.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ChangeKind {
    Create,
    Update,
}

#[derive(Debug, Clone, Serialize)]
struct Change {
    resource: String,
    kind: ChangeKind,
    summary: String,
}

#[derive(Debug, Clone, Serialize, Default)]
struct Plan {
    changes: Vec<Change>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = load_config(&cli.config)?;
    if config.version != 1 {
        bail!(
            "unsupported modde-manager config version {}",
            config.version
        );
    }
    match cli.command {
        CommandKind::Check { json } => check_all(&config, json),
        CommandKind::Plan { json } => plan_all(&config, json),
        CommandKind::Apply { prune } => apply_all(&config, prune),
        CommandKind::Update => update_all(&config),
        CommandKind::Import => transaction::import(&config),
        CommandKind::Snapshot {
            source,
            destination,
        } => transaction::snapshot(&config, &source, &destination),
        CommandKind::VerifySnapshot {
            snapshot,
            manifest,
            expected_manifest_sha256,
        } => transaction::verify_snapshot(
            &snapshot,
            &manifest.unwrap_or_else(|| snapshot.join("manifest.json")),
            expected_manifest_sha256.as_deref(),
        ),
        CommandKind::Onboard { action } => {
            // Lutris locations follow XDG_* with $HOME fallback, exactly as
            // the Lutris client resolves them.
            let home = wiring::home_dir()?;
            let dirs = wiring::HomeDirs::from_home(home);
            match action {
                OnboardAction::Status { instance, json } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    let items = wiring::status(&instance, instance_config, &dirs)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&items)?);
                    } else if items
                        .iter()
                        .all(|item| item.state == wiring::ItemState::Verified)
                    {
                        println!("modde-manager: {instance} wiring ready");
                    } else {
                        for item in &items {
                            println!(
                                "{:?} {} — {}{}",
                                item.state,
                                item.name,
                                item.detail,
                                if item.fix.is_empty() {
                                    String::new()
                                } else {
                                    format!(" (fix: {})", item.fix)
                                }
                            );
                        }
                    }
                    if items
                        .iter()
                        .any(|item| item.state != wiring::ItemState::Verified)
                    {
                        bail!("{instance}: wiring not ready");
                    }
                    Ok(())
                }
                OnboardAction::Plan { instance, json } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    let changes = wiring::plan(&instance, instance_config, &dirs)?;
                    if json {
                        println!("{}", serde_json::to_string_pretty(&changes)?);
                    } else if changes.is_empty() {
                        println!("modde-manager: no wiring changes");
                    } else {
                        for change in &changes {
                            let kind = match change.kind {
                                wiring::WiringChangeKind::Change => "change",
                                wiring::WiringChangeKind::Blocker => "BLOCKER",
                            };
                            println!("[{kind}] {} — {}", change.resource, change.summary);
                        }
                    }
                    Ok(())
                }
                OnboardAction::Apply {
                    instance,
                    adopt,
                    reselect,
                    expect_runner,
                } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    wiring::apply(
                        &instance,
                        instance_config,
                        &dirs,
                        adopt,
                        reselect,
                        expect_runner.as_deref(),
                    )
                }
                OnboardAction::RegisterLauncher { instance, adopt } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    wiring::register_launcher(&instance, instance_config, &dirs, adopt)
                }
                OnboardAction::Prepare { instance, reselect } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    wiring::prepare_native(&instance, instance_config, &dirs, reselect)
                }
                OnboardAction::Launch {
                    instance,
                    target,
                    mode,
                } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    wiring::launch(
                        &instance,
                        instance_config,
                        &dirs,
                        match target {
                            NativeTargetArg::Game => wiring::NativeTarget::Game,
                            NativeTargetArg::Launcher => wiring::NativeTarget::Launcher,
                            NativeTargetArg::Installer => wiring::NativeTarget::Installer,
                        },
                        match mode {
                            LaunchModeArg::Vanilla => wiring::LaunchMode::Vanilla,
                            LaunchModeArg::Hd => wiring::LaunchMode::Hd,
                        },
                    )
                }
                OnboardAction::DesktopEntry { instance } => {
                    let instance_config = config
                        .instances
                        .get(&instance)
                        .with_context(|| format!("unknown instance '{instance}'"))?;
                    wiring::desktop_entry(&instance, instance_config, &dirs)
                }
            }
        }
        CommandKind::Capture => capture_all(&config),
    }
}

fn load_config(path: &Path) -> Result<Config> {
    let bytes =
        fs::read(path).with_context(|| format!("read manager config {}", path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("parse manager config {}", path.display()))
}

fn check_all(config: &Config, json: bool) -> Result<()> {
    let mut errors = Vec::new();
    for (name, instance) in &config.instances {
        if let Err(error) = check_instance(name, instance) {
            errors.push(error.to_string());
        }
    }
    if json {
        println!(
            "{}",
            serde_json::json!({"ok": errors.is_empty(), "errors": errors})
        );
    } else if errors.is_empty() {
        println!("modde-manager: all declared instances are healthy");
    } else {
        for error in &errors {
            eprintln!("modde-manager: {error}");
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        bail!("{} instance check(s) failed", errors.len())
    }
}

fn check_instance(name: &str, instance: &Instance) -> Result<()> {
    transaction::prepare(name, instance).map(|_| ())
}

fn validate_instance(name: &str, instance: &Instance) -> Result<()> {
    if !matches!(instance.client.as_str(), "wow-wotlk" | "wow-classic") {
        bail!(
            "{name}: unsupported client '{}'; supported: wow-wotlk, wow-classic",
            instance.client
        );
    }
    if !instance.root.is_dir() {
        bail!(
            "{name}: client root does not exist: {}",
            instance.root.display()
        );
    }
    let addons_root = instance.root.join("Interface/AddOns");
    if !addons_root.is_dir() {
        bail!(
            "{name}: addon root does not exist: {}",
            addons_root.display()
        );
    }
    if !addons_root
        .canonicalize()?
        .starts_with(instance.root.canonicalize()?)
    {
        bail!("addon root escapes client root");
    }
    assert_stopped(instance)?;
    let mut ids = BTreeSet::new();
    let mut targets = BTreeSet::new();
    for addon in &instance.addons {
        if addon.branch.is_empty()
            || addon.branch.starts_with('-')
            || addon.branch.chars().any(char::is_control)
        {
            bail!("unsafe addon branch");
        }
        let reference = format!("refs/heads/{}", addon.branch);
        if !Command::new("git")
            .args(["check-ref-format", &reference])
            .output()?
            .status
            .success()
        {
            bail!("invalid addon branch");
        }
        if addon.id.is_empty()
            || !addon
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
        {
            bail!("unsafe addon id: {}", addon.id);
        }
        if !ids.insert(&addon.id) {
            bail!("duplicate addon id: {}", addon.id);
        }
        if instance.client == "wow-classic"
            && addon
                .repository
                .as_ref()
                .is_none_or(|url| url.trim().is_empty())
        {
            bail!(
                "{name}: Classic addon {} requires an explicit repository",
                addon.id
            );
        }
        for directory in &addon.directories {
            safe_addon_target(&addons_root, &directory.target)?;
            if !targets.insert(&directory.target) {
                bail!("duplicate addon target: {}", directory.target);
            }
            if Path::new(&directory.source).is_absolute()
                || directory.source.contains(['\\', ':'])
                || directory
                    .source
                    .split('/')
                    .any(|part| part == ".." || part.is_empty())
            {
                bail!("unsafe addon source: {}", directory.source);
            }
        }
    }
    for profile in &instance.profiles {
        if profile.name.is_empty()
            || profile.account.is_empty()
            || profile.realm.is_empty()
            || profile.character.is_empty()
        {
            bail!("{name}: profile fields must not be empty");
        }
        for component in [&profile.account, &profile.realm, &profile.character] {
            if component == "."
                || component == ".."
                || component.contains(['/', '\\', ':'])
                || component.chars().any(char::is_control)
            {
                bail!("unsafe character profile component");
            }
        }
        let saved = instance
            .root
            .join("WTF/Account")
            .join(&profile.account)
            .join(&profile.realm)
            .join(&profile.character)
            .join("SavedVariables.lua");
        if !saved.is_file() {
            eprintln!(
                "modde-manager: warning: {name}: profile '{}' has no character SavedVariables yet",
                profile.name
            );
        }
    }
    for saved in &instance.saved_variables {
        if saved.mode != "seed" && saved.mode != "replace" {
            bail!("{name}: unsupported SavedVariables mode '{}'", saved.mode);
        }
        if saved.mode == "replace" && saved.source.is_none() {
            bail!(
                "{name}: replace requires a source for {}",
                saved.path.display()
            );
        }
    }
    Ok(())
}

fn plan_all(config: &Config, json: bool) -> Result<()> {
    let mut plan = Plan::default();
    for (name, instance) in &config.instances {
        validate_instance(name, instance)?;
        plan_instance(name, instance, &mut plan)?;
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&plan)?);
    } else if plan.changes.is_empty() {
        println!("modde-manager: no changes");
    } else {
        for change in &plan.changes {
            println!("{:?} {} — {}", change.kind, change.resource, change.summary);
        }
    }
    Ok(())
}

fn plan_instance(name: &str, instance: &Instance, plan: &mut Plan) -> Result<()> {
    let prepared = transaction::prepare(name, instance)?;
    plan.changes.extend(prepared.plan(name).changes);
    Ok(())
}

fn apply_all(config: &Config, prune: bool) -> Result<()> {
    if prune {
        bail!("pruning is not supported by the safe migration pipeline");
    }
    let prepared = config
        .instances
        .iter()
        .map(|(name, instance)| Ok((instance, transaction::prepare(name, instance)?)))
        .collect::<Result<Vec<_>>>()?;
    for (instance, prepared) in prepared {
        prepared.apply(instance)?;
    }
    println!("modde-manager: apply complete");
    Ok(())
}

#[cfg(test)]
fn apply_instance(name: &str, instance: &Instance, prune: bool) -> Result<()> {
    if prune {
        bail!("pruning is not supported by the safe migration pipeline");
    }
    transaction::prepare(name, instance)?.apply(instance)
}

fn update_all(config: &Config) -> Result<()> {
    for (name, instance) in &config.instances {
        let root = files::Anchor::open(&instance.root)?;
        let _lease = root.lock()?;
        validate_instance(name, instance)?;
        let mut lock = read_lock(instance)?;
        for addon in &instance.addons {
            let checkout = ensure_checkout(
                instance,
                &addon.id,
                &addon.branch,
                addon.repository.as_deref(),
            )?;
            let revision = git_output(&checkout, &["rev-parse", "HEAD"])?;
            lock.repositories.insert(
                addon.id.clone(),
                LockedRepository {
                    branch: addon.branch.clone(),
                    revision,
                    repository: Some(addon.repository.clone().unwrap_or_else(|| {
                        format!("https://github.com/Ascension-Addons/{}.git", addon.id)
                    })),
                    imported_from: None,
                    committed_at: None,
                    content_sha256: Some(transaction::checkout_digest(&checkout)?),
                },
            );
            println!("{name}: {} updated", addon.id);
        }
        write_lock(instance, &lock)?;
    }
    Ok(())
}

fn capture_all(config: &Config) -> Result<()> {
    for (name, instance) in &config.instances {
        for file in &instance.config {
            let path = instance.root.join(&file.path);
            if path.is_file() {
                println!("{name}: {}", path.display());
            }
        }
        for profile in &instance.profiles {
            println!(
                "{name}: profile {} ({}/{}/{})",
                profile.name, profile.account, profile.realm, profile.character
            );
        }
    }
    Ok(())
}

fn assert_stopped(instance: &Instance) -> Result<()> {
    for process in &instance.processes {
        if process.trim().is_empty() {
            bail!("empty process pattern");
        }
        let status = Command::new("pgrep")
            .args(["-f", "--", process])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .context("check game processes")?;
        if status.success() {
            bail!(
                "game process '{}' is running; stop it before reconciling",
                process
            );
        }
        if status.code() != Some(1) {
            bail!("pgrep failed while checking game processes: {status}");
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate_toc_tree(tree: &Path, expected_interface: u32) -> Result<()> {
    let name = tree
        .file_name()
        .context("addon directory has no name")?
        .to_string_lossy();
    let toc = tree.join(format!("{name}.toc"));
    let body = fs::read_to_string(&toc).with_context(|| format!("read {}", toc.display()))?;
    let interfaces: Vec<_> = body
        .lines()
        .filter_map(|line| line.strip_prefix("## Interface:"))
        .map(|value| value.trim().parse::<u32>())
        .collect();
    if interfaces.len() != 1 || interfaces[0].as_ref().ok() != Some(&expected_interface) {
        bail!("unsupported Interface in {}", toc.display());
    }
    Ok(())
}

fn read_lock(instance: &Instance) -> Result<LockFile> {
    let Some(path) = &instance.lock_file else {
        return Ok(LockFile::default());
    };
    let root = files::Anchor::open(Path::new("/"))?;
    match root.read(
        path.strip_prefix("/")
            .context("lock_file must be absolute")?,
        false,
    )? {
        files::Image::Missing => Ok(LockFile::default()),
        files::Image::File(bytes) => Ok(serde_json::from_slice(&bytes)?),
        _ => bail!("lock_file must be a regular file"),
    }
}

fn write_lock(instance: &Instance, lock: &LockFile) -> Result<()> {
    let path = instance
        .lock_file
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("lock_file is required for update"))?;
    atomic_write_0600(path, serde_json::to_string_pretty(lock)?.as_bytes())
}

fn state_dir(instance: &Instance) -> PathBuf {
    instance
        .state_dir
        .clone()
        .unwrap_or_else(|| instance.root.join(".modde-manager"))
}

fn safe_addon_target(root: &Path, target: &str) -> Result<PathBuf> {
    if target.is_empty() || target == "." || target == ".." || target.contains(['/', '\\', ':']) {
        bail!("unsafe managed addon target: {target}");
    }
    let path = root.join(target);
    if path
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        bail!("managed addon target is a symlink: {}", path.display());
    }
    Ok(path)
}

fn checkout_path(instance: &Instance, id: &str) -> PathBuf {
    state_dir(instance).join("repos").join(safe_name(id))
}

fn safe_name(value: &str) -> String {
    let digest = Sha256::digest(value.as_bytes());
    if value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "-_".contains(character))
    {
        value.to_owned()
    } else {
        format!("{}-{}", value.replace('/', "-"), hex(&digest[..6]))
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn ensure_checkout(
    instance: &Instance,
    id: &str,
    branch: &str,
    repository: Option<&str>,
) -> Result<PathBuf> {
    let path = checkout_path(instance, id);
    let url = repository
        .map(str::to_owned)
        .unwrap_or_else(|| format!("https://github.com/Ascension-Addons/{id}.git"));
    fs::create_dir_all(state_dir(instance).join("repos"))?;
    if path.join(".git").is_dir() {
        run_git(&path, &["fetch", "--prune", "origin", branch])?;
        run_git(&path, &["checkout", branch])?;
        run_git(&path, &["reset", "--hard", &format!("origin/{branch}")])?;
    } else {
        let status = Command::new("git")
            .args(["clone", "--single-branch", "--branch", branch])
            .arg(&url)
            .arg(&path)
            .status()?;
        if !status.success() {
            bail!("git clone failed for {id}");
        }
    }
    assert_recent_checkout(&path, id)?;
    Ok(path)
}

fn assert_recent_checkout(path: &Path, id: &str) -> Result<()> {
    let committed = git_output(path, &["show", "-s", "--format=%ct", "HEAD"])?
        .parse::<u64>()
        .with_context(|| format!("parse commit timestamp for {id}"))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let age = now.saturating_sub(committed);
    if age > 365 * 24 * 60 * 60 {
        bail!("{id} has not received a commit in the last 365 days");
    }
    Ok(())
}

fn verify_checkout_revision(path: &Path, expected: &str) -> Result<()> {
    let actual = git_output(path, &["rev-parse", "HEAD"])?;
    if actual != expected {
        bail!(
            "checkout {} is at {}, expected {}",
            path.display(),
            actual,
            expected
        );
    }
    Ok(())
}

fn run_git(path: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new("git").args(args).current_dir(path).status()?;
    if !status.success() {
        bail!("git {:?} failed in {}", args, path.display());
    }
    Ok(())
}

fn git_output(path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git").args(args).current_dir(path).output()?;
    if !output.status.success() {
        bail!("git {:?} failed in {}", args, path.display());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

#[cfg(test)]
fn copy_tree(source: &Path, target: &Path) -> Result<()> {
    if source.symlink_metadata()?.file_type().is_symlink() {
        bail!("addon source contains a symlink: {}", source.display());
    }
    if !source.is_dir() {
        bail!("addon source does not exist: {}", source.display());
    }
    fs::create_dir_all(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let from = entry.path();
        let to = target.join(entry.file_name());
        if entry.file_name() == ".git" {
            continue;
        }
        if entry.file_type()?.is_symlink() {
            bail!("addon source contains a symlink: {}", from.display());
        }
        if from.is_dir() {
            copy_tree(&from, &to)?;
        } else if entry.file_type()?.is_file() {
            fs::copy(&from, &to).with_context(|| format!("copy {}", from.display()))?;
        } else {
            bail!("unsupported addon file type: {}", from.display());
        }
    }
    Ok(())
}

#[cfg(test)]
fn reconcile_config_file(path: &Path, settings: &BTreeMap<String, Value>) -> Result<()> {
    if let Some(rendered) = prepare_config_file(path, settings)? {
        atomic_write_0600(path, rendered.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
fn prepare_config_file(path: &Path, settings: &BTreeMap<String, Value>) -> Result<Option<String>> {
    if settings.is_empty() {
        return Ok(None);
    }
    let original = match fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
    };
    let rendered = render_config(&original, settings)?;
    Ok((rendered != original).then_some(rendered))
}

fn render_config(original: &str, settings: &BTreeMap<String, Value>) -> Result<String> {
    if settings.is_empty() {
        return Ok(original.to_owned());
    }
    let mut desired = BTreeMap::new();
    for (key, value) in settings {
        if !key.bytes().next().is_some_and(|c| c.is_ascii_alphabetic())
            || !key.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            bail!("invalid WoW setting key: {key:?}");
        }
        desired.insert(key.as_str(), wow_value(value)?);
    }
    let mut rendered = String::new();
    let mut seen = BTreeSet::new();
    for line in original.split_inclusive('\n') {
        let mut fields = line.split_whitespace();
        let key = fields
            .next()
            .filter(|command| *command == "SET")
            .and_then(|_| fields.next());
        if let Some(key) = key.filter(|key| desired.contains_key(key)) {
            if seen.insert(key) {
                rendered.push_str(&format!("SET {key} {}", desired[key]));
                if line.ends_with("\r\n") {
                    rendered.push_str("\r\n");
                } else if line.ends_with('\n') {
                    rendered.push('\n');
                }
            }
        } else {
            rendered.push_str(line);
        }
    }
    for (key, value) in desired {
        if !seen.contains(key) {
            if !rendered.is_empty() && !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            rendered.push_str(&format!("SET {key} {value}\n"));
        }
    }
    Ok(rendered)
}

#[cfg(test)]
fn config_needs_reconcile(path: &Path, settings: &BTreeMap<String, Value>) -> Result<bool> {
    Ok(prepare_config_file(path, settings)?.is_some())
}

fn wow_value(value: &Value) -> Result<String> {
    Ok(match value {
        Value::String(value) => {
            if value.chars().any(char::is_control) {
                bail!("WoW setting strings must not contain control characters");
            }
            format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Value::Bool(value) => {
            if *value {
                "\"1\"".into()
            } else {
                "\"0\"".into()
            }
        }
        Value::Number(value) => format!("\"{value}\""),
        _ => bail!("WoW settings must be strings, booleans, or numbers"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_rejects_final_entry_before_any_write() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Interface/AddOns")).unwrap();
        let instance: Instance = serde_json::from_value(serde_json::json!({
            "root": dir.path(), "client": "wow-classic",
            "config": [
                {"path": "WTF/Config.wtf", "settings": {"autoSelfCast": true}},
                {"path": "WTF/other.wtf", "settings": {"invalid key": true}}
            ]
        }))
        .unwrap();
        assert!(apply_instance("test", &instance, false).is_err());
        assert!(!dir.path().join("WTF").exists());
        assert!(!dir.path().join(".modde-manager").exists());
        let mut valid = instance.clone();
        valid.config.pop();
        for client in ["wow-classic", "wow-wotlk"] {
            valid.client = client.into();
            check_instance("test", &valid).unwrap();
            let mut plan = Plan::default();
            plan_instance("test", &valid, &mut plan).unwrap();
            assert_eq!(plan.changes.len(), 1);
            assert!(!dir.path().join("WTF").exists());
        }
        valid.config.push(valid.config[0].clone());
        assert!(transaction::prepare("test", &valid).is_err());
        for path in ["../outside", "/absolute", "WTF/../outside", "WTF\\outside"] {
            assert!(files::relative(Path::new(path)).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("absent"), dir.path().join("WTF")).unwrap();
            assert!(
                files::Anchor::open(dir.path())
                    .unwrap()
                    .read(Path::new("WTF/Config.wtf"), false)
                    .is_err()
            );
        }
    }

    #[test]
    fn sparse_renderer_preserves_unmanaged_bytes_and_relinquishes_keys() {
        let settings = BTreeMap::from([("autoSelfCast".into(), Value::Bool(true))]);
        let original = "# comment\r\nSET autoSelfCast \"0\"\r\nSET volume \"0.3\"\r\nSET autoSelfCast \"1\"\n# no final newline";
        let expected =
            "# comment\r\nSET autoSelfCast \"1\"\r\nSET volume \"0.3\"\r\n# no final newline";
        assert_eq!(render_config(original, &settings).unwrap(), expected);
        assert_eq!(render_config(expected, &settings).unwrap(), expected);
        assert_eq!(render_config(expected, &BTreeMap::new()).unwrap(), expected);
        let settings = BTreeMap::from([
            ("z".into(), Value::Number(42.into())),
            ("a".into(), Value::String("a\\b\"c".into())),
        ]);
        assert_eq!(
            render_config("# keep", &settings).unwrap(),
            "# keep\nSET a \"a\\\\b\\\"c\"\nSET z \"42\"\n"
        );
        for key in ["", "1key", "bad key", "bad\nkey", "bad\"key"] {
            assert!(render_config("", &BTreeMap::from([(key.into(), Value::Bool(true))])).is_err());
        }
        for value in [
            Value::Null,
            serde_json::json!([]),
            serde_json::json!({}),
            Value::String("bad\nline".into()),
            Value::String("bad\0value".into()),
        ] {
            assert!(render_config("", &BTreeMap::from([("valid".into(), value)])).is_err());
        }
    }

    #[test]
    fn sparse_preparation_is_read_only_and_second_apply_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing/Config.wtf");
        let empty = BTreeMap::new();
        assert!(!config_needs_reconcile(&path, &empty).unwrap());
        reconcile_config_file(&path, &empty).unwrap();
        assert!(!path.parent().unwrap().exists());
        let settings = BTreeMap::from([("autoSelfCast".into(), Value::Bool(true))]);
        assert!(config_needs_reconcile(&path, &settings).unwrap());
        assert!(!path.parent().unwrap().exists());
        reconcile_config_file(&path, &settings).unwrap();
        let metadata = fs::metadata(&path).unwrap();
        assert!(!config_needs_reconcile(&path, &settings).unwrap());
        reconcile_config_file(&path, &settings).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            metadata.modified().unwrap()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(fs::metadata(&path).unwrap().ino(), metadata.ino());
        }
        fs::write(&path, [0xff]).unwrap();
        assert!(config_needs_reconcile(&path, &settings).is_err());
        assert!(reconcile_config_file(&path, &settings).is_err());
        assert_eq!(fs::read(&path).unwrap(), [0xff]);
        assert!(config_needs_reconcile(dir.path(), &settings).is_err());
    }

    #[test]
    fn classic_first_install_and_variant_tocs() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("Interface/AddOns")).unwrap();
        let instance: Instance = serde_json::from_value(serde_json::json!({
            "root": dir.path(), "client": "wow-classic",
            "addons": [{"id": "Example", "repository": "https://example.org/addon.git",
                "directories": [{"source": ".", "target": "Example"}]}]
        }))
        .unwrap();
        validate_instance("test", &instance).unwrap();
        let mut plan = Plan::default();
        assert!(plan_instance("test", &instance, &mut plan).is_err());
        assert!(plan.changes.is_empty());
        assert!(check_instance("test", &instance).is_err());
        let addon = dir.path().join("Interface/AddOns/Example");
        fs::create_dir(&addon).unwrap();
        fs::write(addon.join("Example.toc"), "## Interface: 11200\n").unwrap();
        fs::write(addon.join("Example-tbc.toc"), "## Interface: 20400\n").unwrap();
        validate_toc_tree(&addon, 11200).unwrap();
        fs::write(addon.join("Example.toc"), "## Interface: invalid\n").unwrap();
        assert!(validate_toc_tree(&addon, 11200).is_err());
    }

    #[test]
    fn copy_rejects_symlinks_and_skips_git() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        fs::create_dir_all(source.join(".git")).unwrap();
        fs::write(source.join("addon.lua"), "original").unwrap();
        let target = dir.path().join("target");
        copy_tree(&source, &target).unwrap();
        assert!(!target.join(".git").exists());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/", source.join("escape")).unwrap();
            assert!(copy_tree(&source, &dir.path().join("staging")).is_err());
            assert_eq!(
                fs::read_to_string(target.join("addon.lua")).unwrap(),
                "original"
            );
        }
    }

    #[test]
    fn sparse_config_reconciliation_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("Config.wtf");
        fs::write(
            &path,
            "SET gxResolution \"1920x1080\"\nSET unmanaged \"keep\"\n",
        )
        .expect("write config");
        let settings = BTreeMap::from([
            ("gxResolution".to_owned(), Value::String("3440x1440".into())),
            ("gxVSync".to_owned(), Value::Number(0.into())),
        ]);
        assert!(config_needs_reconcile(&path, &settings).expect("compare"));
        reconcile_config_file(&path, &settings).expect("reconcile");
        assert!(!config_needs_reconcile(&path, &settings).expect("compare"));
        let body = fs::read_to_string(path).expect("read config");
        assert!(body.contains("SET unmanaged \"keep\""));
    }

    #[test]
    fn addon_prune_rejects_parent_traversal() {
        let root = Path::new("/tmp/addons");
        assert!(safe_addon_target(root, "../outside").is_err());
        assert!(safe_addon_target(root, "/absolute").is_err());
    }
}
