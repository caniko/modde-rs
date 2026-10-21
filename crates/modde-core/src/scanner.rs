use std::collections::{HashMap, HashSet};

use crate::manifest::wabbajack::{
    ArchiveEntry, ArchiveState, InstallDirective, WabbajackManifest, compute_manifest_hash,
};
use crate::nexus_id::{NexusFileId, NexusModId};
use crate::profile::{EnabledMod, LoadOrderLock, LockReason, Profile};
mod duplicates;
pub use duplicates::{DuplicateReport, ModFootprint, detect_stale_duplicates};

/// Canonical `mod_id` derivation for a Wabbajack archive entry.
///
/// Used by **both** the scanner and the Wabbajack installer so that a
/// profile installed via `modde install wabbajack` and the same modlist
/// re-scanned via `modde scan --manifest` produce identical `mod_id`
/// strings — otherwise retroactive-lock flows would create duplicates
/// rather than matching existing mods.
///
/// - Nexus-sourced archives: `nexus_{game_domain}_{mod_id}_{file_id}`
/// - Everything else:        `wj_{archive_hash}`
#[must_use]
pub fn archive_mod_id(archive: &ArchiveEntry) -> String {
    if let Some(ArchiveState::NexusDownloader {
        game_name,
        mod_id,
        file_id,
    }) = archive.state.as_ref()
    {
        format!("nexus_{game_name}_{mod_id}_{file_id}")
    } else {
        format!("wj_{}", archive.hash)
    }
}

/// A mod discovered by matching a Wabbajack manifest against files on disk.
pub struct ManifestMatch {
    /// Stable unique ID based on Nexus identity or archive hash.
    pub mod_id: String,
    /// Human-readable name (from archive filename, cleaned).
    pub display_name: String,
    /// Original archive filename.
    pub archive_name: String,
    pub archive_hash: u64,
    pub total_files: usize,
    pub present_files: usize,
    pub confidence: f32,
    pub nexus_mod_id: Option<NexusModId>,
    pub nexus_file_id: Option<NexusFileId>,
    pub nexus_game_domain: Option<String>,
    /// Game-relative file paths that this archive covers on disk (lowercased).
    /// Used for correlation with filesystem-discovered mods.
    pub covered_paths: Vec<String>,
}

/// Match files on disk against a Wabbajack manifest.
///
/// Groups directives by their source `archive_hash`, then checks what
/// fraction of each archive's `to` paths exist in `on_disk_files`.
/// Archives where the fraction meets or exceeds `threshold` are returned.
///
/// `on_disk_files` should contain lowercased, forward-slash relative paths
/// from the game install root.
#[must_use]
pub fn match_wabbajack_manifest(
    manifest: &WabbajackManifest,
    on_disk_files: &HashSet<String>,
    threshold: f32,
) -> Vec<ManifestMatch> {
    let directives = manifest.install_directives();

    // Group directives by archive_hash → list of game-relative paths.
    let mut archive_files: HashMap<u64, Vec<String>> = HashMap::new();

    for d in &directives {
        match d {
            InstallDirective::FromArchive {
                archive_hash, to, ..
            }
            | InstallDirective::PatchedFromArchive {
                archive_hash, to, ..
            } => {
                // Strip prefix and lowercase for matching.
                let game_relative = strip_mo2_prefix(&to.replace('\\', "/").to_lowercase());
                archive_files
                    .entry(*archive_hash)
                    .or_default()
                    .push(game_relative);
            }
            _ => {}
        }
    }

    // Build archive hash → ArchiveEntry lookup for metadata.
    let archive_map: HashMap<u64, &crate::manifest::wabbajack::ArchiveEntry> =
        manifest.archives.iter().map(|a| (a.hash, a)).collect();

    let mut results = Vec::new();

    for (hash, files) in &archive_files {
        let total = files.len();
        if total == 0 {
            continue;
        }

        let present_paths: Vec<String> = files
            .iter()
            .filter(|path| on_disk_files.contains(path.as_str()))
            .cloned()
            .collect();
        let present = present_paths.len();

        let fraction = present as f32 / total as f32;
        if fraction < threshold {
            continue;
        }

        let archive = archive_map.get(hash);
        let archive_name = archive.map_or_else(|| format!("unknown_{hash}"), |a| a.name.clone());

        // Display name: prefer cleaned archive filename (unique per archive).
        let display_name = clean_archive_name(&archive_name);

        let (nexus_mod_id, nexus_file_id, nexus_game_domain) = archive
            .and_then(|a| a.state.as_ref())
            .map_or((None, None, None), |state| match state {
                ArchiveState::NexusDownloader {
                    game_name,
                    mod_id,
                    file_id,
                } => (Some(*mod_id), Some(*file_id), Some(game_name.clone())),
                _ => (None, None, None),
            });

        // Canonical mod_id — must match `archive_mod_id` exactly so Wabbajack
        // installs + retroactive scans dedup correctly.
        let mod_id = match archive {
            Some(a) => archive_mod_id(a),
            None => format!("wj_{hash}"),
        };

        results.push(ManifestMatch {
            mod_id,
            display_name,
            archive_name,
            archive_hash: *hash,
            total_files: total,
            present_files: present,
            confidence: fraction,
            nexus_mod_id,
            nexus_file_id,
            nexus_game_domain,
            covered_paths: present_paths,
        });
    }

    // Sort by display_name for readability.
    results.sort_by(|a, b| {
        a.display_name
            .to_lowercase()
            .cmp(&b.display_name.to_lowercase())
    });
    results
}

/// Convert a `ManifestMatch` into an `EnabledMod` for database storage.
#[must_use]
pub fn manifest_match_to_enabled(m: &ManifestMatch) -> EnabledMod {
    EnabledMod {
        mod_id: m.mod_id.clone(),
        display_name: Some(m.display_name.clone()),
        enabled: true,
        version: None,
        fomod_config: None,
        nexus_mod_id: m.nexus_mod_id,
        nexus_file_id: m.nexus_file_id,
        nexus_game_domain: m.nexus_game_domain.clone(),
        installed_timestamp: Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64,
        ),
        ..Default::default()
    }
}

/// Strip MO2 staging prefix from a path.
///
/// `mods/<mod_name>/<game_relative_path>` → `<game_relative_path>`.
/// Non-mod paths (e.g., MO2 executables) are returned as-is.
fn strip_mo2_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix("mods/")
        && let Some(idx) = rest.find('/')
    {
        return rest[idx + 1..].to_string();
    }
    path.to_string()
}

/// Clean an archive filename into a display name.
///
/// `ImmersiveHealing-26281-3-1-3-1772288704.zip` → `ImmersiveHealing`.
/// Strips the Nexus suffix pattern (mod_id-version-timestamp.ext).
fn clean_archive_name(name: &str) -> String {
    // Strip extension.
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    // Nexus filenames: "ModName-modid-version-timestamp". Strip from first `-{digits}`.
    if let Some((idx, _)) = stem
        .match_indices('-')
        .find(|&(i, _)| stem[i + 1..].starts_with(|c: char| c.is_ascii_digit()))
    {
        stem[..idx].replace('_', " ")
    } else {
        stem.replace('_', " ")
    }
}

/// Compute the canonical mod order from a Wabbajack manifest's install
/// directives.
///
/// `WabbajackManifest.archives` is an unordered JSON array — not a load
/// order. The *directive* list, however, is the sequence Wabbajack applies
/// on install, so the first-appearance order of each archive in the
/// directives is the closest reproducible approximation of "load order".
///
/// Returns a `Vec<String>` of canonical `mod_id`s (as produced by
/// [`archive_mod_id`]) in the order the corresponding archives first
/// appear in the install directives. Archives that never appear in a
/// [`InstallDirective::FromArchive`] / [`InstallDirective::PatchedFromArchive`]
/// are omitted.
#[must_use]
pub fn manifest_directive_order(manifest: &WabbajackManifest) -> Vec<String> {
    let archive_by_hash: HashMap<u64, &ArchiveEntry> =
        manifest.archives.iter().map(|a| (a.hash, a)).collect();

    let mut seen: HashSet<u64> = HashSet::new();
    let mut order: Vec<String> = Vec::new();
    for d in manifest.install_directives() {
        let hash = match d {
            InstallDirective::FromArchive { archive_hash, .. }
            | InstallDirective::PatchedFromArchive { archive_hash, .. } => archive_hash,
            _ => continue,
        };
        if !seen.insert(hash) {
            continue;
        }
        if let Some(archive) = archive_by_hash.get(&hash) {
            order.push(archive_mod_id(archive));
        }
    }
    order
}

/// Report from [`apply_wabbajack_lock`] — what the in-place reorder did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WabbajackLockApplied {
    /// `manifest_hash` recorded on the new lock. Matches
    /// `ProfileSource::Wabbajack { manifest_hash }` on installs.
    pub manifest_hash: String,
    /// Number of mods whose `mod_id` is present in the manifest order
    /// (these end up at the front of the mod list).
    pub matched: usize,
    /// Number of pre-existing profile mods not mentioned by the
    /// manifest (these are appended after, preserving relative order).
    pub unmatched: usize,
    /// Whether the profile already carried a lock that was overwritten.
    pub replaced_existing_lock: bool,
}

/// Reorder `profile.mods` to follow the manifest's install-directive
/// order and stamp a `LockReason::Wabbajack` lock onto the profile.
///
/// This is the pure helper that powers `modde scan --manifest` and is
/// the recommended way to retroactively lock an existing profile to a
/// Wabbajack modlist. Extracted from `scan.rs` so it can be unit-tested
/// without touching the filesystem scanner.
///
/// Invariants:
///
/// 1. **Mod count is preserved** — no mod is ever dropped. Matched mods
///    move to the front in manifest order; unmatched mods retain their
///    original relative order and are appended after.
/// 2. **Matched mods are sorted by first-appearance in install
///    directives** — see [`manifest_directive_order`] for the semantic.
/// 3. **`profile.load_order_lock` is overwritten** — any prior lock
///    (including a stale Wabbajack or Manual lock) is replaced. The
///    return value's `replaced_existing_lock` field lets callers surface
///    this to the user.
pub fn apply_wabbajack_lock(
    profile: &mut Profile,
    manifest: &WabbajackManifest,
) -> WabbajackLockApplied {
    let manifest_order = manifest_directive_order(manifest);
    let manifest_rank: HashMap<String, usize> = manifest_order
        .iter()
        .enumerate()
        .map(|(i, mid)| (mid.clone(), i))
        .collect();

    // Stable partition: matched first (in manifest order), unmatched
    // after (original relative order preserved).
    let (mut matched, unmatched): (Vec<EnabledMod>, Vec<EnabledMod>) =
        std::mem::take(&mut profile.mods)
            .into_iter()
            .partition(|m| manifest_rank.contains_key(&m.mod_id));

    matched.sort_by_key(|m| manifest_rank.get(&m.mod_id).copied().unwrap_or(usize::MAX));

    let matched_count = matched.len();
    let unmatched_count = unmatched.len();
    profile.mods = matched;
    profile.mods.extend(unmatched);

    let manifest_hash = compute_manifest_hash(manifest);
    let replaced_existing_lock = profile.load_order_lock.is_some();
    profile.load_order_lock = Some(LoadOrderLock::now(LockReason::Wabbajack {
        manifest_hash: manifest_hash.clone(),
    }));

    WabbajackLockApplied {
        manifest_hash,
        matched: matched_count,
        unmatched: unmatched_count,
        replaced_existing_lock,
    }
}

/// Convert a filesystem-discovered mod into an `EnabledMod`.
pub fn discovered_to_enabled(
    mod_id: &str,
    display_name: &str,
    version: Option<&str>,
    _confidence: f32,
) -> EnabledMod {
    EnabledMod {
        mod_id: mod_id.to_string(),
        display_name: Some(display_name.to_string()),
        enabled: true,
        version: version.map(String::from),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> WabbajackManifest {
        serde_json::from_value(serde_json::json!({
            "Name": "t", "Author": "t", "Description": "t", "Game": "SkyrimSpecialEdition", "Version": "1",
            "Archives": [
                { "Hash": 1, "Name": "SkyUI-Patch-12604-5-2-1700000000.7z", "Size": 1,
                  "State": { "$type": "NexusDownloader, Wabbajack.Lib",
                             "GameName": "skyrimspecialedition", "ModID": 12604, "FileID": 5 } },
                { "Hash": 2, "Name": "Half_Present.zip", "Size": 1 }
            ],
            "Directives": [
                { "$type": "FromArchive, Wabbajack.Lib", "ArchiveHashPath": [1, "a"],
                  "To": "mods\\SkyUI\\Interface\\A.swf" },
                { "$type": "FromArchive, Wabbajack.Lib", "ArchiveHashPath": [2, "b"], "To": "Data/b1.esp" },
                { "$type": "FromArchive, Wabbajack.Lib", "ArchiveHashPath": [2, "c"], "To": "Data/b2.esp" },
                { "$type": "FromArchive, Wabbajack.Lib", "ArchiveHashPath": [3, "d"], "To": "Data/orphan.esp" }
            ]
        }))
        .unwrap()
    }

    fn on_disk(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|p| (*p).to_string()).collect()
    }

    #[test]
    fn match_strips_mo2_prefix_and_reports_nexus_identity() {
        let matches = match_wabbajack_manifest(&manifest(), &on_disk(&["interface/a.swf"]), 1.0);
        assert_eq!(matches.len(), 1);
        let m = &matches[0];
        assert_eq!(m.mod_id, "nexus_skyrimspecialedition_12604_5");
        assert_eq!(m.display_name, "SkyUI-Patch");
        assert_eq!(m.covered_paths, vec!["interface/a.swf".to_string()]);
        assert_eq!(m.nexus_game_domain.as_deref(), Some("skyrimspecialedition"));
    }

    #[test]
    fn match_threshold_is_inclusive_and_unknown_hashes_fall_back() {
        let disk = on_disk(&["data/b1.esp", "data/orphan.esp"]);
        let names = |threshold| -> Vec<String> {
            match_wabbajack_manifest(&manifest(), &disk, threshold)
                .into_iter()
                .map(|m| format!("{}={}", m.mod_id, m.display_name))
                .collect()
        };
        assert_eq!(names(0.5), ["wj_2=Half Present", "wj_3=unknown 3"]);
        assert_eq!(names(0.51), ["wj_3=unknown 3"]);
    }

    #[test]
    fn clean_archive_name_strips_nexus_suffix() {
        assert_eq!(
            clean_archive_name("ImmersiveHealing-26281-3-1-3-1772288704.zip"),
            "ImmersiveHealing"
        );
        assert_eq!(
            clean_archive_name("SkyUI-Patch-12604-5-2.7z"),
            "SkyUI-Patch"
        );
        assert_eq!(clean_archive_name("No_Suffix-Here.7z"), "No Suffix-Here");
        assert_eq!(clean_archive_name("noext"), "noext");
    }
}
