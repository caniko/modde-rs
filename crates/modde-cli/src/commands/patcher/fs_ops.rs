//! Patcher filesystem, manifest, and environment helpers.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

use modde_core::fs::{is_cross_device_error, walk_files_relative};
use modde_core::hash::sha256_hex;
use modde_core::profile::{Profile, ProfileManager};
use modde_core::{ModdeDb, PatcherStageRow, paths};

use crate::commands::load_plugin_order;

use super::FileFingerprint;

pub(super) async fn write_load_order(profile: &Profile, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let pm = ProfileManager::open()
        .await
        .context("failed to open profile database for patcher load order")?;
    let plugins = load_plugin_order(&pm, profile).await?;
    let mut content = String::new();
    for plugin in plugins.iter().filter(|plugin| plugin.enabled) {
        content.push('*');
        content.push_str(&plugin.plugin_name);
        content.push('\n');
    }
    fs::write(path, content)?;
    Ok(())
}

pub(super) async fn snapshot_dir(root: &Path) -> Result<HashMap<String, FileFingerprint>> {
    let mut snapshot = HashMap::new();
    for (rel_path, abs_path) in walk_files_relative(root)? {
        let sha256 = modde_core::hash::hash_file_sha256(&abs_path)
            .await
            .with_context(|| format!("failed to hash {}", abs_path.display()))?;
        snapshot.insert(rel_path, FileFingerprint { sha256 });
    }
    Ok(snapshot)
}

pub(super) fn compute_next_manifest(
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
    own_before: &HashSet<String>,
    other_owned: &HashSet<String>,
    stage: &PatcherStageRow,
) -> Result<Vec<String>> {
    let mut next_manifest = own_before
        .iter()
        .filter(|rel_path| after.contains_key(*rel_path))
        .cloned()
        .collect::<Vec<_>>();

    for rel_path in changed_non_owned_paths(before, after, own_before) {
        if before.contains_key(&rel_path) {
            let owner = if other_owned.contains(&rel_path) {
                "another managed stage"
            } else {
                "the base deployment"
            };
            anyhow::bail!(
                "patcher stage '{}' modified '{}' owned by {}; patcher stages may only create new files or rewrite their own prior outputs",
                stage.name,
                rel_path,
                owner
            );
        }
        if after.contains_key(&rel_path) {
            next_manifest.push(rel_path);
        }
    }

    next_manifest.sort();
    next_manifest.dedup();
    Ok(next_manifest)
}

pub(super) fn changed_non_owned_paths(
    before: &HashMap<String, FileFingerprint>,
    after: &HashMap<String, FileFingerprint>,
    own_before: &HashSet<String>,
) -> Vec<String> {
    let mut changed = HashSet::new();
    for rel_path in before.keys().chain(after.keys()) {
        if own_before.contains(rel_path) {
            continue;
        }
        if before.get(rel_path) != after.get(rel_path) {
            changed.insert(rel_path.clone());
        }
    }
    let mut changed = changed.into_iter().collect::<Vec<_>>();
    changed.sort();
    changed
}

pub(super) async fn install_managed_output(
    db: &ModdeDb,
    profile: &Profile,
    game_plugin: &dyn modde_games::GamePlugin,
    install_dir: &Path,
    prefix: Option<&Path>,
    game_mod_dir: &Path,
    stage: &PatcherStageRow,
    own_before: &HashSet<String>,
    next_manifest: &[String],
    cache_key: &str,
) -> Result<()> {
    let generated = stage_generated_dir(profile, &stage.name);
    let mut old_paths = own_before.iter().cloned().collect::<Vec<_>>();
    old_paths.sort();
    remove_rel_paths(game_mod_dir, &old_paths)?;

    if generated.exists() {
        fs::remove_dir_all(&generated)?;
    }
    fs::create_dir_all(&generated)?;

    for rel_path in next_manifest {
        let src = game_mod_dir.join(rel_path);
        let dst = generated.join(rel_path);
        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }
        move_file(&src, &dst)?;
    }

    db.replace_patcher_stage_outputs(require_profile_id(profile)?, &stage.name, next_manifest)
        .await?;

    if !next_manifest.is_empty() {
        game_plugin
            .deploy_to_install_at(&generated, install_dir, prefix)
            .with_context(|| {
                format!(
                    "failed to project generated output for patcher stage '{}'",
                    stage.name
                )
            })?;
    }
    db.mark_patcher_stage_cache_success(require_profile_id(profile)?, &stage.name, cache_key)
        .await?;
    Ok(())
}

pub(super) fn remove_rel_paths(root: &Path, rel_paths: &[String]) -> Result<()> {
    for rel_path in rel_paths {
        let path = root.join(rel_path);
        if path.symlink_metadata().is_ok() {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove managed output {}", path.display()))?;
            remove_empty_parents(root, path.parent());
        }
    }
    Ok(())
}

pub(super) fn patcher_cache_key(stage: &PatcherStageRow, load_order: &str) -> Result<String> {
    let settings_json = serde_json::to_string(&stage.settings)
        .context("failed to serialize patcher settings for cache key")?;
    let payload = serde_json::json!({
        "stage_name": &stage.name,
        "stage_kind": stage.stage_kind.as_str(),
        "settings": settings_json,
        "output_mod": &stage.output_mod,
        "load_order": load_order,
    });
    Ok(sha256_hex(payload.to_string().as_bytes()))
}

pub(super) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&src_path, &dst_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    src_path.display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

pub(super) fn remove_dir_if_exists(path: &Path) -> Result<()> {
    if path.exists() {
        fs::remove_dir_all(path).with_context(|| format!("failed to remove {}", path.display()))?;
    }
    Ok(())
}

pub(super) fn patcher_time_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}{:09}", now.as_secs(), now.subsec_nanos())
}

pub(super) fn remove_empty_parents(root: &Path, mut current: Option<&Path>) {
    while let Some(dir) = current {
        if dir == root {
            break;
        }
        match fs::remove_dir(dir) {
            Ok(()) => current = dir.parent(),
            Err(_) => break,
        }
    }
}

pub(super) fn move_file(src: &Path, dst: &Path) -> Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(err) if is_cross_device_error(&err) => {
            fs::copy(src, dst).with_context(|| {
                format!("failed to copy {} to {}", src.display(), dst.display())
            })?;
            fs::remove_file(src).with_context(|| format!("failed to remove {}", src.display()))?;
            Ok(())
        }
        Err(err) => Err(err)
            .with_context(|| format!("failed to move {} to {}", src.display(), dst.display())),
    }
}

pub(super) fn stage_generated_dir(profile: &Profile, stage_name: &str) -> PathBuf {
    paths::generated_dir()
        .join(profile.game_id.as_str())
        .join(&profile.name)
        .join(stage_name)
}

pub(super) fn parse_env(entries: Vec<String>) -> Result<HashMap<String, String>> {
    let mut env = HashMap::new();
    for entry in entries {
        let (key, value) = entry
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("environment entries must be KEY=VALUE"))?;
        if key.is_empty() {
            anyhow::bail!("environment variable key cannot be empty");
        }
        env.insert(key.to_string(), value.to_string());
    }
    Ok(env)
}

pub(super) fn require_profile_id(profile: &Profile) -> Result<i64> {
    profile
        .id
        .ok_or_else(|| anyhow::anyhow!("profile '{}' is not persisted", profile.name))
}
