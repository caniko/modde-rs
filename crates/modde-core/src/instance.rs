use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A modde instance — a self-contained data directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub name: String,
    pub data_dir: PathBuf,
    #[serde(default)]
    pub is_default: bool,
}

/// Instance registry stored at ~/.config/modde/instances.toml
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceRegistry {
    #[serde(default)]
    pub instances: Vec<Instance>,
    #[serde(default)]
    pub active: Option<String>,
}

impl InstanceRegistry {
    /// Load the registry from the default config location.
    #[must_use]
    pub fn load() -> Self {
        Self::load_from(&registry_path())
    }

    /// Save the registry.
    pub fn save(&self) -> crate::error::Result<()> {
        self.save_to(&registry_path())
    }

    /// Create a new instance.
    pub fn create(&mut self, name: &str, data_dir: PathBuf) -> crate::error::Result<()> {
        self.create_with_path(name, data_dir, &registry_path())
    }

    /// Switch to a different instance.
    pub fn switch(&mut self, name: &str) -> crate::error::Result<()> {
        self.switch_with_path(name, &registry_path())
    }

    /// Get the active instance's data directory.
    #[must_use]
    pub fn active_data_dir(&self) -> Option<&Path> {
        let name = self.active.as_ref()?;
        self.instances
            .iter()
            .find(|i| &i.name == name)
            .map(|i| i.data_dir.as_path())
    }

    /// List all instances.
    #[must_use]
    pub fn list(&self) -> &[Instance] {
        &self.instances
    }

    /// Load the registry from a specific file path.
    #[must_use]
    pub fn load_from(path: &Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Save the registry to a specific file path.
    pub fn save_to(&self, path: &Path) -> crate::error::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Create a new instance, saving to a specific registry path.
    pub fn create_with_path(
        &mut self,
        name: &str,
        data_dir: PathBuf,
        registry_path: &Path,
    ) -> crate::error::Result<()> {
        if self.instances.iter().any(|i| i.name == name) {
            return Err(crate::error::CoreError::Validation(
                format!("instance '{name}' already exists").into(),
            ));
        }
        std::fs::create_dir_all(&data_dir)?;
        self.instances.push(Instance {
            name: name.to_string(),
            data_dir,
            is_default: self.instances.is_empty(),
        });
        if self.active.is_none() {
            self.active = Some(name.to_string());
        }
        self.save_to(registry_path)?;
        Ok(())
    }

    /// Switch to a different instance, saving to a specific registry path.
    pub fn switch_with_path(
        &mut self,
        name: &str,
        registry_path: &Path,
    ) -> crate::error::Result<()> {
        if !self.instances.iter().any(|i| i.name == name) {
            return Err(crate::error::CoreError::Validation(
                format!("instance '{name}' not found").into(),
            ));
        }
        self.active = Some(name.to_string());
        self.save_to(registry_path)?;
        Ok(())
    }
}

fn registry_path() -> PathBuf {
    crate::paths::modde_config_dir().join("instances.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_path = tmp.path().join("instances.toml");
        let data_dir = tmp.path().join("instance-data");

        let mut reg = InstanceRegistry::default();
        reg.create_with_path("default", data_dir.clone(), &registry_path)
            .unwrap();

        assert_eq!(reg.list().len(), 1);
        assert_eq!(reg.list()[0].name, "default");
        assert_eq!(reg.list()[0].data_dir, data_dir);
        assert!(reg.list()[0].is_default);
    }

    #[test]
    fn test_switch() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_path = tmp.path().join("instances.toml");

        let mut reg = InstanceRegistry::default();
        reg.create_with_path("first", tmp.path().join("first"), &registry_path)
            .unwrap();
        reg.create_with_path("second", tmp.path().join("second"), &registry_path)
            .unwrap();

        assert_eq!(reg.active.as_deref(), Some("first"));

        reg.switch_with_path("second", &registry_path).unwrap();
        assert_eq!(reg.active.as_deref(), Some("second"));
    }

    #[test]
    fn test_duplicate_name_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_path = tmp.path().join("instances.toml");

        let mut reg = InstanceRegistry::default();
        reg.create_with_path("myinstance", tmp.path().join("data1"), &registry_path)
            .unwrap();

        let result = reg.create_with_path("myinstance", tmp.path().join("data2"), &registry_path);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("already exists"),
            "expected 'already exists' error, got: {err}"
        );
    }

    #[test]
    fn test_active_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let registry_path = tmp.path().join("instances.toml");
        let first_dir = tmp.path().join("first");
        let second_dir = tmp.path().join("second");

        let mut reg = InstanceRegistry::default();
        reg.create_with_path("first", first_dir.clone(), &registry_path)
            .unwrap();
        reg.create_with_path("second", second_dir.clone(), &registry_path)
            .unwrap();

        // First instance is active by default
        assert_eq!(reg.active_data_dir(), Some(first_dir.as_path()));

        // Switch and verify
        reg.switch_with_path("second", &registry_path).unwrap();
        assert_eq!(reg.active_data_dir(), Some(second_dir.as_path()));
    }
}
