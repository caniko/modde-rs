//! Imports user-supplied archive files into the Wabbajack download store by
//! matching their xxHash64 against a [`WabbajackManifest`], hard-linking or
//! copying matches into place and classifying each input via
//! [`ArchiveImportStatus`].

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use modde_core::manifest::wabbajack::WabbajackManifest;

use super::installer::archive_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveImportStatus {
    Imported,
    AlreadyPresent,
    Mismatched,
    Unused,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveImportResult {
    pub source_path: PathBuf,
    pub status: ArchiveImportStatus,
    pub computed_xxh64: u64,
    pub matched_archive: Option<String>,
    pub store_path: Option<PathBuf>,
}

pub async fn import_archives(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    archive_paths: &[PathBuf],
) -> Result<Vec<ArchiveImportResult>> {
    let mut results = Vec::with_capacity(archive_paths.len());
    tokio::fs::create_dir_all(store_dir)
        .await
        .with_context(|| format!("failed to create {}", store_dir.display()))?;

    for source_path in archive_paths {
        let computed_xxh64 = modde_core::hash::hash_file_xxh64(source_path)
            .await
            .with_context(|| format!("failed to hash {}", source_path.display()))?;
        let Some(archive) = manifest
            .archives
            .iter()
            .find(|archive| archive.hash == computed_xxh64)
        else {
            results.push(ArchiveImportResult {
                source_path: source_path.clone(),
                status: if manifest.archives.iter().any(|archive| {
                    archive.name
                        == source_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or_default()
                }) {
                    ArchiveImportStatus::Mismatched
                } else {
                    ArchiveImportStatus::Unused
                },
                computed_xxh64,
                matched_archive: None,
                store_path: None,
            });
            continue;
        };

        let dest = archive_path(store_dir, &archive.hash);
        if dest.exists()
            && modde_core::hash::verify_xxh64(&dest, archive.hash)
                .await
                .is_ok()
        {
            results.push(ArchiveImportResult {
                source_path: source_path.clone(),
                status: ArchiveImportStatus::AlreadyPresent,
                computed_xxh64,
                matched_archive: Some(archive.name.clone()),
                store_path: Some(dest),
            });
            continue;
        }

        if let Some(parent) = dest.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        if tokio::fs::hard_link(source_path, &dest).await.is_err() {
            tokio::fs::copy(source_path, &dest).await.with_context(|| {
                format!(
                    "failed to import {} to {}",
                    source_path.display(),
                    dest.display()
                )
            })?;
        }
        modde_core::hash::verify_xxh64(&dest, archive.hash)
            .await
            .with_context(|| format!("imported archive failed verification: {}", dest.display()))?;

        results.push(ArchiveImportResult {
            source_path: source_path.clone(),
            status: ArchiveImportStatus::Imported,
            computed_xxh64,
            matched_archive: Some(archive.name.clone()),
            store_path: Some(dest),
        });
    }

    Ok(results)
}

impl std::fmt::Display for ArchiveImportStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Imported => "imported",
            Self::AlreadyPresent => "already present",
            Self::Mismatched => "hash mismatch",
            Self::Unused => "not in modlist",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use modde_core::manifest::wabbajack::ArchiveEntry;
    use xxhash_rust::xxh64::xxh64;

    fn manifest_for(bytes: &[u8], name: &str) -> WabbajackManifest {
        WabbajackManifest {
            name: "test".into(),
            author: "a".into(),
            description: "d".into(),
            game: "SkyrimSE".into(),
            version: "1".into(),
            archives: vec![ArchiveEntry {
                hash: xxh64(bytes, 0),
                name: name.into(),
                size: bytes.len() as u64,
                state: None,
            }],
            directives: vec![],
        }
    }

    #[tokio::test]
    async fn wabbajack_archive_import_imports_exact_hash() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("archive.7z");
        tokio::fs::write(&archive, b"archive bytes").await.unwrap();
        let manifest = manifest_for(b"archive bytes", "archive.7z");

        let results = import_archives(&manifest, &temp.path().join("store"), &[archive])
            .await
            .unwrap();

        assert_eq!(results[0].status, ArchiveImportStatus::Imported);
        assert!(results[0].store_path.as_ref().unwrap().exists());
    }

    #[tokio::test]
    async fn wabbajack_archive_import_accepts_already_present() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("archive.7z");
        tokio::fs::write(&archive, b"archive bytes").await.unwrap();
        let manifest = manifest_for(b"archive bytes", "archive.7z");
        let store = temp.path().join("store");

        import_archives(&manifest, &store, std::slice::from_ref(&archive))
            .await
            .unwrap();
        let results = import_archives(&manifest, &store, &[archive])
            .await
            .unwrap();

        assert_eq!(results[0].status, ArchiveImportStatus::AlreadyPresent);
    }

    #[tokio::test]
    async fn wabbajack_archive_import_refuses_name_only_mismatch() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("archive.7z");
        tokio::fs::write(&archive, b"wrong bytes").await.unwrap();
        let manifest = manifest_for(b"archive bytes", "archive.7z");

        let results = import_archives(&manifest, &temp.path().join("store"), &[archive])
            .await
            .unwrap();

        assert_eq!(results[0].status, ArchiveImportStatus::Mismatched);
        assert!(results[0].store_path.is_none());
    }

    #[tokio::test]
    async fn wabbajack_archive_import_refuses_unreferenced_archive() {
        let temp = tempfile::tempdir().unwrap();
        let archive = temp.path().join("unused.7z");
        tokio::fs::write(&archive, b"unused bytes").await.unwrap();
        let manifest = manifest_for(b"archive bytes", "archive.7z");

        let results = import_archives(&manifest, &temp.path().join("store"), &[archive])
            .await
            .unwrap();

        assert_eq!(results[0].status, ArchiveImportStatus::Unused);
    }
}
