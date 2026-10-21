use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NexusIdError {
    #[error("Nexus id cannot be negative: {0}")]
    Negative(i64),
    #[error("Nexus id {0} exceeds SQLite INTEGER range")]
    TooLarge(u64),
}

macro_rules! nexus_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }

            pub fn to_i64(self) -> Result<i64, NexusIdError> {
                i64::try_from(self.0).map_err(|_| NexusIdError::TooLarge(self.0))
            }
        }

        impl From<u64> for $name {
            fn from(value: u64) -> Self {
                Self(value)
            }
        }

        impl TryFrom<i64> for $name {
            type Error = NexusIdError;

            fn try_from(value: i64) -> Result<Self, Self::Error> {
                if value < 0 {
                    return Err(NexusIdError::Negative(value));
                }
                Ok(Self(value as u64))
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

nexus_id!(NexusModId);
nexus_id!(NexusFileId);

/// Reject Nexus game domains that are not a plain `[A-Za-z0-9_-]+` token.
///
/// Domains arrive from URLs, `nxm://` links and imported profile data, and
/// are interpolated into store, staging and dossier directory names.
pub fn validate_game_domain(domain: &str) -> crate::error::Result<()> {
    if domain.is_empty()
        || !domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(crate::error::CoreError::Validation(
            format!("invalid Nexus game domain: {domain:?}").into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_game_domain;

    #[test]
    fn game_domains_must_be_plain_tokens() {
        for ok in ["skyrimspecialedition", "fallout4", "mount-and-blade_2"] {
            validate_game_domain(ok).unwrap_or_else(|e| panic!("{ok:?} rejected: {e}"));
        }
        for bad in ["", "..", "a/b", "a\\b", "sky.rim", "sky rim", "%2E%2E"] {
            assert!(validate_game_domain(bad).is_err(), "{bad:?} accepted");
        }
    }
}
