//! LOOT masterlist parser for automatic plugin load order sorting.
//!
//! Parses LOOT's publicly available masterlist YAML files to extract
//! plugin ordering rules (`after`, `requires`, `incompatible`), then
//! maps them to modde's `LoadOrderRule` variants.
//!
//! Masterlist format reference:
//! <https://loot.readthedocs.io/en/latest/app/usage/masterlist_editing.html>

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;
use tracing::{debug, info, warn};

use modde_core::resolver::{LoadOrderRule, ModId};

/// A parsed LOOT masterlist containing plugin rules.
#[derive(Debug, Clone, Default)]
pub struct LootMasterlist {
    /// Plugin name (lowercase) -> rules for that plugin.
    pub plugins: HashMap<String, LootPluginEntry>,
}

/// Rules for a single plugin from the masterlist.
#[derive(Debug, Clone, Default)]
pub struct LootPluginEntry {
    /// This plugin must load after these plugins.
    pub after: Vec<String>,
    /// This plugin requires these plugins (treated as load-after + missing master warning).
    pub requires: Vec<String>,
    /// This plugin is incompatible with these plugins.
    pub incompatible: Vec<String>,
}

// ── YAML deserialization types ──────────────────────────────────

#[derive(Debug, Deserialize)]
struct RawMasterlist {
    #[serde(default)]
    plugins: Vec<RawPlugin>,
}

#[derive(Debug, Deserialize)]
struct RawPlugin {
    name: String,
    #[serde(default)]
    after: Vec<RawFileRef>,
    #[serde(default)]
    requires: Vec<RawFileRef>,
    #[serde(default)]
    incompatible: Vec<RawFileRef>,
}

/// A file reference in LOOT masterlist can be either a plain string
/// or an object with a `name` field (and optional conditions).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum RawFileRef {
    Simple(String),
    Complex { name: String },
}

impl RawFileRef {
    fn name(&self) -> &str {
        match self {
            RawFileRef::Simple(s) => s,
            RawFileRef::Complex { name } => name,
        }
    }
}

impl LootMasterlist {
    /// Parse a LOOT masterlist from a YAML string.
    pub fn from_yaml(yaml: &str) -> Result<Self> {
        let raw: RawMasterlist =
            serde_yaml_ng::from_str(yaml).context("failed to parse LOOT masterlist YAML")?;

        let mut plugins = HashMap::new();

        for plugin in raw.plugins {
            let key = plugin.name.to_lowercase();

            let entry = LootPluginEntry {
                after: plugin
                    .after
                    .iter()
                    .map(|r| r.name().to_lowercase())
                    .collect(),
                requires: plugin
                    .requires
                    .iter()
                    .map(|r| r.name().to_lowercase())
                    .collect(),
                incompatible: plugin
                    .incompatible
                    .iter()
                    .map(|r| r.name().to_lowercase())
                    .collect(),
            };

            if !entry.after.is_empty()
                || !entry.requires.is_empty()
                || !entry.incompatible.is_empty()
            {
                plugins.insert(key, entry);
            }
        }

        info!(plugin_count = plugins.len(), "parsed LOOT masterlist");
        Ok(Self { plugins })
    }

    /// Parse a LOOT masterlist from a file.
    pub fn from_file(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read LOOT masterlist: {}", path.display()))?;
        Self::from_yaml(&content)
    }

    /// Convert masterlist rules for the given active plugins into `LoadOrderRule`s.
    ///
    /// Only generates rules for plugins that are actually present in the active set.
    /// Plugin names are matched case-insensitively.
    pub fn rules_for_plugins(&self, active_plugins: &[&str]) -> Vec<LoadOrderRule> {
        let active_lower: HashMap<String, &str> = active_plugins
            .iter()
            .map(|p| (p.to_lowercase(), *p))
            .collect();

        let mut rules = Vec::new();

        for plugin_name in active_plugins {
            let key = plugin_name.to_lowercase();

            if let Some(entry) = self.plugins.get(&key) {
                // `after` rules: this plugin must load after the listed plugins
                for after_name in &entry.after {
                    if let Some(&original) = active_lower.get(after_name) {
                        rules.push(LoadOrderRule::LoadAfter {
                            mod_id: ModId::from(*plugin_name),
                            after: ModId::from(original),
                        });
                    }
                }

                // `requires` rules: also treated as load-after (dependency must load first)
                for req_name in &entry.requires {
                    if let Some(&original) = active_lower.get(req_name) {
                        rules.push(LoadOrderRule::LoadAfter {
                            mod_id: ModId::from(*plugin_name),
                            after: ModId::from(original),
                        });
                    } else {
                        debug!(
                            plugin = *plugin_name,
                            requires = req_name.as_str(),
                            "required plugin not in active set"
                        );
                    }
                }

                // `incompatible` rules
                for incompat_name in &entry.incompatible {
                    if let Some(&original) = active_lower.get(incompat_name) {
                        rules.push(LoadOrderRule::Incompatible {
                            mod_a: ModId::from(*plugin_name),
                            mod_b: ModId::from(original),
                        });
                    }
                }
            }
        }

        debug!(
            rule_count = rules.len(),
            "generated load order rules from LOOT masterlist"
        );
        rules
    }
}

/// Well-known LOOT masterlist repository URLs.
pub fn masterlist_url(game_id: &str) -> Option<&'static str> {
    match game_id {
        "skyrim-se" | "skyrim-ae" => {
            Some("https://raw.githubusercontent.com/loot/skyrimse/master/masterlist.yaml")
        }
        "fallout4" => {
            Some("https://raw.githubusercontent.com/loot/fallout4/master/masterlist.yaml")
        }
        "fallout76" => {
            Some("https://raw.githubusercontent.com/loot/fallout76/master/masterlist.yaml")
        }
        "starfield" => {
            Some("https://raw.githubusercontent.com/loot/starfield/master/masterlist.yaml")
        }
        _ => {
            warn!(game_id, "no LOOT masterlist URL known for game");
            None
        }
    }
}

/// Cached masterlist path within the modde data directory.
#[must_use]
pub fn masterlist_cache_path(game_id: &str) -> std::path::PathBuf {
    modde_core::paths::data_dir()
        .join("loot")
        .join(format!("{game_id}.yaml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_YAML: &str = r#"
plugins:
  - name: "Unofficial Skyrim Special Edition Patch.esp"
    after:
      - "Skyrim.esm"
      - "Update.esm"
      - "Dawnguard.esm"
    requires:
      - "Skyrim.esm"
  - name: "SkyUI_SE.esp"
    after:
      - "Unofficial Skyrim Special Edition Patch.esp"
  - name: "BadMod.esp"
    incompatible:
      - name: "ConflictMod.esp"
"#;

    #[test]
    fn test_parse_masterlist() {
        let ml = LootMasterlist::from_yaml(TEST_YAML).unwrap();
        assert_eq!(ml.plugins.len(), 3);

        let ussep = &ml.plugins["unofficial skyrim special edition patch.esp"];
        assert_eq!(ussep.after.len(), 3);
        assert_eq!(ussep.requires.len(), 1);
        assert_eq!(ussep.incompatible, [] as [std::string::String; 0]);

        let skyui = &ml.plugins["skyui_se.esp"];
        assert_eq!(skyui.after.len(), 1);
        assert_eq!(
            skyui.after[0],
            "unofficial skyrim special edition patch.esp"
        );
    }

    #[test]
    fn test_rules_for_plugins() {
        let ml = LootMasterlist::from_yaml(TEST_YAML).unwrap();

        let active = vec![
            "Skyrim.esm",
            "Update.esm",
            "Dawnguard.esm",
            "Unofficial Skyrim Special Edition Patch.esp",
            "SkyUI_SE.esp",
        ];

        let rules = ml.rules_for_plugins(&active);

        // USSEP should have LoadAfter for Skyrim.esm, Update.esm, Dawnguard.esm (from after)
        // + LoadAfter for Skyrim.esm (from requires) = 4 LoadAfter rules
        // SkyUI should have 1 LoadAfter for USSEP
        let load_after_count = rules
            .iter()
            .filter(|r| matches!(r, LoadOrderRule::LoadAfter { .. }))
            .count();
        assert_eq!(load_after_count, 5);
    }

    #[test]
    fn test_incompatible_rules() {
        let ml = LootMasterlist::from_yaml(TEST_YAML).unwrap();

        let active = vec!["BadMod.esp", "ConflictMod.esp"];
        let rules = ml.rules_for_plugins(&active);

        let incompat_count = rules
            .iter()
            .filter(|r| matches!(r, LoadOrderRule::Incompatible { .. }))
            .count();
        assert_eq!(incompat_count, 1);
    }

    #[test]
    fn test_inactive_plugins_excluded() {
        let ml = LootMasterlist::from_yaml(TEST_YAML).unwrap();

        // Only SkyUI is active but its dependency (USSEP) is not
        let active = vec!["SkyUI_SE.esp"];
        let rules = ml.rules_for_plugins(&active);

        // No rules because USSEP isn't in the active set
        assert!(rules.is_empty());
    }

    #[test]
    fn test_masterlist_urls() {
        assert!(masterlist_url("skyrim-se").is_some());
        assert!(masterlist_url("skyrim-ae").is_some());
        assert!(masterlist_url("fallout4").is_some());
        assert!(masterlist_url("cyberpunk2077").is_none());
    }
}
