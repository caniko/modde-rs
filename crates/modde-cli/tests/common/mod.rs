//! Shared fixtures for CLI integration tests.
//!
//! [`Fixture`] gives each test an isolated tempdir-rooted environment:
//! a private modde data dir (via `MODDE_DATA_DIR`), HOME, and XDG dirs,
//! so tests never read or write the developer's real `~/.local/share/modde`.
//!
//! Use [`Fixture::cmd`] to spawn the `modde` binary pre-configured for
//! the fixture. Use [`Fixture::data_dir`] for assertions on on-disk state.

#![allow(dead_code)] // shared module; not every test uses every helper

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use tempfile::TempDir;

pub struct Fixture {
    tmp: TempDir,
    data_dir: PathBuf,
    home: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let tmp = TempDir::new().expect("create tempdir");
        let root = tmp.path().to_path_buf();
        let data_dir = root.join("data");
        let home = root.join("home");
        std::fs::create_dir_all(&data_dir).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Self {
            tmp,
            data_dir,
            home,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn root(&self) -> &Path {
        self.tmp.path()
    }

    /// Build a `modde` command pre-wired to use this fixture's tempdirs.
    /// Inherited env is cleared so the developer's real config can't leak in.
    pub fn cmd(&self) -> Command {
        let mut cmd = Command::cargo_bin("modde").expect("binary `modde` should be buildable");
        cmd.env_clear()
            .env("MODDE_DATA_DIR", &self.data_dir)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .env("XDG_CONFIG_HOME", self.home.join(".config"))
            .env("XDG_CACHE_HOME", self.home.join(".cache"))
            // Keep PATH so the binary can find linkers / libc on platforms
            // that need them at runtime; assert_cmd resolves the binary
            // path explicitly so PATH itself doesn't pick a stale modde.
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());
        // Trust-store env vars: rustls-native-certs respects these and they
        // are the only way the TLS init can find roots inside the Nix build
        // sandbox (no /etc/ssl/certs/). Pass them through when the parent
        // env sets them; harmless when unset. `LLVM_PROFILE_FILE` lets
        // cargo-llvm-cov collect coverage from the spawned binary.
        for var in [
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "NIX_SSL_CERT_FILE",
            "LLVM_PROFILE_FILE",
        ] {
            if let Some(val) = std::env::var_os(var) {
                cmd.env(var, val);
            }
        }
        cmd
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
