use std::fmt;

use serde::{Deserialize, Serialize};

pub const CURRENT_SCHEMA_VERSION: u16 = 1;
pub const MAX_EVENTS_PER_BATCH: usize = 100;
pub const MAX_HASH_LEN: usize = 96;
pub const MAX_GAME_ID_LEN: usize = 64;
pub const MAX_PLATFORM_LEN: usize = 32;
pub const DEFAULT_MIN_COHORT: i64 = 250;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompatEventKind {
    Session,
    Crash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ConfidenceBucket {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OracleConfigResponse {
    pub schema_version: u16,
    pub salt_epoch: String,
    pub min_cohort: i64,
    pub max_events_per_batch: usize,
}

impl Default for OracleConfigResponse {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            salt_epoch: "epoch-1".to_string(),
            min_cohort: DEFAULT_MIN_COHORT,
            max_events_per_batch: MAX_EVENTS_PER_BATCH,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatEventBatch {
    pub schema_version: u16,
    pub client_version: String,
    pub events: Vec<CompatEvent>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatEvent {
    pub game_id: String,
    pub platform: String,
    pub salt_epoch: String,
    pub mod_set_hash: String,
    pub mod_hashes: Vec<String>,
    pub pair_hashes: Vec<String>,
    pub crash_signature_hash: Option<String>,
    pub kind: CompatEventKind,
    pub observed_at_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompatEventAccepted {
    pub accepted: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatQueryResponse {
    pub game_id: String,
    pub stats: Vec<CompatAggregateStat>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompatAggregateStat {
    pub pair_hash: String,
    pub coinstall_count: i64,
    pub crash_signature_rate: f64,
    pub baseline_rate: f64,
    pub lift: f64,
    pub confidence: ConfidenceBucket,
    pub window_start_unix: i64,
    pub window_end_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ValidationError {
    #[error("unsupported schema version {0}")]
    UnsupportedSchema(u16),
    #[error("batch is empty")]
    EmptyBatch,
    #[error("batch has {actual} events, maximum is {max}")]
    BatchTooLarge { actual: usize, max: usize },
    #[error("{field} is empty")]
    EmptyField { field: &'static str },
    #[error("{field} is too long")]
    FieldTooLong { field: &'static str },
    #[error("{field} contains raw or unsafe data")]
    UnsafeField { field: &'static str },
    #[error("event has no mod hashes")]
    EmptyModSet,
    #[error("crash event is missing crash_signature_hash")]
    MissingCrashSignature,
}

impl CompatEventBatch {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(ValidationError::UnsupportedSchema(self.schema_version));
        }
        validate_plain_field("client_version", &self.client_version, 64)?;
        if self.events.is_empty() {
            return Err(ValidationError::EmptyBatch);
        }
        if self.events.len() > MAX_EVENTS_PER_BATCH {
            return Err(ValidationError::BatchTooLarge {
                actual: self.events.len(),
                max: MAX_EVENTS_PER_BATCH,
            });
        }
        for event in &self.events {
            event.validate()?;
        }
        Ok(())
    }
}

impl CompatEvent {
    pub fn validate(&self) -> Result<(), ValidationError> {
        validate_plain_field("game_id", &self.game_id, MAX_GAME_ID_LEN)?;
        validate_plain_field("platform", &self.platform, MAX_PLATFORM_LEN)?;
        validate_plain_field("salt_epoch", &self.salt_epoch, 64)?;
        validate_hash_field("mod_set_hash", &self.mod_set_hash)?;
        if self.mod_hashes.is_empty() {
            return Err(ValidationError::EmptyModSet);
        }
        for hash in &self.mod_hashes {
            validate_hash_field("mod_hashes", hash)?;
        }
        for hash in &self.pair_hashes {
            validate_hash_field("pair_hashes", hash)?;
        }
        if let Some(hash) = &self.crash_signature_hash {
            validate_hash_field("crash_signature_hash", hash)?;
        } else if self.kind == CompatEventKind::Crash {
            return Err(ValidationError::MissingCrashSignature);
        }
        Ok(())
    }
}

fn validate_plain_field(
    field: &'static str,
    value: &str,
    max_len: usize,
) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::EmptyField { field });
    }
    if value.len() > max_len {
        return Err(ValidationError::FieldTooLong { field });
    }
    if contains_raw_or_path_like_data(value) {
        return Err(ValidationError::UnsafeField { field });
    }
    Ok(())
}

fn validate_hash_field(field: &'static str, value: &str) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyField { field });
    }
    if value.len() > MAX_HASH_LEN {
        return Err(ValidationError::FieldTooLong { field });
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'-' || b == b'_')
        || contains_raw_or_path_like_data(value)
    {
        return Err(ValidationError::UnsafeField { field });
    }
    Ok(())
}

fn contains_raw_or_path_like_data(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    value.contains('/')
        || value.contains('\\')
        || value.contains('~')
        || lower.contains(".esp")
        || lower.contains(".esm")
        || lower.contains(".esl")
        || lower.contains(".dll")
        || lower.contains("crashloggersse")
        || lower.contains("exception")
        || lower.contains("stack trace")
        || lower.contains("users")
        || lower.contains("appdata")
        || lower.contains("steamapps")
}

impl fmt::Display for ConfidenceBucket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Low => f.write_str("low"),
            Self::Medium => f.write_str("medium"),
            Self::High => f.write_str("high"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_event() -> CompatEvent {
        CompatEvent {
            game_id: "skyrim-se".to_string(),
            platform: "x86_64-linux".to_string(),
            salt_epoch: "epoch-1".to_string(),
            mod_set_hash: "a".repeat(64),
            mod_hashes: vec!["b".repeat(64), "c".repeat(64)],
            pair_hashes: vec!["d".repeat(64)],
            crash_signature_hash: Some("e".repeat(64)),
            kind: CompatEventKind::Crash,
            observed_at_unix: 1_784_000_000,
        }
    }

    #[test]
    fn validates_safe_batch() {
        let batch = CompatEventBatch {
            schema_version: CURRENT_SCHEMA_VERSION,
            client_version: "0.3.9".to_string(),
            events: vec![valid_event()],
        };
        batch.validate().expect("safe event accepted");
    }

    #[test]
    fn rejects_raw_paths_and_crash_text() {
        let mut event = valid_event();
        event.game_id = "C:\\Users\\person\\crash.log".to_string();
        assert!(matches!(
            event.validate(),
            Err(ValidationError::UnsafeField { field: "game_id" })
        ));

        let mut event = valid_event();
        event.mod_hashes = vec!["Unhandled exception at SomeMod.dll".to_string()];
        assert!(matches!(
            event.validate(),
            Err(ValidationError::UnsafeField {
                field: "mod_hashes"
            })
        ));
    }

    #[test]
    fn batch_limits_are_enforced() {
        let batch = |schema_version, events| CompatEventBatch {
            schema_version,
            client_version: "0.3.9".to_string(),
            events,
        };
        assert_eq!(
            batch(2, vec![valid_event()]).validate(),
            Err(ValidationError::UnsupportedSchema(2))
        );
        assert_eq!(
            batch(CURRENT_SCHEMA_VERSION, Vec::new()).validate(),
            Err(ValidationError::EmptyBatch)
        );
        batch(
            CURRENT_SCHEMA_VERSION,
            vec![valid_event(); MAX_EVENTS_PER_BATCH],
        )
        .validate()
        .expect("a full batch is accepted");
        assert_eq!(
            batch(
                CURRENT_SCHEMA_VERSION,
                vec![valid_event(); MAX_EVENTS_PER_BATCH + 1]
            )
            .validate(),
            Err(ValidationError::BatchTooLarge {
                actual: MAX_EVENTS_PER_BATCH + 1,
                max: MAX_EVENTS_PER_BATCH,
            })
        );
    }

    #[test]
    fn event_fields_are_bounded_and_hash_shaped() {
        let check = |edit: fn(&mut CompatEvent)| {
            let mut event = valid_event();
            edit(&mut event);
            event.validate()
        };
        assert_eq!(
            check(|e| e.game_id = "g".repeat(MAX_GAME_ID_LEN + 1)),
            Err(ValidationError::FieldTooLong { field: "game_id" })
        );
        assert_eq!(
            check(|e| e.platform = " ".to_string()),
            Err(ValidationError::EmptyField { field: "platform" })
        );
        assert_eq!(
            check(|e| e.mod_set_hash = "a".repeat(MAX_HASH_LEN + 1)),
            Err(ValidationError::FieldTooLong {
                field: "mod_set_hash"
            })
        );
        assert_eq!(
            check(|e| e.mod_hashes.clear()),
            Err(ValidationError::EmptyModSet)
        );
        assert_eq!(
            check(|e| e.pair_hashes = vec!["g".repeat(64)]),
            Err(ValidationError::UnsafeField {
                field: "pair_hashes"
            })
        );
        assert_eq!(
            check(|e| e.mod_set_hash = "AB:cd-ef_01".to_string()),
            Ok(())
        );
        assert_eq!(
            check(|e| {
                e.kind = CompatEventKind::Session;
                e.crash_signature_hash = None;
            }),
            Ok(())
        );
    }

    #[test]
    fn rejects_crash_without_signature() {
        let mut event = valid_event();
        event.crash_signature_hash = None;
        assert_eq!(
            event.validate(),
            Err(ValidationError::MissingCrashSignature)
        );
    }
}
