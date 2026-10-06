use std::path::PathBuf;

use modde_core::resolver::GameId;
use modde_core::stock::StockGameManager;
use tempfile::TempDir;

fn create_test_tree(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join("subdir")).unwrap();
    std::fs::write(dir.join("file_a.txt"), b"hello world").unwrap();
    std::fs::write(dir.join("subdir/file_b.txt"), b"nested content").unwrap();
}

// ── Snapshot edge cases ─────────────────────────────────────────────

#[tokio::test]
async fn test_snapshot_nonexistent_source_returns_game_not_detected() {
    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let result = mgr
        .snapshot(
            &GameId::from("fake-game"),
            &PathBuf::from("/nonexistent/path/to/game"),
        )
        .await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("not detected"), "unexpected error: {err}");
}

#[tokio::test]
async fn test_snapshot_empty_directory() {
    let src = TempDir::new().unwrap();
    // empty directory, no files
    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let snap = mgr
        .snapshot(&GameId::from("empty-game"), src.path())
        .await
        .unwrap();
    assert_ne!(snap.hash, "");
    // Verify still passes (empty tree is consistent)
    let ok = mgr.verify(&GameId::from("empty-game")).await.unwrap();
    assert!(ok);
}

#[tokio::test]
async fn test_snapshot_single_file() {
    let src = TempDir::new().unwrap();
    std::fs::write(src.path().join("only.txt"), b"solo").unwrap();

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let snap = mgr
        .snapshot(&GameId::from("single-file"), src.path())
        .await
        .unwrap();
    assert_ne!(snap.hash, "");

    let ok = mgr.verify(&GameId::from("single-file")).await.unwrap();
    assert!(ok);
}

#[tokio::test]
async fn test_snapshot_deeply_nested() {
    let src = TempDir::new().unwrap();
    let deep = src.path().join("a/b/c/d/e");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("deep.txt"), b"deep content").unwrap();

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let snap = mgr
        .snapshot(&GameId::from("deep-game"), src.path())
        .await
        .unwrap();
    let ok = mgr.verify(&GameId::from("deep-game")).await.unwrap();
    assert!(ok);
    assert_ne!(snap.hash, "");
}

#[tokio::test]
async fn test_verify_fails_after_file_modification() {
    let src = TempDir::new().unwrap();
    create_test_tree(src.path());

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    mgr.snapshot(&GameId::from("mod-test"), src.path())
        .await
        .unwrap();

    // Modify a file in the snapshot directly
    let snapshot_file = store.path().join("mod-test/file_a.txt");
    std::fs::write(&snapshot_file, b"tampered content").unwrap();

    let ok = mgr.verify(&GameId::from("mod-test")).await.unwrap();
    assert!(!ok, "verify should fail after file modification");
}

#[tokio::test]
async fn test_verify_fails_after_file_deletion() {
    let src = TempDir::new().unwrap();
    create_test_tree(src.path());

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    mgr.snapshot(&GameId::from("del-test"), src.path())
        .await
        .unwrap();

    // Delete a file from the snapshot
    let snapshot_file = store.path().join("del-test/file_a.txt");
    std::fs::remove_file(&snapshot_file).unwrap();

    let ok = mgr.verify(&GameId::from("del-test")).await.unwrap();
    assert!(!ok, "verify should fail after file deletion");
}

#[tokio::test]
async fn test_verify_fails_after_file_addition() {
    let src = TempDir::new().unwrap();
    create_test_tree(src.path());

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    mgr.snapshot(&GameId::from("add-test"), src.path())
        .await
        .unwrap();

    // Add a new file to the snapshot
    std::fs::write(store.path().join("add-test/extra.txt"), b"extra").unwrap();

    let ok = mgr.verify(&GameId::from("add-test")).await.unwrap();
    assert!(!ok, "verify should fail after extra file added");
}

#[tokio::test]
async fn test_verify_nonexistent_snapshot() {
    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let result = mgr.verify(&GameId::from("nonexistent-game")).await;
    assert!(result.is_err());
    let err = format!("{}", result.unwrap_err());
    assert!(err.contains("no snapshot found"), "unexpected error: {err}");
}

#[tokio::test]
async fn test_snapshot_preserves_content_via_hardlink_or_copy() {
    let src = TempDir::new().unwrap();
    std::fs::write(src.path().join("data.bin"), b"binary data here").unwrap();

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    mgr.snapshot(&GameId::from("content-test"), src.path())
        .await
        .unwrap();

    let snapshot_content = std::fs::read(store.path().join("content-test/data.bin")).unwrap();
    assert_eq!(snapshot_content, b"binary data here");
}

#[tokio::test]
async fn test_snapshot_creates_store_subdirectory() {
    let src = TempDir::new().unwrap();
    std::fs::write(src.path().join("file.txt"), b"test").unwrap();

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    mgr.snapshot(&GameId::from("new-game-dir"), src.path())
        .await
        .unwrap();

    assert!(store.path().join("new-game-dir").exists());
    assert!(store.path().join("new-game-dir").is_dir());
}

#[tokio::test]
async fn test_two_snapshots_different_games_independent() {
    let src1 = TempDir::new().unwrap();
    std::fs::write(src1.path().join("game1.txt"), b"game1").unwrap();

    let src2 = TempDir::new().unwrap();
    std::fs::write(src2.path().join("game2.txt"), b"game2").unwrap();

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let snap1 = mgr
        .snapshot(&GameId::from("game-1"), src1.path())
        .await
        .unwrap();
    let snap2 = mgr
        .snapshot(&GameId::from("game-2"), src2.path())
        .await
        .unwrap();

    // Different content -> different hashes
    assert_ne!(snap1.hash, snap2.hash);

    // Both verify independently
    assert!(mgr.verify(&GameId::from("game-1")).await.unwrap());
    assert!(mgr.verify(&GameId::from("game-2")).await.unwrap());
}

#[tokio::test]
async fn test_resnapshot_after_cleaning_previous() {
    let src = TempDir::new().unwrap();
    std::fs::write(src.path().join("file.txt"), b"version1").unwrap();

    let store = TempDir::new().unwrap();
    let mgr = StockGameManager::new(store.path().to_path_buf());

    let snap1 = mgr
        .snapshot(&GameId::from("overwrite-test"), src.path())
        .await
        .unwrap();

    // Clean the existing snapshot directory, then re-snapshot with modified source
    std::fs::remove_dir_all(store.path().join("overwrite-test")).unwrap();
    std::fs::write(src.path().join("file.txt"), b"version2").unwrap();
    let snap2 = mgr
        .snapshot(&GameId::from("overwrite-test"), src.path())
        .await
        .unwrap();

    assert_ne!(snap1.hash, snap2.hash);
    // Verify should pass with the new hash
    assert!(mgr.verify(&GameId::from("overwrite-test")).await.unwrap());
}

// ── Default directory tests ─────────────────────────────────────────

#[test]
fn test_default_dir_returns_valid_path() {
    let dir = StockGameManager::default_dir();
    assert!(dir.to_string_lossy().contains("modde"));
    assert!(dir.to_string_lossy().contains("stock"));
}
