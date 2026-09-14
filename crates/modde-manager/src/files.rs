//! Linux descriptor-anchored, no-follow filesystem operations.
use super::*;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Image {
    Missing,
    File(Vec<u8>),
    Directory(BTreeMap<String, Image>),
}

pub struct Anchor {
    pub file: File,
    original: PathBuf,
}

pub struct Lease(File);

impl Drop for Lease {
    fn drop(&mut self) {
        // Release explicitly: a child between fork and exec may still hold a duplicate.
        let _ = self.0.unlock();
    }
}

pub fn relative(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.components().count() > 128
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || path.to_str().is_none_or(|s| s.contains(['\\', ':']))
    {
        bail!("unsafe relative path: {}", path.display());
    }
    Ok(())
}

fn directory(path: &Path) -> Result<File> {
    Ok(OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?)
}

impl Anchor {
    pub fn lock(&self) -> Result<Lease> {
        self.revalidate()?;
        // A separate open description also prevents reentrant locking of one Anchor.
        let file = directory(&self.path().join("."))?;
        file.try_lock()
            .context("another manager mutation holds this instance")?;
        Ok(Lease(file))
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !cfg!(target_os = "linux") {
            bail!("safe manager filesystem operations currently require Linux");
        }
        if !path.is_absolute() {
            bail!("runtime root must be absolute: {}", path.display());
        }
        let mut file = directory(Path::new("/"))?;
        for part in path.components().skip(1) {
            if !matches!(part, std::path::Component::Normal(_)) {
                bail!("unsafe runtime root");
            }
            file = directory(&fd_path(&file).join(part.as_os_str()))?;
        }
        Ok(Self {
            file,
            original: path.to_owned(),
        })
    }

    pub fn path(&self) -> PathBuf {
        fd_path(&self.file)
    }

    pub fn revalidate(&self) -> Result<()> {
        let current = Self::open(&self.original)?;
        let old = self.file.metadata()?;
        let new = current.file.metadata()?;
        if old.dev() != new.dev() || old.ino() != new.ino() {
            bail!("runtime root changed during operation");
        }
        Ok(())
    }

    pub fn parent(
        &self,
        path: &Path,
        create: bool,
        created: &mut Vec<PathBuf>,
    ) -> Result<Option<File>> {
        relative(path)?;
        self.revalidate()?;
        let mut file = self.file.try_clone()?;
        let mut traversed = PathBuf::new();
        for part in path.parent().context("missing parent")?.components() {
            traversed.push(part.as_os_str());
            let candidate = fd_path(&file).join(part.as_os_str());
            match directory(&candidate) {
                Ok(next) => file = next,
                Err(error)
                    if error
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
                {
                    if !create {
                        return Ok(None);
                    }
                    match fs::create_dir(&candidate) {
                        Ok(()) => created.push(traversed.clone()),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(e) => return Err(e.into()),
                    }
                    file = directory(&candidate)?;
                }
                Err(error) => return Err(error),
            }
        }
        Ok(Some(file))
    }

    pub fn read(&self, path: &Path, skip_git: bool) -> Result<Image> {
        let Some(parent) = self.parent(path, false, &mut Vec::new())? else {
            return Ok(Image::Missing);
        };
        read_image(
            &fd_path(&parent).join(path.file_name().context("missing name")?),
            skip_git,
        )
    }

    pub fn target(
        &self,
        path: &Path,
        create: bool,
        created: &mut Vec<PathBuf>,
    ) -> Result<(File, PathBuf)> {
        let parent = self
            .parent(path, create, created)?
            .context("destination parent disappeared")?;
        let target = fd_path(&parent).join(path.file_name().context("missing name")?);
        Ok((parent, target))
    }

    pub fn revalidate_parent(&self, path: &Path, pinned: &File) -> Result<()> {
        let current = self
            .parent(path, false, &mut Vec::new())?
            .context("parent disappeared before commit")?;
        let before = pinned.metadata()?;
        let after = current.metadata()?;
        if before.dev() != after.dev() || before.ino() != after.ino() {
            bail!("destination parent changed before commit");
        }
        Ok(())
    }
}

pub fn fd_path(file: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

pub fn rename_noreplace(from: &Path, to: &Path) -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (from, to);
        bail!("safe no-clobber rename requires Linux");
    }
    #[cfg(target_os = "linux")]
    {
        let from = std::ffi::CString::new(from.as_os_str().as_encoded_bytes())?;
        let to = std::ffi::CString::new(to.as_os_str().as_encoded_bytes())?;
        // SAFETY: both C strings remain alive for the call. No ownership is transferred.
        // Kernel RENAME_NOREPLACE rejects a destination created after our last read.
        let result = unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
}

pub fn read_image(path: &Path, skip_git: bool) -> Result<Image> {
    read_image_at(path, skip_git, 0)
}

fn read_image_at(path: &Path, skip_git: bool, depth: usize) -> Result<Image> {
    if depth > 128 {
        bail!("source tree exceeds supported nesting depth");
    }
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Image::Missing),
        Err(e) => return Err(e.into()),
    };
    if metadata.is_dir() {
        let dir = directory(path)?;
        let mut entries = BTreeMap::new();
        for entry in fs::read_dir(fd_path(&dir))? {
            let name = entry?
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("non-UTF8 filename"))?;
            relative(Path::new(&name))?;
            if skip_git && name == ".git" {
                continue;
            }
            let image = read_image_at(&fd_path(&dir).join(&name), skip_git, depth + 1)?;
            if image == Image::Missing {
                bail!("source changed during read");
            }
            entries.insert(name, image);
        }
        Ok(Image::Directory(entries))
    } else if metadata.is_file() {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            bail!("source changed type");
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok(Image::File(bytes))
    } else {
        bail!(
            "symlink or unsupported filesystem object: {}",
            path.display()
        );
    }
}

pub fn write_image(path: &Path, image: &Image) -> Result<()> {
    match image {
        Image::File(bytes) => {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)?;
            file.write_all(bytes)?;
            file.sync_all()?;
        }
        Image::Directory(entries) => {
            fs::create_dir(path)?;
            for (name, child) in entries {
                write_image(&path.join(name), child)?;
            }
        }
        Image::Missing => bail!("cannot stage missing payload"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lease_release_is_not_delayed_by_inherited_descriptors() {
        let dir = tempfile::tempdir().unwrap();
        let root = Anchor::open(dir.path()).unwrap();
        let lease = root.lock().unwrap();
        assert!(root.lock().is_err());
        let inherited = lease.0.try_clone().unwrap();
        drop(lease);
        let next = root.lock().unwrap();
        drop(inherited);
        drop(next);
    }
}
