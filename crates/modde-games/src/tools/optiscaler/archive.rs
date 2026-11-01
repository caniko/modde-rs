#![allow(clippy::wildcard_imports)]
use super::*;

pub fn extract_optiscaler_archive_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let lower = archive_path.to_string_lossy().to_ascii_lowercase();
    if lower.ends_with(".zip") {
        return extract_zip_flat(archive_path, dest_dir);
    }
    if lower.ends_with(".7z") {
        return extract_7z_flat(archive_path, dest_dir);
    }
    anyhow::bail!(
        "unsupported OptiScaler archive type: {}",
        archive_path.display()
    )
}

pub(super) fn extract_zip_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut copied = 0usize;
    let mut copied_optiscaler = false;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            continue;
        };
        let Some(name) = enclosed.file_name().map(ToOwned::to_owned) else {
            continue;
        };
        let Some(name_str) = name.to_str() else {
            continue;
        };
        let lower = name_str.to_ascii_lowercase();
        if !is_optiscaler_payload_file(&lower) {
            continue;
        }
        let out = optiscaler_payload_dest(dest_dir, &enclosed, &name);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        let mut output = std::fs::File::create(&out)
            .with_context(|| format!("failed to create {}", out.display()))?;
        std::io::copy(&mut entry, &mut output)?;
        copied += 1;
        copied_optiscaler |= lower == "optiscaler.dll";
    }
    if copied == 0 || !copied_optiscaler {
        anyhow::bail!("archive did not contain OptiScaler.dll");
    }
    Ok(())
}

pub(super) fn extract_7z_flat(archive_path: &Path, dest_dir: &Path) -> Result<()> {
    let tmp_base = modde_core::paths::modde_data_dir().join("tmp");
    extract_7z_flat_in(&tmp_base, archive_path, dest_dir)
}

/// The scratch dir is a `TempDir`, so it is removed on success, error and panic.
pub(super) fn extract_7z_flat_in(
    tmp_base: &Path,
    archive_path: &Path,
    dest_dir: &Path,
) -> Result<()> {
    std::fs::create_dir_all(tmp_base)
        .with_context(|| format!("failed to create {}", tmp_base.display()))?;
    let extract_dir = tempfile::Builder::new()
        .prefix("modde-optiscaler-")
        .tempdir_in(tmp_base)
        .with_context(|| format!("failed to create temp dir in {}", tmp_base.display()))?;
    let out_arg = format!("-o{}", extract_dir.path().display());
    let archive_arg = archive_path.to_string_lossy().to_string();
    let mut extracted = false;
    for bin in ["7zz", "7z"] {
        let status = Command::new(bin)
            .args(["x", "-y", &out_arg, &archive_arg])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        if status.is_ok_and(|status| status.success()) {
            extracted = true;
            break;
        }
    }
    if !extracted {
        anyhow::bail!(
            "failed to extract {} (tried 7zz and 7z)",
            archive_path.display()
        );
    }
    copy_optiscaler_payload_flat(extract_dir.path(), dest_dir)
}

pub(super) fn copy_optiscaler_payload_flat(source_root: &Path, dest_dir: &Path) -> Result<()> {
    let mut stack = vec![source_root.to_path_buf()];
    let mut copied = 0usize;
    let mut copied_optiscaler = false;
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir)
            .with_context(|| format!("failed to read directory: {}", dir.display()))?
        {
            let entry = entry?;
            let path = entry.path();
            let metadata = path.symlink_metadata()?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            let lower = name.to_ascii_lowercase();
            if is_optiscaler_payload_file(&lower) {
                let relative = path.strip_prefix(source_root).unwrap_or(&path);
                let dest = optiscaler_payload_dest(dest_dir, relative, std::ffi::OsStr::new(name));
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)
                        .with_context(|| format!("failed to create {}", parent.display()))?;
                }
                std::fs::copy(&path, &dest)
                    .with_context(|| format!("failed to copy {}", dest.display()))?;
                copied += 1;
                copied_optiscaler |= lower == "optiscaler.dll";
            }
        }
    }
    if copied == 0 || !copied_optiscaler {
        anyhow::bail!("archive did not contain OptiScaler.dll");
    }
    Ok(())
}

pub(super) fn is_optiscaler_payload_file(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "optiscaler.dll"
            | "optiscaler.ini"
            | "fakenvapi.dll"
            | "nvngx-wrapper.dll"
            | "optipatcher.asi"
    ) || lower_name.ends_with(".dll")
}

pub(super) fn optiscaler_payload_dest(
    dest_dir: &Path,
    relative_path: &Path,
    file_name: &std::ffi::OsStr,
) -> PathBuf {
    if file_name
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(FSR4_DLL_NAME))
    {
        for component in relative_path.components() {
            let value = component.as_os_str().to_string_lossy();
            if value.eq_ignore_ascii_case(FSR4_LATEST_DIR) {
                return dest_dir.join(FSR4_LATEST_DIR).join(FSR4_DLL_NAME);
            }
            if value.eq_ignore_ascii_case(FSR4_INT8_DIR) {
                return dest_dir.join(FSR4_INT8_DIR).join(FSR4_DLL_NAME);
            }
        }
    }
    if file_name
        .to_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(OPTIPATCHER_ASSET))
    {
        return dest_dir.join("plugins").join(OPTIPATCHER_ASSET);
    }
    dest_dir.join(file_name)
}
