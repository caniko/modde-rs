//! Wabbajack modlist support: parsing `.wabbajack` files, acquiring their
//! archives, and running the installer that materializes a modlist on disk.

pub mod acquire;
#[cfg(feature = "bethesda-archives")]
pub mod bsa_repack;
pub mod catalog;
pub mod cdn;
pub mod diagnostics;
pub mod impact;
pub mod import;
pub mod inline;
pub mod installer;
pub mod manifest;
pub mod patcher;
pub(crate) mod preflight;
pub mod readiness;
pub mod runner;
pub mod staging;
pub mod validator;
