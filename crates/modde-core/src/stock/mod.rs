use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use xxhash_rust::xxh3::xxh3_64;

use crate::db::ModdeDb;
use crate::error::{CoreError, Result};
use crate::hash::hash_file_xxhash;
use crate::resolver::GameId;

const TREE_HASH_FILENAME: &str = ".modde-tree-hash.toml";

/// Manages vanilla game snapshots for stock game preservation.
pub struct StockGameManager {
    store_dir: PathBuf,
    db: Option<ModdeDb>,
}

/// A snapshot of a vanilla game installation.
#[derive(Debug)]
pub struct StockSnapshot {
    pub game_id: GameId,
    pub path: PathBuf,
    pub hash: String,
}

/// Persisted tree hash metadata stored alongside a snapshot.
#[derive(Debug, Serialize, Deserialize)]
struct TreeHashMeta {
    /// Hex-encoded xxh3-64 hash of the sorted path+hash pairs.
    tree_hash: String,
    /// Number of files included in the hash.
    file_count: usize,
}

impl StockGameManager {
    #[must_use]
    pub fn new(store_dir: PathBuf) -> Self {
        Self {
            store_dir,
            db: None,
        }
    }

    /// Create a manager backed by both filesystem and `SQLite`.
    pub fn with_db(store_dir: PathBuf, db: ModdeDb) -> Self {
        Self {
            store_dir,
            db: Some(db),
        }
    }

    /// Default store directory: `~/.local/share/modde/stock/`.
    #[must_use]
    pub fn default_dir() -> PathBuf {
        crate::paths::stock_dir()
    }

    /// Detect Steam install path for a given game.
    pub fn detect_steam_install(&self, game_dir_name: &str) -> Option<PathBuf> {
        let game_path = crate::paths::steam_common().join(game_dir_name);
        game_path.exists().then_some(game_path)
    }

    /// Create a hardlink snapshot of a game installation.
    ///
    /// Falls back to file copy if source and destination are on different filesystems.
    pub async fn snapshot(&self, game_id: &GameId, source_dir: &Path) -> Result<StockSnapshot> {
        if !source_dir.exists() {
            return Err(CoreError::GameNotDetected(game_id.to_string()));
        }

        let snapshot_dir = self.store_dir.join(game_id.as_str());
        tokio::fs::create_dir_all(&snapshot_dir).await?;

        // Walk source and hardlink/copy files
        snapshot_recursive(source_dir, &snapshot_dir).await?;

        // Compute and store the tree hash (dual-write: file + DB)
        let tree_hash = compute_tree_hash(&snapshot_dir).await?;
        store_tree_hash(&snapshot_dir, &tree_hash).await?;

        if let Some(ref db) = self.db {
            db.upsert_snapshot(
                game_id,
                &snapshot_dir,
                &tree_hash.tree_hash,
                tree_hash.file_count,
            )
            .await?;
        }

        info!(game_id = %game_id, path = %snapshot_dir.display(), hash = %tree_hash.tree_hash, "stock snapshot created");

        Ok(StockSnapshot {
            game_id: game_id.clone(),
            path: snapshot_dir,
            hash: tree_hash.tree_hash,
        })
    }

    /// Verify an existing snapshot still matches the stored tree hash.
    pub async fn verify(&self, game_id: &GameId) -> Result<bool> {
        let snapshot_dir = self.store_dir.join(game_id.as_str());
        if !snapshot_dir.exists() {
            return Err(CoreError::Other(
                format!("no snapshot found for game '{game_id}'").into(),
            ));
        }

        let stored = load_tree_hash(&snapshot_dir).await?;
        let current = compute_tree_hash(&snapshot_dir).await?;

        if stored.tree_hash == current.tree_hash {
            info!(game_id = %game_id, hash = %stored.tree_hash, "snapshot verified OK");
            Ok(true)
        } else {
            warn!(
                game_id = %game_id,
                expected = %stored.tree_hash,
                actual = %current.tree_hash,
                "snapshot verification failed: tree hash mismatch"
            );
            Ok(false)
        }
    }
}

/// Walk a directory recursively, collecting `(relative_path, xxhash)` for every file.
async fn collect_file_hashes(
    root: &Path,
    dir: &Path,
    entries: &mut Vec<(String, u64)>,
) -> Result<()> {
    let mut read_dir = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read_dir.next_entry().await? {
        let path = entry.path();
        let file_type = entry.file_type().await?;

        // Skip the metadata file itself
        if path.file_name().and_then(|n| n.to_str()) == Some(TREE_HASH_FILENAME) {
            continue;
        }

        if file_type.is_dir() {
            Box::pin(collect_file_hashes(root, &path, entries)).await?;
        } else if file_type.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let hash = hash_file_xxhash(&path).await?;
            entries.push((rel, hash));
        }
    }
    Ok(())
}

/// Compute a deterministic tree hash for an entire directory.
///
/// The hash is the xxh3-64 of all `"<relative_path>\0<hash_hex>\n"` pairs,
/// sorted by relative path for determinism.
async fn compute_tree_hash(dir: &Path) -> Result<TreeHashMeta> {
    let mut entries = Vec::new();
    collect_file_hashes(dir, dir, &mut entries).await?;

    // Sort by relative path for deterministic ordering
    entries.sort_by(|a, b| a.0.cmp(&b.0));

    // Build the concatenated representation and hash it
    let mut combined = Vec::new();
    for (rel_path, hash) in &entries {
        combined.extend_from_slice(rel_path.as_bytes());
        combined.push(0);
        combined.extend_from_slice(format!("{hash:016x}").as_bytes());
        combined.push(b'\n');
    }

    let tree_hash = xxh3_64(&combined);

    Ok(TreeHashMeta {
        tree_hash: format!("{tree_hash:016x}"),
        file_count: entries.len(),
    })
}

/// Write tree hash metadata to the snapshot directory.
async fn store_tree_hash(snapshot_dir: &Path, meta: &TreeHashMeta) -> Result<()> {
    let meta_path = snapshot_dir.join(TREE_HASH_FILENAME);
    let toml_str = toml::to_string_pretty(meta)?;
    tokio::fs::write(&meta_path, toml_str.as_bytes()).await?;
    Ok(())
}

/// Load tree hash metadata from the snapshot directory.
async fn load_tree_hash(snapshot_dir: &Path) -> Result<TreeHashMeta> {
    let meta_path = snapshot_dir.join(TREE_HASH_FILENAME);
    let data = tokio::fs::read_to_string(&meta_path).await.map_err(|_| {
        CoreError::Other(format!("tree hash metadata not found at {}", meta_path.display()).into())
    })?;
    let meta: TreeHashMeta = toml::from_str(&data)?;
    Ok(meta)
}

async fn snapshot_recursive(src: &Path, dst: &Path) -> Result<()> {
    let mut entries = tokio::fs::read_dir(src).await?;
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            tokio::fs::create_dir_all(&dst_path).await?;
            Box::pin(snapshot_recursive(&src_path, &dst_path)).await?;
        } else if file_type.is_file() {
            let kind = crate::link::link_or_copy(&src_path, &dst_path).await?;
            tracing::debug!(src = %src_path.display(), dst = %dst_path.display(), ?kind, "stock snapshot linked file");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_tree(dir: &Path) {
        // Create a small directory tree with known content
        std::fs::create_dir_all(dir.join("subdir")).unwrap();
        let mut f1 = std::fs::File::create(dir.join("file_a.txt")).unwrap();
        f1.write_all(b"hello world").unwrap();
        let mut f2 = std::fs::File::create(dir.join("subdir/file_b.txt")).unwrap();
        f2.write_all(b"nested content").unwrap();
    }

    #[tokio::test]
    async fn test_tree_hash_deterministic() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(tmp.path());

        let h1 = compute_tree_hash(tmp.path()).await.unwrap();
        let h2 = compute_tree_hash(tmp.path()).await.unwrap();
        assert_eq!(h1.tree_hash, h2.tree_hash);
        assert_eq!(h1.file_count, 2);
    }

    #[tokio::test]
    async fn test_tree_hash_changes_on_modification() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(tmp.path());

        let h1 = compute_tree_hash(tmp.path()).await.unwrap();

        // Modify a file
        std::fs::write(tmp.path().join("file_a.txt"), b"changed").unwrap();

        let h2 = compute_tree_hash(tmp.path()).await.unwrap();
        assert_ne!(h1.tree_hash, h2.tree_hash);
    }

    #[tokio::test]
    async fn test_store_and_load_tree_hash() {
        let tmp = TempDir::new().unwrap();
        create_test_tree(tmp.path());

        let hash = compute_tree_hash(tmp.path()).await.unwrap();
        store_tree_hash(tmp.path(), &hash).await.unwrap();

        let loaded = load_tree_hash(tmp.path()).await.unwrap();
        assert_eq!(hash.tree_hash, loaded.tree_hash);
        assert_eq!(hash.file_count, loaded.file_count);
    }

    #[tokio::test]
    async fn test_snapshot_and_verify() {
        let src = TempDir::new().unwrap();
        create_test_tree(src.path());

        let store = TempDir::new().unwrap();
        let mgr = StockGameManager::new(store.path().to_path_buf());

        let snap = mgr
            .snapshot(&GameId::from("test-game"), src.path())
            .await
            .unwrap();
        assert_ne!(snap.hash, "");

        let ok = mgr.verify(&GameId::from("test-game")).await.unwrap();
        assert!(ok);
    }
}
