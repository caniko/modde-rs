use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

/// Check if an I/O error is a cross-device link error (EXDEV on Unix,
/// `ERROR_NOT_SAME_DEVICE` on Windows). Used to fall back from `rename`
/// to copy+delete when source and destination are on different filesystems.
#[must_use]
pub fn is_cross_device_error(e: &std::io::Error) -> bool {
    #[cfg(unix)]
    {
        e.raw_os_error() == Some(libc::EXDEV)
    }
    #[cfg(windows)]
    {
        e.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
    }
}

/// Create a symlink at `link` pointing to `original`, using the correct
/// platform API.  On Windows the call inspects `original` to decide between
/// `symlink_file` and `symlink_dir`.
pub fn symlink(original: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(original, link)
    }
    #[cfg(windows)]
    {
        if original.is_dir() {
            std::os::windows::fs::symlink_dir(original, link)
        } else {
            std::os::windows::fs::symlink_file(original, link)
        }
    }
}

/// Recursively visit every file under `dir`, calling `visitor(absolute_path)` for each.
///
/// This is the single recursive walker that all public helpers delegate to.
fn walk_dir(dir: &Path, visitor: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)
        .with_context(|| format!("failed to read directory: {}", dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            walk_dir(&path, visitor)?;
        } else {
            visitor(&path)?;
        }
    }
    Ok(())
}

/// Recursively walk a directory, collecting `(relative_path, absolute_path)` pairs for all files.
pub fn walk_files_relative(base: &Path) -> Result<Vec<(String, PathBuf)>> {
    let mut files = Vec::new();
    walk_dir(base, &mut |path| {
        let rel = path
            .strip_prefix(base)
            .with_context(|| "failed to compute relative path")?;
        files.push((rel.to_string_lossy().to_string(), path.to_path_buf()));
        Ok(())
    })?;
    Ok(files)
}

/// Recursively walk a directory, collecting all absolute file paths.
pub fn walk_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    walk_dir(dir, &mut |path| {
        files.push(path.to_path_buf());
        Ok(())
    })?;
    Ok(files)
}

/// Count files recursively in a directory.
pub fn count_files(dir: &Path) -> Result<u64> {
    let mut count = 0u64;
    walk_dir(dir, &mut |_| {
        count += 1;
        Ok(())
    })?;
    Ok(count)
}

/// Create a symlink asynchronously, using the correct platform API.
/// On Windows, inspects `original` to pick `symlink_file` vs `symlink_dir`.
pub async fn symlink_async(original: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        tokio::fs::symlink(original, link).await
    }
    #[cfg(windows)]
    {
        if original.is_dir() {
            tokio::fs::symlink_dir(original, link).await
        } else {
            tokio::fs::symlink_file(original, link).await
        }
    }
}

/// Resolve `relative` under `root`, reusing existing target-tree casing when
/// a path component matches ASCII-case-insensitively.
pub fn case_match_path(root: &Path, relative: &Path) -> std::io::Result<PathBuf> {
    let mut resolved = root.to_path_buf();

    for component in relative.components() {
        match component {
            Component::Normal(name) => {
                resolved = case_match_child(&resolved, name)?;
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }

    Ok(resolved)
}

fn case_match_child(parent: &Path, name: &OsStr) -> std::io::Result<PathBuf> {
    if parent.is_dir() {
        let requested = name.to_string_lossy();
        for entry in std::fs::read_dir(parent)? {
            let entry = entry?;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&requested)
            {
                return Ok(entry.path());
            }
        }
    }

    Ok(parent.join(name))
}

/// Deploy symlinks from `src` into `dst` recursively (creating directories as needed).
pub fn deploy_symlinks(src: &Path, dst: &Path) -> Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)
        .with_context(|| format!("failed to read staging dir: {}", src.display()))?
    {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = case_match_path(dst, Path::new(&entry.file_name()))
            .with_context(|| format!("failed to case-match path in {}", dst.display()))?;

        if src_path.is_dir() {
            std::fs::create_dir_all(&dst_path)?;
            deploy_symlinks(&src_path, &dst_path)?;
        } else {
            if dst_path.exists() || dst_path.symlink_metadata().is_ok() {
                std::fs::remove_file(&dst_path)?;
            }
            symlink(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn walk_files_relative_empty() {
        let tmp = TempDir::new().unwrap();
        let files = walk_files_relative(tmp.path()).unwrap();
        assert_eq!(files, [] as [(std::string::String, std::path::PathBuf); 0]);
    }

    #[test]
    fn walk_files_relative_flat() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.txt"), "a").unwrap();
        std::fs::write(tmp.path().join("b.esp"), "b").unwrap();
        let mut files = walk_files_relative(tmp.path()).unwrap();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "a.txt");
        assert_eq!(files[1].0, "b.esp");
    }

    #[test]
    fn walk_files_relative_nested() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("sub/deep")).unwrap();
        std::fs::write(tmp.path().join("sub/deep/file.txt"), "x").unwrap();
        std::fs::write(tmp.path().join("top.txt"), "y").unwrap();
        let files = walk_files_relative(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        let rels: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        assert!(rels.contains(&"top.txt"));
        assert!(rels.contains(&"sub/deep/file.txt"));
    }

    #[test]
    fn walk_files_relative_nonexistent() {
        let tmp = TempDir::new().unwrap();
        let files = walk_files_relative(&tmp.path().join("nope")).unwrap();
        assert_eq!(files, [] as [(std::string::String, std::path::PathBuf); 0]);
    }

    #[test]
    fn walk_files_flat_test() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a"), "a").unwrap();
        std::fs::write(tmp.path().join("b"), "b").unwrap();
        let files = walk_files(tmp.path()).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn count_files_test() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("sub")).unwrap();
        std::fs::write(tmp.path().join("a"), "a").unwrap();
        std::fs::write(tmp.path().join("sub/b"), "b").unwrap();
        assert_eq!(count_files(tmp.path()).unwrap(), 2);
    }

    #[test]
    fn count_files_nonexistent() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(count_files(&tmp.path().join("nope")).unwrap(), 0);
    }

    #[test]
    fn deploy_symlinks_test() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("sub")).unwrap();
        std::fs::write(src.join("a.txt"), "a").unwrap();
        std::fs::write(src.join("sub/b.txt"), "b").unwrap();

        deploy_symlinks(&src, &dst).unwrap();

        assert!(
            dst.join("a.txt")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(
            dst.join("sub/b.txt")
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(dst.join("sub/b.txt")).unwrap(), "b");
    }

    #[test]
    fn deploy_symlinks_reuses_existing_parent_directory_casing() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("Textures")).unwrap();
        std::fs::create_dir_all(dst.join("textures")).unwrap();
        std::fs::write(src.join("Textures/sky.dds"), "sky").unwrap();

        deploy_symlinks(&src, &dst).unwrap();

        assert!(dst.join("textures/sky.dds").exists());
        assert!(!dst.join("Textures").exists());
    }

    #[test]
    fn deploy_symlinks_reuses_existing_file_casing_and_replaces() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("Textures")).unwrap();
        std::fs::create_dir_all(dst.join("textures")).unwrap();
        std::fs::write(src.join("Textures/SKY.DDS"), "new sky").unwrap();
        std::fs::write(dst.join("textures/sky.dds"), "old sky").unwrap();

        deploy_symlinks(&src, &dst).unwrap();

        let deployed = dst.join("textures/sky.dds");
        assert!(
            deployed
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(deployed).unwrap(), "new sky");
        assert!(!dst.join("Textures/SKY.DDS").exists());
    }

    #[test]
    fn deploy_symlinks_uses_staged_casing_when_no_target_match_exists() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("Textures")).unwrap();
        std::fs::write(src.join("Textures/SKY.DDS"), "sky").unwrap();

        deploy_symlinks(&src, &dst).unwrap();

        assert!(dst.join("Textures/SKY.DDS").exists());
    }

    #[test]
    fn deploy_symlinks_matches_nested_components_independently() {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        let dst = tmp.path().join("dst");
        std::fs::create_dir_all(src.join("data/textures/landscape")).unwrap();
        std::fs::create_dir_all(dst.join("Data/Textures/LANDSCAPE")).unwrap();
        std::fs::write(src.join("data/textures/landscape/DIRT.DDS"), "dirt").unwrap();
        std::fs::write(dst.join("Data/Textures/LANDSCAPE/dirt.dds"), "old").unwrap();

        deploy_symlinks(&src, &dst).unwrap();

        let deployed = dst.join("Data/Textures/LANDSCAPE/dirt.dds");
        assert!(
            deployed
                .symlink_metadata()
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(std::fs::read_to_string(deployed).unwrap(), "dirt");
        assert!(!dst.join("data").exists());
    }
}
