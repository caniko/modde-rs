use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
mod parser;
use parser::{
    crash_logger_section, net_script_framework_section, normalize_path, parse_with_sections,
    sha256_hex, trainwreck_section,
};

use crate::PluginEntry;
use crate::installer::StagedFile;
use crate::profile::{EnabledMod, Profile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashLogFormat {
    Auto,
    CrashLoggerSse,
    NetScriptFramework,
    Trainwreck,
    Generic,
}

impl CrashLogFormat {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::CrashLoggerSse => "crash-logger-sse",
            Self::NetScriptFramework => "net-script-framework",
            Self::Trainwreck => "trainwreck",
            Self::Generic => "generic",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "crash-logger-sse" | "crashlogger" | "crashloggersse" => Some(Self::CrashLoggerSse),
            "net-script-framework" | "netscriptframework" | ".net-script-framework" => {
                Some(Self::NetScriptFramework)
            }
            "trainwreck" => Some(Self::Trainwreck),
            "generic" => Some(Self::Generic),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashToken {
    pub value: String,
    pub kind: CrashTokenKind,
    pub section: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashTokenKind {
    Plugin,
    Dll,
    AssetPath,
    FormId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashSignature {
    pub detected_format: CrashLogFormat,
    pub exception_line: Option<String>,
    pub tokens: Vec<CrashToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashCorrelationReport {
    pub game_id: String,
    pub profile_name: String,
    pub source_path: PathBuf,
    pub raw_sha256: String,
    pub format: CrashLogFormat,
    pub signature: CrashSignature,
    pub suspects: Vec<CrashSuspect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashSuspect {
    pub mod_id: Option<String>,
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub nexus_mod_id: Option<i64>,
    pub nexus_file_id: Option<i64>,
    pub nexus_game_domain: Option<String>,
    pub installed_timestamp: Option<i64>,
    pub plugin_name: Option<String>,
    pub plugin_load_index: Option<i64>,
    pub evidence: Vec<CrashEvidence>,
    pub confidence: CrashConfidence,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashEvidence {
    pub token: String,
    pub kind: CrashTokenKind,
    pub section: String,
    pub reason: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CrashConfidence {
    Low,
    Medium,
    High,
    Highest,
}

pub trait CrashLogParser {
    fn format(&self) -> CrashLogFormat;
    fn parse(&self, text: &str) -> CrashSignature;
}

pub struct CrashLoggerSseParser;
pub struct NetScriptFrameworkParser;
pub struct TrainwreckParser;
pub struct GenericCrashParser;

impl CrashLogParser for CrashLoggerSseParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::CrashLoggerSse
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), crash_logger_section)
    }
}

impl CrashLogParser for TrainwreckParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::Trainwreck
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), trainwreck_section)
    }
}

impl CrashLogParser for NetScriptFrameworkParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::NetScriptFramework
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), net_script_framework_section)
    }
}

impl CrashLogParser for GenericCrashParser {
    fn format(&self) -> CrashLogFormat {
        CrashLogFormat::Generic
    }

    fn parse(&self, text: &str) -> CrashSignature {
        parse_with_sections(text, self.format(), |_| None)
    }
}

#[derive(Debug, Clone)]
pub struct CrashCorrelationInput {
    pub game_id: String,
    pub profile: Profile,
    pub active_plugins: Vec<PluginEntry>,
    pub installed_files: Vec<(String, StagedFile)>,
    pub tool_files: Vec<String>,
}

#[must_use]
pub fn detect_format(text: &str) -> CrashLogFormat {
    let mut lines = text.lines();
    let head = (0..8)
        .filter_map(|_| lines.next())
        .collect::<Vec<_>>()
        .join("\n")
        .to_lowercase();
    if head.contains(".net script framework") || head.contains("net script framework") {
        CrashLogFormat::NetScriptFramework
    } else if head.contains("crashloggersse") {
        CrashLogFormat::CrashLoggerSse
    } else if head.contains("trainwreck") {
        CrashLogFormat::Trainwreck
    } else {
        CrashLogFormat::Generic
    }
}

#[must_use]
pub fn parse_crash_log(text: &str, requested: CrashLogFormat) -> CrashSignature {
    let format = match requested {
        CrashLogFormat::Auto => detect_format(text),
        other => other,
    };
    match format {
        CrashLogFormat::Auto => unreachable!("auto is resolved before parser dispatch"),
        CrashLogFormat::CrashLoggerSse => CrashLoggerSseParser.parse(text),
        CrashLogFormat::NetScriptFramework => NetScriptFrameworkParser.parse(text),
        CrashLogFormat::Trainwreck => TrainwreckParser.parse(text),
        CrashLogFormat::Generic => GenericCrashParser.parse(text),
    }
}

#[must_use]
pub fn correlate_crash_log(
    source_path: &Path,
    raw_text: &str,
    requested_format: CrashLogFormat,
    input: CrashCorrelationInput,
) -> CrashCorrelationReport {
    let signature = parse_crash_log(raw_text, requested_format);
    let raw_sha256 = sha256_hex(raw_text.as_bytes());
    let suspects = correlate_tokens(&signature.tokens, &input);
    CrashCorrelationReport {
        game_id: input.game_id,
        profile_name: input.profile.name,
        source_path: source_path.to_path_buf(),
        raw_sha256,
        format: signature.detected_format,
        signature,
        suspects,
    }
}

fn correlate_tokens(tokens: &[CrashToken], input: &CrashCorrelationInput) -> Vec<CrashSuspect> {
    let mut by_key: HashMap<String, CrashSuspect> = HashMap::new();
    let mods_by_id = input
        .profile
        .mods
        .iter()
        .map(|m| (m.mod_id.to_lowercase(), m))
        .collect::<HashMap<_, _>>();
    let active_plugins = input
        .active_plugins
        .iter()
        .filter(|p| p.enabled)
        .map(|p| (p.plugin_name.to_lowercase(), p))
        .collect::<HashMap<_, _>>();

    for token in tokens {
        match token.kind {
            CrashTokenKind::Plugin => {
                let key = token.value.to_lowercase();
                let plugin = active_plugins.get(&key);
                let matched = input.profile.mods.iter().find(|m| {
                    m.enabled
                        && (m.mod_id.eq_ignore_ascii_case(&token.value)
                            || m.display_name
                                .as_deref()
                                .is_some_and(|name| name.eq_ignore_ascii_case(&token.value)))
                });
                upsert_suspect(
                    &mut by_key,
                    matched,
                    Some(token.value.clone()),
                    plugin.map(|p| p.sort_index),
                    token,
                    CrashConfidence::Highest,
                    "exact active plugin name mentioned by log",
                );
            }
            CrashTokenKind::Dll => {
                let needle = token.value.to_lowercase();
                let matched = input.installed_files.iter().find_map(|(mod_id, file)| {
                    file.rel_path
                        .rsplit(['/', '\\'])
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&needle))
                        .then(|| mods_by_id.get(&mod_id.to_lowercase()).copied())
                        .flatten()
                });
                let is_tool_file = input.tool_files.iter().any(|path| {
                    path.rsplit(['/', '\\'])
                        .next()
                        .is_some_and(|name| name.eq_ignore_ascii_case(&needle))
                });
                upsert_suspect(
                    &mut by_key,
                    matched,
                    None,
                    None,
                    token,
                    CrashConfidence::High,
                    if is_tool_file {
                        "DLL basename mentioned by log and also tracked as tool-applied file"
                    } else {
                        "DLL basename mentioned by log and matched to installed file manifest"
                    },
                );
            }
            CrashTokenKind::AssetPath => {
                let needle = normalize_path(&token.value);
                let matched = input.installed_files.iter().find_map(|(mod_id, file)| {
                    let rel = normalize_path(&file.rel_path);
                    (rel == needle || rel.ends_with(&needle) || needle.ends_with(&rel))
                        .then(|| mods_by_id.get(&mod_id.to_lowercase()).copied())
                        .flatten()
                });
                upsert_suspect(
                    &mut by_key,
                    matched,
                    None,
                    None,
                    token,
                    CrashConfidence::Medium,
                    "asset path mentioned by log and matched to installed file manifest",
                );
            }
            CrashTokenKind::FormId => {
                upsert_suspect(
                    &mut by_key,
                    None,
                    None,
                    None,
                    token,
                    CrashConfidence::Low,
                    "form ID mentioned by log; no local plugin ownership is inferred",
                );
            }
        }
    }

    let mut suspects = by_key.into_values().collect::<Vec<_>>();
    for suspect in &mut suspects {
        suspect.confidence = suspect
            .evidence
            .iter()
            .map(|e| match e.reason.as_str() {
                "exact active plugin name mentioned by log" => CrashConfidence::Highest,
                reason if reason.contains("DLL basename") => CrashConfidence::High,
                reason if reason.contains("asset path") => CrashConfidence::Medium,
                _ => CrashConfidence::Low,
            })
            .max()
            .unwrap_or(CrashConfidence::Low);
        let name = suspect
            .display_name
            .as_deref()
            .or(suspect.mod_id.as_deref())
            .or(suspect.plugin_name.as_deref())
            .unwrap_or("unmatched crash evidence");
        suspect.summary =
            format!("{name} is mentioned by the crash log; review the evidence rows.");
    }
    suspects.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.summary.cmp(&b.summary))
    });
    suspects
}

fn upsert_suspect(
    by_key: &mut HashMap<String, CrashSuspect>,
    matched_mod: Option<&EnabledMod>,
    plugin_name: Option<String>,
    plugin_load_index: Option<i64>,
    token: &CrashToken,
    confidence: CrashConfidence,
    reason: &str,
) {
    let key = matched_mod
        .map(|m| format!("mod:{}", m.mod_id.to_lowercase()))
        .or_else(|| {
            plugin_name
                .as_ref()
                .map(|p| format!("plugin:{}", p.to_lowercase()))
        })
        .unwrap_or_else(|| format!("token:{}:{}", token.kind as u8, token.value.to_lowercase()));

    let suspect = by_key.entry(key).or_insert_with(|| CrashSuspect {
        mod_id: matched_mod.map(|m| m.mod_id.clone()),
        display_name: matched_mod.and_then(|m| m.display_name.clone()),
        version: matched_mod.and_then(|m| m.version.clone()),
        nexus_mod_id: matched_mod
            .and_then(|m| m.nexus_mod_id)
            .and_then(|id| id.to_i64().ok()),
        nexus_file_id: matched_mod
            .and_then(|m| m.nexus_file_id)
            .and_then(|id| id.to_i64().ok()),
        nexus_game_domain: matched_mod.and_then(|m| m.nexus_game_domain.clone()),
        installed_timestamp: matched_mod.and_then(|m| m.installed_timestamp),
        plugin_name,
        plugin_load_index,
        evidence: Vec::new(),
        confidence,
        summary: String::new(),
    });

    if confidence > suspect.confidence {
        suspect.confidence = confidence;
    }
    suspect.evidence.push(CrashEvidence {
        token: token.value.clone(),
        kind: token.kind,
        section: token.section.clone(),
        reason: reason.to_string(),
        line: token.line,
    });
}

#[cfg(test)]
mod tests;

impl std::fmt::Display for CrashConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Highest => "Highest",
        })
    }
}
