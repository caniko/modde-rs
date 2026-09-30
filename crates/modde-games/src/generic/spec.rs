//! TOML specification for user-defined generic games and its validation.

use std::path::{Component, Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::registry::RESERVED_GAME_IDS;

/// Deserialized configuration for a user-defined generic game.
#[derive(Debug, Clone, Deserialize)]
pub struct GameSpec {
    pub id: String,
    pub display_name: String,
    pub steam_app_id: Option<String>,
    pub install_dir_name: Option<String>,
    pub install_path_override: Option<PathBuf>,
    pub executable_dir: PathBuf,
    pub mod_dir: Option<PathBuf>,
    pub nexus_domain: Option<String>,
    #[serde(default)]
    pub proxy_dlls: Vec<String>,
}

/// Borrowing serialization view of a [`GameSpec`] for writing TOML.
#[derive(Debug, Clone, Serialize)]
pub struct GameSpecToml<'a> {
    pub id: &'a str,
    pub display_name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steam_app_id: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_dir_name: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_path_override: Option<&'a std::path::Path>,
    pub executable_dir: &'a std::path::Path,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mod_dir: Option<&'a std::path::Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nexus_domain: Option<&'a str>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub proxy_dlls: Vec<&'a str>,
}

/// Serialize a [`GameSpec`] to a pretty TOML string.
pub fn serialize(game: &GameSpec) -> Result<String> {
    let toml = GameSpecToml {
        id: &game.id,
        display_name: &game.display_name,
        steam_app_id: game.steam_app_id.as_deref(),
        install_dir_name: game.install_dir_name.as_deref(),
        install_path_override: game.install_path_override.as_deref(),
        executable_dir: &game.executable_dir,
        mod_dir: game.mod_dir.as_deref(),
        nexus_domain: game.nexus_domain.as_deref(),
        proxy_dlls: game.proxy_dlls.iter().map(String::as_str).collect(),
    };

    Ok(toml::to_string_pretty(&toml)?)
}

impl GameSpec {
    /// Validate the spec: well-formed `id`, no built-in collision, relative
    /// install-root paths, and a non-empty display name.
    pub fn validate(&self) -> Result<()> {
        if !is_valid_game_id(&self.id) {
            bail!(
                "invalid game id '{}': must match ^[a-z0-9][a-z0-9-]*$",
                self.id
            );
        }

        if RESERVED_GAME_IDS.contains(&self.id.as_str()) {
            bail!("game id '{}' collides with a built-in game", self.id);
        }

        ensure_relative_path(&self.executable_dir, "executable_dir")?;
        if let Some(mod_dir) = &self.mod_dir {
            ensure_relative_path(mod_dir, "mod_dir")?;
        }

        if self.display_name.trim().is_empty() {
            bail!("display_name must not be empty");
        }

        Ok(())
    }
}

fn is_valid_game_id(id: &str) -> bool {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first.is_ascii_digit() => {}
        _ => return false,
    }

    chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
}

fn ensure_relative_path(path: &Path, field: &str) -> Result<()> {
    if path.is_absolute() {
        bail!("{field} must be relative to the install root");
    }

    if path.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("{field} must not escape the install root");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_minimal_spec_parses() {
        let spec: GameSpec = toml::from_str(
            r#"
                id = "custom-game"
                display_name = "Custom Game"
                executable_dir = "bin/x64"
            "#,
        )
        .expect("spec should parse");

        spec.validate().expect("spec should validate");
        assert_eq!(spec.proxy_dlls, Vec::<String>::new());
    }

    #[test]
    fn reject_absolute_executable_dir() {
        let spec: GameSpec = toml::from_str(
            r#"
                id = "custom-game"
                display_name = "Custom Game"
                executable_dir = "/opt/game/bin"
            "#,
        )
        .expect("spec should parse");

        let err = spec
            .validate()
            .expect_err("absolute path should be rejected");
        assert!(err.to_string().contains("executable_dir"));
    }

    #[test]
    fn reject_built_in_id_collision() {
        let spec: GameSpec = toml::from_str(
            r#"
                id = "skyrim-se"
                display_name = "Not Skyrim"
                executable_dir = "bin/x64"
            "#,
        )
        .expect("spec should parse");

        let err = spec
            .validate()
            .expect_err("built-in collision should be rejected");
        assert!(err.to_string().contains("collides with a built-in game"));
    }
}
