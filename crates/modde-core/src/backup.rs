use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::PluginEntry;
use crate::error::{CoreError, Result};
use crate::paths;
use crate::resolver::{GameId, ModId};

// ── Backup Manager ──────────────────────────────────────────────

/// Manages zip-based backups for mods and plugin load orders.
pub struct BackupManager {
    backup_dir: PathBuf,
}

/// Metadata for a single backup.
#[derive(Debug, Clone)]
pub struct BackupEntry {
    pub name: String,
    pub path: PathBuf,
    pub created: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum PluginBackupPayload {
    Entries(Vec<PluginEntryBackup>),
    Legacy(Vec<String>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PluginEntryBackup {
    plugin_name: String,
    enabled: bool,
}

impl BackupManager {
    /// Create a new backup manager rooted at the default backup directory.
    pub fn new() -> Result<Self> {
        let backup_dir = paths::modde_data_dir().join("backups");
        fs::create_dir_all(&backup_dir)?;
        Ok(Self { backup_dir })
    }

    /// Create a backup of a mod directory, stored as a zip file.
    pub fn create_mod_backup(&self, mod_id: &ModId, mod_dir: &Path) -> Result<BackupEntry> {
        let dest_dir = self.backup_dir.join("mods").join(mod_id.as_str());
        fs::create_dir_all(&dest_dir)?;

        let timestamp = unix_timestamp_nanos();
        let name = format!("{mod_id}_{timestamp}.zip");
        let zip_path = dest_dir.join(&name);

        info!(%mod_id, path = %zip_path.display(), "creating mod backup");

        create_zip_from_dir(mod_dir, &zip_path)?;

        let created = fs::metadata(&zip_path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::now());

        Ok(BackupEntry {
            name,
            path: zip_path,
            created,
        })
    }

    /// Restore a mod from its latest backup zip into `dest_dir`.
    pub fn restore_mod_backup(&self, mod_id: &ModId, dest_dir: &Path) -> Result<BackupEntry> {
        let entries = self.list_mod_backups(mod_id)?;
        let latest = entries.last().ok_or_else(|| {
            CoreError::Other(format!("no backups found for mod '{mod_id}'").into())
        })?;

        info!(%mod_id, backup = %latest.path.display(), "restoring mod backup");

        extract_zip_to_dir(&latest.path, dest_dir)?;

        Ok(latest.clone())
    }

    /// List available backups for a mod, sorted oldest-first.
    pub fn list_mod_backups(&self, mod_id: &ModId) -> Result<Vec<BackupEntry>> {
        let dir = self.backup_dir.join("mods").join(mod_id.as_str());
        if !dir.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "zip") {
                let created = entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(SystemTime::now());
                entries.push(BackupEntry {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path,
                    created,
                });
            }
        }

        // Order by the nanosecond stamp in the name: mtime ticks coarsely, so
        // two backups made back-to-back can tie or invert.
        entries.sort_by_key(|e| (name_stamp(&e.name), e.created));
        Ok(entries)
    }

    /// Backup the plugin load order for a profile+game as a JSON file.
    pub fn backup_plugin_order(
        &self,
        profile: &str,
        game: &GameId,
        plugins: &[PluginEntry],
    ) -> Result<PathBuf> {
        let dir = self.backup_dir.join("plugins").join(game.as_str());
        fs::create_dir_all(&dir)?;

        let timestamp = unix_timestamp_nanos();
        let name = format!("{profile}_{timestamp}.json");
        let path = dir.join(&name);

        let payload = PluginBackupPayload::Entries(
            plugins
                .iter()
                .map(|plugin| PluginEntryBackup {
                    plugin_name: plugin.plugin_name.clone(),
                    enabled: plugin.enabled,
                })
                .collect(),
        );
        let json = serde_json::to_string_pretty(&payload)?;
        fs::write(&path, json)?;

        info!(%profile, %game, path = %path.display(), "plugin order backed up");
        Ok(path)
    }

    /// Restore the most recent plugin load order backup for a profile+game.
    pub fn restore_plugin_order(&self, profile: &str, game: &GameId) -> Result<Vec<PluginEntry>> {
        let dir = self.backup_dir.join("plugins").join(game.as_str());
        if !dir.exists() {
            return Err(CoreError::Other(
                format!("no plugin backups for game '{game}'").into(),
            ));
        }

        // Match `<profile>_<timestamp>.json` exactly, so profile `main` never
        // picks up `main_extra_<timestamp>.json`.
        let prefix = format!("{profile}_");
        let mut candidates: Vec<_> = fs::read_dir(&dir)?
            .filter_map(std::result::Result::ok)
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .and_then(|name| name.strip_prefix(&prefix)?.strip_suffix(".json"))
                    .is_some_and(|ts| !ts.is_empty() && ts.bytes().all(|b| b.is_ascii_digit()))
            })
            .collect();

        candidates.sort_by_key(|e| {
            let mtime = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(UNIX_EPOCH);
            (name_stamp(&e.file_name().to_string_lossy()), mtime)
        });

        let latest = candidates.last().ok_or_else(|| {
            CoreError::Other(
                format!("no plugin backups for profile '{profile}' / game '{game}'").into(),
            )
        })?;

        let data = fs::read_to_string(latest.path())?;
        let payload: PluginBackupPayload = serde_json::from_str(&data)?;

        Ok(match payload {
            PluginBackupPayload::Entries(entries) => entries
                .into_iter()
                .enumerate()
                .map(|(sort_index, entry)| PluginEntry {
                    plugin_name: entry.plugin_name,
                    sort_index: sort_index as i64,
                    enabled: entry.enabled,
                })
                .collect(),
            PluginBackupPayload::Legacy(entries) => entries
                .into_iter()
                .enumerate()
                .map(|(sort_index, plugin_name)| PluginEntry {
                    plugin_name,
                    sort_index: sort_index as i64,
                    enabled: true,
                })
                .collect(),
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────

/// Nanoseconds since the epoch, so backups made within the same second get
/// distinct file names instead of overwriting each other.
fn unix_timestamp_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}

/// Create a zip archive from a directory.
/// The nanosecond stamp in `<id>_<stamp>.<ext>`; 0 when absent.
fn name_stamp(name: &str) -> u128 {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    stem.rsplit_once('_')
        .and_then(|(_, ts)| ts.parse().ok())
        .unwrap_or(0)
}

fn create_zip_from_dir(src: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let entries = walkdir(src)?;
    for entry in &entries {
        let rel = entry
            .strip_prefix(src)
            .map_err(|e| CoreError::Other(e.to_string().into()))?;
        let rel_str = rel.to_string_lossy();

        if entry.is_dir() {
            zip.add_directory(rel_str.as_ref(), options)
                .map_err(|e| CoreError::Other(e.to_string().into()))?;
        } else {
            zip.start_file(rel_str.as_ref(), options)
                .map_err(|e| CoreError::Other(e.to_string().into()))?;
            let mut f = fs::File::open(entry)?;
            let mut buf = Vec::new();
            f.read_to_end(&mut buf)?;
            std::io::Write::write_all(&mut zip, &buf)?;
        }
    }

    zip.finish()
        .map_err(|e| CoreError::Other(e.to_string().into()))?;
    Ok(())
}

/// Extract a zip archive into `dest`.
fn extract_zip_to_dir(zip_path: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| CoreError::Other(e.to_string().into()))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| CoreError::Other(e.to_string().into()))?;
        let Some(name) = entry.enclosed_name() else {
            warn!(
                entry = entry.name(),
                "skipping backup entry that escapes the destination"
            );
            continue;
        };
        let out_path = dest.join(name);

        if entry.is_dir() {
            fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut outfile)?;
        }
    }

    Ok(())
}

/// Recursively list all files and directories under `root`.
fn walkdir(root: &Path) -> Result<Vec<PathBuf>> {
    let mut result = Vec::new();
    walk_recursive(root, &mut result)?;
    result.sort();
    Ok(result)
}

fn walk_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        out.push(path.clone());
        if path.is_dir() {
            walk_recursive(&path, out)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager_in(dir: &Path) -> BackupManager {
        BackupManager {
            backup_dir: dir.to_path_buf(),
        }
    }

    #[test]
    fn plugin_backup_round_trips_enabled_state() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = manager_in(tmp.path());

        let plugins = vec![
            PluginEntry {
                plugin_name: "Skyrim.esm".to_string(),
                sort_index: 0,
                enabled: true,
            },
            PluginEntry {
                plugin_name: "Optional.esp".to_string(),
                sort_index: 1,
                enabled: false,
            },
        ];

        mgr.backup_plugin_order("default", &GameId::from("skyrim-se"), &plugins)
            .unwrap();

        let restored = mgr
            .restore_plugin_order("default", &GameId::from("skyrim-se"))
            .unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].plugin_name, "Skyrim.esm");
        assert!(restored[0].enabled);
        assert_eq!(restored[1].plugin_name, "Optional.esp");
        assert!(!restored[1].enabled);
    }

    #[test]
    fn restore_plugin_order_accepts_legacy_backups() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = manager_in(tmp.path());
        let dir = tmp.path().join("plugins").join("skyrim-se");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("default_1.json"),
            serde_json::to_string(&vec!["One.esm", "Two.esp"]).unwrap(),
        )
        .unwrap();

        let restored = mgr
            .restore_plugin_order("default", &GameId::from("skyrim-se"))
            .unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[0].plugin_name, "One.esm");
        assert!(restored[0].enabled);
        assert_eq!(restored[1].plugin_name, "Two.esp");
        assert!(restored[1].enabled);
    }

    fn plugin(name: &str) -> PluginEntry {
        PluginEntry {
            plugin_name: name.to_string(),
            sort_index: 0,
            enabled: true,
        }
    }

    #[test]
    fn mod_backups_round_trip_and_are_never_overwritten() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = manager_in(&tmp.path().join("backups"));
        let mod_id = ModId::from("mod_a");
        let mod_dir = tmp.path().join("store/mod_a");
        fs::create_dir_all(mod_dir.join("nested")).unwrap();
        fs::write(mod_dir.join("nested/file.txt"), "v1").unwrap();

        assert!(matches!(
            mgr.list_mod_backups(&mod_id).unwrap().as_slice(),
            []
        ));
        assert!(mgr.restore_mod_backup(&mod_id, &mod_dir).is_err());

        mgr.create_mod_backup(&mod_id, &mod_dir).unwrap();
        fs::write(mod_dir.join("nested/file.txt"), "v2").unwrap();
        let second = mgr.create_mod_backup(&mod_id, &mod_dir).unwrap();
        // Two backups in the same second must not share a file name.
        assert_eq!(mgr.list_mod_backups(&mod_id).unwrap().len(), 2);

        fs::write(mod_dir.join("nested/file.txt"), "broken").unwrap();
        let restored = mgr.restore_mod_backup(&mod_id, &mod_dir).unwrap();
        assert_eq!(restored.name, second.name);
        assert_eq!(
            fs::read_to_string(mod_dir.join("nested/file.txt")).unwrap(),
            "v2"
        );
    }

    #[test]
    fn restore_skips_entries_that_escape_the_destination() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = manager_in(&tmp.path().join("backups"));
        let mod_id = ModId::from("mod_a");
        let dir = tmp.path().join("backups/mods/mod_a");
        fs::create_dir_all(&dir).unwrap();
        let mut zip = zip::ZipWriter::new(fs::File::create(dir.join("mod_a_1.zip")).unwrap());
        for name in ["../escape.txt", "kept.txt"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"x").unwrap();
        }
        zip.finish().unwrap();

        let dest = tmp.path().join("restore/mod_a");
        mgr.restore_mod_backup(&mod_id, &dest).unwrap();
        assert!(dest.join("kept.txt").is_file());
        assert!(!tmp.path().join("restore/escape.txt").exists());
    }

    #[test]
    fn plugin_backups_of_prefix_sharing_profiles_stay_separate() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = manager_in(tmp.path());
        let game = GameId::from("skyrim-se");
        mgr.backup_plugin_order("main", &game, &[plugin("Main.esp")])
            .unwrap();
        mgr.backup_plugin_order("main_extra", &game, &[plugin("Extra.esp")])
            .unwrap();

        let restored = mgr.restore_plugin_order("main", &game).unwrap();
        assert_eq!(restored[0].plugin_name, "Main.esp");
        assert!(mgr.restore_plugin_order("other", &game).is_err());
        assert!(
            mgr.restore_plugin_order("main", &GameId::from("fallout4"))
                .is_err()
        );
    }
}
