//! Launcher-proof replacement DLLs in the game root (`wiring.client_dlls`).
//!
//! `apply` backs the original up once into `state_dir`, installs a copy
//! through the journaled transaction, and records the managed set; the
//! launch gate re-asserts the copy by digest; undeclaring restores the
//! backup on the next `apply`.

use super::{
    BTreeMap, Context, Deserialize, Instance, Path, PathBuf, Result, atomic_write_0600, bail,
    files, fs, state_dir, transaction,
};
use std::os::unix::fs::PermissionsExt;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientDll {
    /// Absolute, traversal-free path of the replacement (e.g. a Nix store file).
    pub source: PathBuf,
}

pub type ClientDlls = BTreeMap<String, ClientDll>;

/// Persisted managed set: key -> whether an original was backed up.
type Managed = BTreeMap<String, bool>;

pub fn validate(dlls: &ClientDlls) -> Result<()> {
    for (key, dll) in dlls {
        files::relative(Path::new(key)).with_context(|| format!("client_dlls key '{key}'"))?;
        if !key.to_ascii_lowercase().ends_with(".dll") {
            bail!("client_dlls key must end in .dll: {key}");
        }
        if !dll.source.is_absolute() || super::wiring::has_parent_traversal(&dll.source) {
            bail!(
                "client_dlls source must be absolute and free of '..': {}",
                dll.source.display()
            );
        }
    }
    Ok(())
}

fn manifest_path(instance: &Instance) -> PathBuf {
    state_dir(instance).join("client-dlls.json")
}

fn backup_path(instance: &Instance, key: &str) -> PathBuf {
    state_dir(instance).join("client-dll-backups").join(key)
}

fn read_managed(instance: &Instance) -> Result<Managed> {
    match fs::read(manifest_path(instance)) {
        Ok(bytes) => serde_json::from_slice(&bytes).context("client-dlls.json corrupt"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Managed::new()),
        Err(e) => Err(e).context("read client-dlls.json"),
    }
}

fn write_managed(instance: &Instance, managed: &Managed) -> Result<()> {
    let path = manifest_path(instance);
    fs::create_dir_all(path.parent().context("state dir")?)?;
    atomic_write_0600(&path, &serde_json::to_vec_pretty(managed)?)
}

/// Source bytes; a missing source fails loudly instead of installing nothing.
fn read_source(key: &str, dll: &ClientDll) -> Result<Vec<u8>> {
    match fs::metadata(&dll.source) {
        Ok(m) if m.is_file() => fs::read(&dll.source)
            .with_context(|| format!("read client_dlls source {}", dll.source.display())),
        _ => bail!(
            "client_dlls '{key}': source {} is not a readable file",
            dll.source.display()
        ),
    }
}

fn target_bytes(root: &files::Anchor, key: &str) -> Result<Option<Vec<u8>>> {
    match root.read(Path::new(key), false)? {
        files::Image::Missing => Ok(None),
        files::Image::File(bytes) => Ok(Some(bytes)),
        _ => bail!("client_dlls target {key} is not a regular file"),
    }
}

fn make_readable(root: &Path, key: &str) -> Result<()> {
    fs::set_permissions(root.join(key), fs::Permissions::from_mode(0o644))?;
    Ok(())
}

/// Offline, side-effect-free: sources exist; report installed digest state.
pub fn validate_files(instance: &Instance, dlls: &ClientDlls) -> Result<()> {
    for (key, dll) in dlls {
        let source = read_source(key, dll)?;
        let state = match fs::read(instance.root.join(key)) {
            Ok(bytes) if bytes == source => "installed, hash matches",
            Ok(_) => "installed, hash differs (apply or the launch gate reinstalls)",
            Err(_) => "not installed",
        };
        println!("client_dlls {key}: source ok; {state}");
    }
    Ok(())
}

/// Reconcile the declared set; the caller holds the root lease.
pub fn apply(name: &str, instance: &Instance, dlls: &ClientDlls) -> Result<bool> {
    let managed = read_managed(instance)?;
    if dlls.is_empty() && managed.is_empty() {
        return Ok(false);
    }
    let root = files::Anchor::open(&instance.root)?;
    let mut next = managed.clone();
    let mut installs = Vec::new();
    for (key, dll) in dlls {
        let source = read_source(key, dll)?;
        let current = target_bytes(&root, key)?;
        if !managed.contains_key(key) {
            // First management: back the original up once, before anything
            // can overwrite it. Never overwrite an existing backup.
            let backup = backup_path(instance, key);
            if let Some(original) = &current
                && !backup.exists()
            {
                fs::create_dir_all(backup.parent().context("backup dir")?)?;
                atomic_write_0600(&backup, original)?;
            }
            next.insert(key.clone(), backup.exists());
        }
        if current.as_deref() != Some(source.as_slice()) {
            installs.push((PathBuf::from(key), source));
        }
    }
    // Undeclared: restore the backup (or remove a file we created).
    let mut removals = Vec::new();
    for (key, backed_up) in &managed {
        if dlls.contains_key(key) {
            continue;
        }
        if *backed_up {
            let original = fs::read(backup_path(instance, key))
                .with_context(|| format!("client_dlls backup for {key} is missing"))?;
            if target_bytes(&root, key)?.as_deref() != Some(original.as_slice()) {
                installs.push((PathBuf::from(key), original));
            }
        } else if target_bytes(&root, key)?.is_some() {
            removals.push(key.clone());
        }
    }
    // Record intent first: a crash after the commit must not lose the
    // knowledge that a backup exists.
    if next != managed {
        write_managed(instance, &next)?;
    }
    let changed = !installs.is_empty() || !removals.is_empty();
    let keys: Vec<_> = installs.iter().map(|(p, _)| p.clone()).collect();
    transaction::Prepared::install_files_held(instance, installs)?;
    for key in &keys {
        make_readable(&instance.root, &key.to_string_lossy())?;
        println!("{name}: client dll {} installed", key.display());
    }
    for key in &removals {
        fs::remove_file(instance.root.join(key))?;
        println!("{name}: client dll {key} removed");
    }
    // Undeclared entries are done; drop their records and backups.
    next.retain(|key, _| dlls.contains_key(key));
    for key in managed.keys().filter(|key| !dlls.contains_key(*key)) {
        let _ = fs::remove_file(backup_path(instance, key));
    }
    if next != managed || !keys.is_empty() {
        write_managed(instance, &next)?;
    }
    Ok(changed)
}

/// Launch gate: reinstall any declared DLL the launcher reverted. Cheap
/// (digest compare), no backup, and fails closed on a missing source or an
/// unmanaged key so the game never launches with the wrong DLL.
pub fn reassert(instance: &Instance, dlls: &ClientDlls) -> Result<()> {
    if dlls.is_empty() {
        return Ok(());
    }
    let managed = read_managed(instance)?;
    let root = files::Anchor::open(&instance.root)?;
    for (key, dll) in dlls {
        if !managed.contains_key(key) {
            bail!("client_dlls '{key}' is not applied yet; run `onboard apply` first");
        }
        let source = read_source(key, dll)?;
        if target_bytes(&root, key)?.as_deref() == Some(source.as_slice()) {
            continue;
        }
        let (parent, target) = root.target(Path::new(key), true, &mut Vec::new())?;
        let tmp = files::fd_path(&parent).join(".modde-client-dll-new");
        let _ = fs::remove_file(&tmp);
        fs::write(&tmp, &source)?;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644))?;
        fs::rename(&tmp, &target).with_context(|| format!("reinstall client dll {key}"))?;
        eprintln!("client dll {key} was reverted; reinstalled");
    }
    Ok(())
}
