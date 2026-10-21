use std::collections::HashSet;
use std::path::PathBuf;

use anyhow::{Context, Result};

use modde_core::GameId;
use modde_core::manifest::wabbajack::WabbajackManifest;
use modde_core::profile::{EnabledMod, Profile, ProfileManager, ProfileSource};
use modde_core::scanner::{
    apply_wabbajack_lock, manifest_match_to_enabled, match_wabbajack_manifest,
};
use modde_games::ScanContext;

pub async fn handle(
    game: String,
    game_dir: Option<PathBuf>,
    manifest: Option<PathBuf>,
    import_to: Option<String>,
    threshold: f32,
    dry_run: bool,
    prune_duplicates: bool,
) -> Result<()> {
    if prune_duplicates && manifest.is_none() {
        anyhow::bail!("--prune-duplicates requires --manifest");
    }
    if prune_duplicates && import_to.is_none() {
        anyhow::bail!("--prune-duplicates requires --import-to");
    }
    // Resolve game plugin and scanner.
    let game_plugin = modde_games::resolve_game_plugin(&game).ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported game '{game}'. Supported: {}",
            modde_games::supported_game_ids().join(", ")
        )
    })?;

    let scanner = modde_games::resolve_mod_scanner(&game)
        .ok_or_else(|| anyhow::anyhow!("no mod scanner available for game '{game}'"))?;

    // Resolve install directory.
    let install_dir = match game_dir {
        Some(d) => {
            anyhow::ensure!(d.is_dir(), "game directory does not exist: {}", d.display());
            d
        }
        None => game_plugin.detect_install().ok_or_else(|| {
            anyhow::anyhow!("could not auto-detect install for '{game}'. Use --game-dir")
        })?,
    };

    println!(
        "Scanning: {} at {}",
        game_plugin.display_name(),
        install_dir.display()
    );
    println!("Scan directories: {:?}", scanner.scan_directories());

    // Build case-insensitive file index of the entire game directory.
    println!("Building file index...");
    let on_disk_files = build_file_index(&install_dir);
    println!("  {} files indexed", on_disk_files.len());

    // ── Manifest matching ───────────────────────────────────────────
    //
    // When the user passes `--manifest <foo>.wabbajack`, we parse the
    // manifest, match its archives against the on-disk file index, and
    // carry the resulting data forward so the import step below can
    // (a) reorder the target profile to follow the manifest's install
    // directive order and (b) apply a `LockReason::Wabbajack` retroactive
    // load-order lock. See `plans/greedy-shimmying-pine.md` for the design.
    let mut manifest_mods: Vec<EnabledMod> = Vec::new();
    let mut manifest_covered_files: HashSet<String> = HashSet::new();
    let mut parsed_manifest: Option<WabbajackManifest> = None;

    if let Some(manifest_path) = &manifest {
        let wj_manifest = modde_sources::wabbajack::manifest::parse_wabbajack_file(manifest_path)
            .with_context(|| {
            format!("failed to parse manifest: {}", manifest_path.display())
        })?;

        println!(
            "\nManifest: {} by {} ({} archives, {} directives)",
            wj_manifest.name,
            wj_manifest.author,
            wj_manifest.archives.len(),
            wj_manifest.directives.len(),
        );

        let matches = match_wabbajack_manifest(&wj_manifest, &on_disk_files, threshold);

        println!(
            "  {}/{} archives detected on disk (threshold: {:.0}%)",
            matches.len(),
            wj_manifest.archives.len(),
            threshold * 100.0,
        );

        if !matches.is_empty() {
            println!("\n  {:>5}  {:>6}  Mod Name", "Files", "Conf%");
            println!("  {:>5}  {:>6}  --------", "-----", "-----");
            for m in &matches {
                println!(
                    "  {:>3}/{:<3} {:>5.0}%  {}",
                    m.present_files,
                    m.total_files,
                    m.confidence * 100.0,
                    m.display_name,
                );
                // Collect covered file paths for deduplication with filesystem scan.
                manifest_covered_files.extend(m.covered_paths.iter().cloned());
                manifest_mods.push(manifest_match_to_enabled(m));
            }
        }

        parsed_manifest = Some(wj_manifest);
    }

    // ── Filesystem scanning ─────────────────────────────────────────
    let ctx = ScanContext {
        install_dir: &install_dir,
    };

    let fs_mods = scanner
        .scan_filesystem(&ctx)
        .context("filesystem scan failed")?;

    println!("\nFilesystem scan: {} mods discovered", fs_mods.len());

    if !fs_mods.is_empty() {
        println!(
            "\n  {:>5}  {:>6}  {:12}  Name",
            "Files", "Conf%", "Location"
        );
        println!("  {:>5}  {:>6}  {:12}  ----", "-----", "-----", "--------");
        for m in &fs_mods {
            let location = match &m.source {
                modde_games::ModSource::Filesystem { location } => location.as_str(),
                _ => "?",
            };
            println!(
                "  {:>5}  {:>5.0}%  {:12}  {}{}",
                m.files.len(),
                m.confidence * 100.0,
                location,
                m.display_name,
                m.version
                    .as_deref()
                    .map(|v| format!(" (v{v})"))
                    .unwrap_or_default(),
            );
        }
    }

    // ── Merge and import ────────────────────────────────────────────
    //
    // Build a set of directories the manifest writes into, for the
    // directory-prefix coverage check below. This is derived from the
    // per-file `manifest_covered_files` set by extracting every parent
    // directory of every covered file.
    //
    // Why we need this:
    //
    // The previous coverage check was purely per-file:
    //
    //     m.files.iter().all(|f| manifest_covered_files.contains(...))
    //
    // which required **every** file of a filesystem-discovered mod to be
    // covered by the manifest. That silently failed for directory-based
    // mods like CET (`bin/x64/plugins/cyber_engine_tweaks/mods/<name>/`)
    // because the filesystem scanner picks up runtime-generated files
    // (e.g. `settings.json` written by the game on first run) that the
    // manifest never deploys. One stray user file → whole mod marked as
    // uncovered → duplicate `cet/<name>` entry added next to the
    // `nexus_*` manifest entry. See profile 3077 for the pathological
    // case: 166 duplicate `cet/*` rows all shadowing existing Nexus mods.
    //
    // The fix below uses a directory-prefix check for directory-based
    // mods: if the mod has a distinct root directory AND the manifest
    // deploys any file under that root, treat the whole thing as
    // covered. Single-file mods (archive/pc/mod/*.archive) and mods
    // with no identifiable root still fall back to the per-file check.
    let manifest_covered_dirs: HashSet<String> = manifest_covered_files
        .iter()
        .flat_map(|f| dir_prefixes(f))
        .collect();

    if let Some(profile_name) = &import_to {
        let mut all_mods = manifest_mods;

        // Skip filesystem mods already installed by the manifest. Prefer
        // the directory-prefix check where possible (directory-based
        // mods); fall back to the per-file check otherwise.
        let mut fs_skipped = 0usize;
        for m in &fs_mods {
            let covered = if let Some(root) = mod_root_dir(m) {
                manifest_covered_dirs.contains(&root)
            } else {
                !m.files.is_empty()
                    && m.files
                        .iter()
                        .all(|f| manifest_covered_files.contains(&f.rel_path.to_lowercase()))
            };
            if covered {
                fs_skipped += 1;
            } else {
                all_mods.push(modde_core::scanner::discovered_to_enabled(
                    &m.mod_id,
                    &m.display_name,
                    m.version.as_deref(),
                    m.confidence as f32,
                ));
            }
        }
        if fs_skipped > 0 {
            println!("  ({fs_skipped} filesystem mods skipped — already covered by manifest)");
        }

        if dry_run {
            println!(
                "\n[DRY RUN] Would import {} mods into profile '{profile_name}'",
                all_mods.len(),
            );
            return Ok(());
        }

        let pm = ProfileManager::open()
            .await
            .context("failed to open profile database")?;

        // Load existing profile or create a new one.
        let mut profile = if let Ok(p) = pm
            .load(profile_name, Some(&GameId::from(game.as_str())))
            .await
        {
            p
        } else {
            println!("Creating new profile '{profile_name}' for game '{game}'");
            Profile {
                id: None,
                name: profile_name.clone(),
                game_id: GameId::from(game.clone()),
                source: ProfileSource::Manual,
                mods: Vec::new(),
                overrides: ProfileManager::default_overrides(profile_name),
                load_order_rules: smallvec::SmallVec::new(),
                load_order_lock: None,
            }
        };

        // ── Optional: prune leaked filesystem-scanner duplicates ────
        //
        // When `--prune-duplicates` is set, run the dedup classifier
        // against the profile BEFORE merging new mods. Any filesystem-
        // scanner row whose footprint is covered by the manifest gets
        // removed — those are leaked duplicates left behind by an older
        // buggy scan pass. Genuine additions (user mods not in the
        // manifest) are preserved untouched.
        if prune_duplicates {
            let Some(ref wj_manifest) = parsed_manifest else {
                // validated at entry; unreachable
                unreachable!("prune_duplicates without manifest should have bailed earlier");
            };
            let report =
                modde_core::scanner::detect_stale_duplicates(&profile, wj_manifest, |mod_id| {
                    scanner.mod_id_footprint(mod_id)
                });
            if report.leaked.is_empty() {
                println!(
                    "\nNo leaked duplicates found in '{profile_name}' \
                     ({} genuine fs-scanner additions untouched)",
                    report.genuine.len()
                );
            } else {
                let leaked_set: HashSet<&str> = report
                    .leaked
                    .iter()
                    .map(std::string::String::as_str)
                    .collect();
                let before = profile.mods.len();
                profile
                    .mods
                    .retain(|m| !leaked_set.contains(m.mod_id.as_str()));
                let removed = before - profile.mods.len();
                println!(
                    "\nPruned {removed} leaked duplicate(s) from '{profile_name}' \
                     (kept {} genuine addition(s))",
                    report.genuine.len()
                );
            }
        }

        // Merge: add any mods from `all_mods` that the profile doesn't
        // already track. Existing entries keep their per-mod state (notes,
        // category, per-mod lock, etc.) — we only bring in new mod_ids.
        let existing_ids: HashSet<String> = profile.mods.iter().map(|m| m.mod_id.clone()).collect();
        let new_mods: Vec<EnabledMod> = all_mods
            .into_iter()
            .filter(|m| !existing_ids.contains(&m.mod_id))
            .collect();

        let added = new_mods.len();
        let prior = profile.mods.len();
        profile.mods.extend(new_mods);

        // ── Retroactive Wabbajack lock ──────────────────────────────
        //
        // When `--manifest` was provided, delegate to the pure
        // `apply_wabbajack_lock` helper in modde-core::scanner. It
        // preserves mod count (matched-first, unmatched-after) and
        // stamps a `LockReason::Wabbajack` lock.
        if let Some(ref wj_manifest) = parsed_manifest {
            let report = apply_wabbajack_lock(&mut profile, wj_manifest);
            println!(
                "\nApplied Wabbajack load order lock to '{profile_name}' (manifest_hash={}){}",
                report.manifest_hash,
                if report.replaced_existing_lock {
                    " — overwrote existing lock"
                } else {
                    ""
                }
            );
            println!(
                "  Reordered {} mod(s) by manifest directive order ({} unmatched left in place).",
                report.matched, report.unmatched,
            );

            // Stash the source .wabbajack file in the content-addressed cache
            // so `lock-info` can find it later. Log-and-continue on failure —
            // the lock itself is already applied.
            if let Some(ref manifest_path) = manifest
                && let Err(e) = modde_core::manifest::wabbajack::cache_wabbajack_file(
                    manifest_path,
                    &report.manifest_hash,
                )
            {
                tracing::warn!("failed to cache wabbajack source file: {e:#}");
            }
        }

        pm.create_or_update(&profile)
            .await
            .context("failed to save profile")?;

        println!(
            "\nImported {added} new mods into profile '{profile_name}' (had {prior} tracked before)",
        );
    } else if !dry_run {
        println!("\nUse --import-to <profile> to save discovered mods to a profile.");
    }

    Ok(())
}

/// Every ancestor directory of `path`, normalised to forward slashes,
/// lowercased, and terminated with a trailing `/`.
///
/// `"a/b/c.txt"` → `["a/b/", "a/"]`. Used to build the set of directories
/// the manifest covers from its flat file list.
fn dir_prefixes(path: &str) -> Vec<String> {
    let norm = path.replace('\\', "/").to_lowercase();
    let mut out = Vec::new();
    let mut cur = norm.as_str();
    while let Some(idx) = cur.rfind('/') {
        cur = &cur[..idx];
        out.push(format!("{cur}/"));
    }
    out
}

/// Longest common directory prefix across all files in a filesystem mod,
/// or `None` if the mod has no meaningful root (single-file mods like
/// `archive/pc/mod/foo.archive` fall into this bucket).
///
/// The returned path is lowercased, uses forward slashes, and ends with a
/// trailing `/` so it can be compared against `dir_prefixes` output.
///
/// For a directory-based mod (CET, `REDscript`, `REDmod`, ...) with files
/// like `bin/x64/plugins/cyber_engine_tweaks/mods/ImmersiveHealing/init.lua`
/// and `.../ImmersiveHealing/settings.json`, this returns
/// `"bin/x64/plugins/cyber_engine_tweaks/mods/immersivehealing/"` — the
/// mod's own folder. A single-file mod returns `None` because its
/// "common prefix" is the file itself, not a directory that identifies
/// the mod beyond its scan location.
fn mod_root_dir(m: &modde_games::DiscoveredMod) -> Option<String> {
    let (first_file, rest) = m.files.split_first()?;
    if rest.is_empty() {
        return None;
    }
    let normalize = |p: &str| p.replace('\\', "/").to_lowercase();
    let first = normalize(&first_file.rel_path);
    let mut common_len = first.len();
    for f in rest {
        let other = normalize(&f.rel_path);
        let n = first
            .bytes()
            .zip(other.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        if n < common_len {
            common_len = n;
        }
    }
    // common_len is a byte count from a byte-wise prefix comparison; walk back to
    // a char boundary so slicing a path with multibyte characters can't panic.
    while !first.is_char_boundary(common_len) {
        common_len -= 1;
    }
    let common = &first[..common_len];
    common.rfind('/').map(|i| common[..=i].to_string())
}

/// Walk the entire game directory and build a set of lowercased, forward-slash
/// relative paths for case-insensitive matching.
fn build_file_index(root: &std::path::Path) -> HashSet<String> {
    let mut files = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root) {
                let normalized = rel.to_string_lossy().replace('\\', "/").to_lowercase();
                files.insert(normalized);
            }
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_games::{DiscoveredFile, DiscoveredMod, ModSource};

    fn discovered(paths: &[&str]) -> DiscoveredMod {
        DiscoveredMod {
            mod_id: "m".into(),
            display_name: "m".into(),
            version: None,
            files: paths
                .iter()
                .map(|p| DiscoveredFile {
                    rel_path: (*p).to_string(),
                    size: 1,
                })
                .collect(),
            source: ModSource::Filesystem {
                location: "mods".into(),
            },
            confidence: 1.0,
        }
    }

    #[test]
    fn dir_prefixes_lists_normalised_ancestors() {
        assert_eq!(dir_prefixes("A\\B/c.TXT"), ["a/b/", "a/"]);
        assert_eq!(dir_prefixes("file.txt"), [] as [&str; 0]);
    }

    #[test]
    fn mod_root_dir_is_the_common_directory() {
        assert_eq!(mod_root_dir(&discovered(&["mods/x.archive"])), None);
        assert_eq!(
            mod_root_dir(&discovered(&["Mods\\Heal\\init.lua", "mods/heal/s.json"])),
            Some("mods/heal/".into())
        );
        // A shared name prefix is not a shared directory.
        assert_eq!(
            mod_root_dir(&discovered(&["mods/abc/x", "mods/abd/y"])),
            Some("mods/".into())
        );
        // Multibyte characters diverging mid-codepoint must not panic.
        assert_eq!(
            mod_root_dir(&discovered(&["mods/é/a", "mods/è/b"])),
            Some("mods/".into())
        );
        assert_eq!(mod_root_dir(&discovered(&["a.txt", "b.txt"])), None);
    }
}
