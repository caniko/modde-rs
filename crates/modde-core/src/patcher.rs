use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::{CoreError, Result};

pub const DEFAULT_PATCHER_TIMEOUT_SECONDS: u64 = 1_800;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PatcherStageKind {
    SynthesisCli,
    Command,
    RustNative,
}

impl PatcherStageKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SynthesisCli => "synthesis-cli",
            Self::Command => "command",
            Self::RustNative => "rust-native",
        }
    }

    pub fn parse(raw: &str) -> Result<Self> {
        match raw {
            "synthesis-cli" => Ok(Self::SynthesisCli),
            "command" => Ok(Self::Command),
            "rust-native" => Ok(Self::RustNative),
            _ => Err(CoreError::Validation(
                format!("unknown patcher stage kind '{raw}'").into(),
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesisCliSettings {
    pub executable: PathBuf,
    pub pipeline_settings: PathBuf,
    pub synthesis_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSettings {
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub environment: HashMap<String, String>,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum PatcherStageSettings {
    SynthesisCli(SynthesisCliSettings),
    Command(CommandSettings),
    RustNative,
}

impl PatcherStageSettings {
    #[must_use]
    pub fn kind(&self) -> PatcherStageKind {
        match self {
            Self::SynthesisCli(_) => PatcherStageKind::SynthesisCli,
            Self::Command(_) => PatcherStageKind::Command,
            Self::RustNative => PatcherStageKind::RustNative,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatcherStageRow {
    pub profile_id: i64,
    pub name: String,
    pub stage_kind: PatcherStageKind,
    pub enabled: bool,
    pub sort_index: i64,
    pub settings: PatcherStageSettings,
    pub output_mod: String,
    pub last_cache_key: Option<String>,
    pub last_success_at: Option<String>,
    pub timeout_seconds: u64,
}

impl PatcherStageRow {
    pub fn new(
        profile_id: i64,
        name: impl Into<String>,
        enabled: bool,
        sort_index: i64,
        settings: PatcherStageSettings,
        output_mod: impl Into<String>,
    ) -> Result<Self> {
        let name = name.into();
        if name.trim().is_empty() {
            return Err(CoreError::Validation(
                "patcher stage name cannot be empty".into(),
            ));
        }
        let output_mod = output_mod.into();
        if output_mod.trim().is_empty() {
            return Err(CoreError::Validation(
                "patcher output mod cannot be empty".into(),
            ));
        }
        Ok(Self {
            profile_id,
            name,
            stage_kind: settings.kind(),
            enabled,
            sort_index,
            settings,
            output_mod,
            last_cache_key: None,
            last_success_at: None,
            timeout_seconds: DEFAULT_PATCHER_TIMEOUT_SECONDS,
        })
    }

    #[must_use]
    pub fn with_timeout_seconds(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = timeout_seconds.max(1);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatcherStageOutputRow {
    pub profile_id: i64,
    pub stage_name: String,
    pub rel_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_settings_roundtrip_preserves_kind() {
        let settings = PatcherStageSettings::SynthesisCli(SynthesisCliSettings {
            executable: PathBuf::from("/tools/Synthesis.CLI.exe"),
            pipeline_settings: PathBuf::from("/cfg/PipelineSettings.json"),
            synthesis_profile: "main".to_string(),
        });

        let encoded = serde_json::to_string(&settings).unwrap();
        let decoded: PatcherStageSettings = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded.kind(), PatcherStageKind::SynthesisCli);
        assert_eq!(decoded, settings);
    }

    #[test]
    fn stage_row_rejects_empty_output_mod() {
        let err = PatcherStageRow::new(
            1,
            "synthesis",
            true,
            0,
            PatcherStageSettings::RustNative,
            "",
        )
        .unwrap_err();

        assert!(
            err.to_string()
                .contains("patcher output mod cannot be empty")
        );
    }
}
