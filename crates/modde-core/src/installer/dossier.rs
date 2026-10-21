//! Unknown-mod dossier writer.
//!
//! When [`analyze`](super::analyze::analyze) returns
//! [`InstallMethod::Unknown`] or
//! [`execute`](super::execute::execute) surfaces
//! [`InstallerError::UnknownMethod`](super::types::InstallerError::UnknownMethod),
//! the caller dumps a dossier so a Claude Code skill can extend the
//! installer to handle this layout.
//!
//! Layout on disk:
//!
//! ```text
//! $XDG_DATA_HOME/modde/unknown-installers/<slug>/
//!   ├─ metadata.json        — Nexus mod metadata + context
//!   ├─ archive_tree.txt     — recursive ls (up to 500 entries)
//!   ├─ file_samples/        — up to 5 small text files verbatim
//!   ├─ analyzer_trace.json  — which probes ran, what they rejected
//!   └─ PROMPT.md            — self-contained skill prompt
//! ```
//!
//! Once a skill has landed a fix, the dossier directory is renamed to
//! `<slug>.resolved` and the UI surfaces a **Retry Install** button.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::nexus_id::{NexusFileId, NexusModId};
use crate::paths::modde_data_dir;

use super::fs::walk_files;
use super::types::{InstallMethod, InstallerResult};

/// Context about the mod the analyzer could not classify. Filled in by
/// the CLI / UI caller — the installer crate itself doesn't fetch Nexus
/// metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DossierContext {
    pub game_id: String,
    pub game_domain: Option<String>,
    pub nexus_mod_id: Option<NexusModId>,
    pub nexus_file_id: Option<NexusFileId>,
    pub mod_name: String,
    pub mod_author: Option<String>,
    pub mod_version: Option<String>,
    pub mod_summary: Option<String>,
    pub nexus_url: Option<String>,
    /// xxh64 hex digest of the source archive, for dedup.
    pub source_archive_hash: String,
}

/// A slug identifying the dossier dir. Stable across retries.
#[must_use]
pub fn dossier_slug(ctx: &DossierContext) -> String {
    match (
        ctx.game_domain.as_deref(),
        ctx.nexus_mod_id,
        ctx.nexus_file_id,
    ) {
        (Some(domain), Some(mod_id), Some(file_id)) => {
            format!("{domain}_{mod_id}_{file_id}")
        }
        (Some(domain), Some(mod_id), None) => format!("{domain}_{mod_id}"),
        _ => format!(
            "{}_{}",
            ctx.game_id,
            &ctx.source_archive_hash[..8.min(ctx.source_archive_hash.len())]
        ),
    }
}

/// Root directory holding all unknown-installer dossiers.
#[must_use]
pub fn dossiers_dir() -> PathBuf {
    modde_data_dir().join("unknown-installers")
}

/// Path to the dossier for a specific unknown mod.
#[must_use]
pub fn dossier_path(ctx: &DossierContext) -> PathBuf {
    dossiers_dir().join(dossier_slug(ctx))
}

/// Write the dossier to disk. Overwrites any existing dossier with the
/// same slug (this is idempotent — the skill path consumes by renaming
/// to `.resolved`, so a pre-existing dir here means a prior failed
/// attempt we're retrying).
pub fn dump(
    extracted_dir: &Path,
    ctx: &DossierContext,
    method: &InstallMethod,
    analyzer_trace: Vec<ProbeTrace>,
) -> InstallerResult<PathBuf> {
    let out = dossier_path(ctx);
    if out.exists() {
        let _ = fs::remove_dir_all(&out);
    }
    fs::create_dir_all(&out)?;

    // metadata.json
    let metadata_path = out.join("metadata.json");
    let metadata_bytes = serde_json::to_vec_pretty(ctx).map_err(io_err)?;
    fs::write(&metadata_path, metadata_bytes)?;

    // archive_tree.txt
    write_archive_tree(extracted_dir, &out.join("archive_tree.txt"))?;

    // file_samples/
    copy_text_samples(extracted_dir, &out.join("file_samples"))?;

    // analyzer_trace.json
    let trace_path = out.join("analyzer_trace.json");
    let trace_bytes = serde_json::to_vec_pretty(&analyzer_trace).map_err(io_err)?;
    fs::write(&trace_path, trace_bytes)?;

    // PROMPT.md
    write_prompt(&out, ctx, method)?;

    Ok(out)
}

/// A single probe's decision, recorded so the skill knows which paths
/// were already tried.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeTrace {
    pub probe: String,
    pub matched: bool,
    pub note: String,
}

fn write_archive_tree(extracted_dir: &Path, out: &Path) -> InstallerResult<()> {
    const MAX_ENTRIES: usize = 500;
    let mut file = fs::File::create(out)?;
    let files = walk_files(extracted_dir)?;
    let mut count = 0;
    for (_, rel) in &files {
        if count >= MAX_ENTRIES {
            writeln!(
                file,
                "... ({} more entries omitted)",
                files.len() - MAX_ENTRIES
            )?;
            break;
        }
        writeln!(file, "{}", rel.display())?;
        count += 1;
    }
    Ok(())
}

fn copy_text_samples(extracted_dir: &Path, out: &Path) -> InstallerResult<()> {
    const MAX_SAMPLES: usize = 5;
    const MAX_BYTES: u64 = 16 * 1024;
    const INTERESTING: &[&str] = &[
        "readme",
        "info.json",
        "moduleconfig.xml",
        "meta.ini",
        "manifest.json",
        "install.xml",
        "fomod.xml",
        "modinfo.xml",
    ];

    fs::create_dir_all(out)?;
    let files = walk_files(extracted_dir)?;
    let mut written = 0;
    for (abs, rel) in files {
        if written >= MAX_SAMPLES {
            break;
        }
        let name = abs
            .file_name()
            .and_then(|n| n.to_str())
            .map(str::to_ascii_lowercase)
            .unwrap_or_default();
        let matches = INTERESTING.iter().any(|needle| name.contains(needle));
        if !matches {
            continue;
        }
        let Ok(meta) = fs::metadata(&abs) else {
            continue;
        };
        if meta.len() > MAX_BYTES {
            continue;
        }
        let body = match fs::read_to_string(&abs) {
            Ok(b) => b,
            Err(_) => continue,
        };
        // Flatten the rel path into a single filename so the samples
        // directory stays shallow.
        let flat = rel.to_string_lossy().replace(['/', '\\'], "__");
        let sample_path = out.join(flat);
        fs::write(&sample_path, body)?;
        written += 1;
    }
    Ok(())
}

fn write_prompt(dir: &Path, ctx: &DossierContext, method: &InstallMethod) -> InstallerResult<()> {
    let mut prompt = String::new();
    prompt.push_str("# modde unknown installer dossier\n\n");
    prompt.push_str(&format!("Mod: **{}**\n", ctx.mod_name));
    if let Some(author) = &ctx.mod_author {
        prompt.push_str(&format!("Author: {author}\n"));
    }
    if let Some(version) = &ctx.mod_version {
        prompt.push_str(&format!("Version: {version}\n"));
    }
    prompt.push_str(&format!("Game: {}\n", ctx.game_id));
    if let Some(url) = &ctx.nexus_url {
        prompt.push_str(&format!("Nexus URL: {url}\n"));
    }
    prompt.push_str(&format!(
        "Detection verdict: `{}` (reason: {})\n\n",
        method.label(),
        match method {
            InstallMethod::Unknown { reason } => reason.as_str(),
            _ => "execute-time rejection",
        }
    ));

    prompt.push_str("## What to do\n\n");
    prompt.push_str(
        "modde's generic install-type detector couldn't classify this mod. \
         Extend the installer so the retry path can stage it:\n\n",
    );
    prompt.push_str(
        "1. Read `archive_tree.txt` and `file_samples/` to understand the \
         layout.\n",
    );
    prompt.push_str(
        "2. Decide where the fix belongs:\n   \
         - **Generic** (e.g. a new package format): add a variant to \
           `crates/modde-core/src/installer/types.rs::InstallMethod` and a \
           detection branch in `analyze.rs::detect_method`.\n   \
         - **Game-specific** (e.g. a weird Cyberpunk layout): extend \
           `crates/modde-games/src/<game>/mod.rs` with the new rule inside \
           that game's `analyze_mod_archive` impl.\n",
    );
    prompt.push_str(
        "3. Also extend `execute.rs` if the new variant needs custom \
         staging logic.\n",
    );
    prompt.push_str("4. Write a unit test using the file samples in this dossier.\n");
    prompt.push_str("5. Run `cargo test -p modde-core installer::`.\n");
    prompt.push_str(
        "6. Rename this dossier directory by appending `.resolved` so the \
         UI surfaces a **Retry Install** button.\n\n",
    );

    prompt.push_str("## Files in this dossier\n\n");
    prompt.push_str("- `metadata.json` — mod + context info\n");
    prompt.push_str("- `archive_tree.txt` — recursive listing of the extracted archive\n");
    prompt.push_str(
        "- `file_samples/` — verbatim copies of small text files (READMEs, configs, manifests)\n",
    );
    prompt.push_str("- `analyzer_trace.json` — which generic probes ran and how they voted\n");
    prompt.push_str("- `PROMPT.md` — this file\n\n");

    prompt.push_str("## Critical files\n\n");
    prompt.push_str("- [crates/modde-core/src/installer/types.rs](crates/modde-core/src/installer/types.rs) — `InstallMethod`, `InstallPlan`, `InstallerError`\n");
    prompt.push_str("- [crates/modde-core/src/installer/analyze.rs](crates/modde-core/src/installer/analyze.rs) — detection pipeline\n");
    prompt.push_str("- [crates/modde-core/src/installer/execute.rs](crates/modde-core/src/installer/execute.rs) — plan execution\n");
    prompt.push_str("- [crates/modde-games/src/traits.rs](crates/modde-games/src/traits.rs) — `GamePlugin::analyze_mod_archive` hook\n");

    fs::write(dir.join("PROMPT.md"), prompt)?;
    Ok(())
}

fn io_err(e: serde_json::Error) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(domain: Option<&str>, mod_id: Option<u64>, file_id: Option<u64>, hash: &str) -> DossierContext {
        DossierContext {
            game_id: "skyrim-se".to_string(),
            game_domain: domain.map(String::from),
            nexus_mod_id: mod_id.map(NexusModId::from),
            nexus_file_id: file_id.map(NexusFileId::from),
            mod_name: "Mod".to_string(),
            mod_author: None,
            mod_version: None,
            mod_summary: None,
            nexus_url: None,
            source_archive_hash: hash.to_string(),
        }
    }

    #[test]
    fn slug_prefers_nexus_identity_then_archive_hash() {
        let hash = "0123456789abcdef";
        assert_eq!(dossier_slug(&ctx(Some("skyrimspecialedition"), Some(1), Some(2), hash)), "skyrimspecialedition_1_2");
        assert_eq!(dossier_slug(&ctx(Some("skyrimspecialedition"), Some(1), None, hash)), "skyrimspecialedition_1");
        assert_eq!(dossier_slug(&ctx(None, Some(1), Some(2), hash)), "skyrim-se_01234567");
        assert_eq!(dossier_slug(&ctx(None, None, None, "abc")), "skyrim-se_abc");
    }
}
