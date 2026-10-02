use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::resolver::GameId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BisectOracle {
    Manual,
    Crash {
        crash_dir: PathBuf,
    },
    Perf {
        baseline_run: String,
        #[serde(default = "default_perf_p99_frame_time_percent")]
        p99_frame_time_percent: u16,
        #[serde(default = "default_perf_one_percent_low_fps_percent")]
        one_percent_low_fps_percent: u16,
        #[serde(default = "default_perf_alpha_micros")]
        alpha_micros: u32,
        #[serde(default = "default_perf_min_samples")]
        min_samples: usize,
    },
}

impl BisectOracle {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Crash { .. } => "crash",
            Self::Perf { .. } => "perf",
        }
    }
}

#[must_use]
pub const fn default_perf_p99_frame_time_percent() -> u16 {
    115
}

#[must_use]
pub const fn default_perf_one_percent_low_fps_percent() -> u16 {
    85
}

#[must_use]
pub const fn default_perf_alpha_micros() -> u32 {
    50_000
}

#[must_use]
pub const fn default_perf_min_samples() -> usize {
    30
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BisectResult {
    Good,
    Bad,
}

impl BisectResult {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Good => "good",
            Self::Bad => "bad",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BisectSaveSafety {
    Refuse,
    Force,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BisectStatus {
    Active,
    Waiting,
    Complete,
    Inconclusive,
    Aborted,
}

impl BisectStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Waiting => "waiting",
            Self::Complete => "complete",
            Self::Inconclusive => "inconclusive",
            Self::Aborted => "aborted",
        }
    }

    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "waiting" => Some(Self::Waiting),
            "complete" => Some(Self::Complete),
            "inconclusive" => Some(Self::Inconclusive),
            "aborted" => Some(Self::Aborted),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisectSession {
    pub session_id: String,
    pub game_id: GameId,
    pub source_profile_id: i64,
    pub source_profile_name: String,
    pub oracle: BisectOracle,
    pub status: BisectStatus,
    pub suspect_mod_ids: Vec<String>,
    pub known_good_mod_ids: Vec<String>,
    pub known_bad_mod_ids: Vec<String>,
    pub current_step_id: Option<i64>,
    pub current_candidate_profile: Option<String>,
    pub save_safety: BisectSaveSafety,
    pub keep_profiles: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisectStep {
    pub id: i64,
    pub session_id: String,
    pub step_index: usize,
    pub candidate_profile: String,
    pub candidate_mod_ids: Vec<String>,
    pub enabled_mod_ids: Vec<String>,
    pub disabled_mod_ids: Vec<String>,
    pub result: Option<BisectResult>,
    pub observed_signal: Option<String>,
    pub notes: Option<String>,
    pub launched_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBisectSession {
    pub session_id: String,
    pub game_id: GameId,
    pub source_profile_id: i64,
    pub source_profile_name: String,
    pub oracle: BisectOracle,
    pub suspect_mod_ids: Vec<String>,
    pub save_safety: BisectSaveSafety,
    pub keep_profiles: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBisectStep {
    pub session_id: String,
    pub step_index: usize,
    pub candidate_profile: String,
    pub candidate_mod_ids: Vec<String>,
    pub enabled_mod_ids: Vec<String>,
    pub disabled_mod_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidatePlan {
    pub candidate_mod_ids: Vec<String>,
    pub enabled_mod_ids: Vec<String>,
    pub disabled_mod_ids: Vec<String>,
}

#[must_use]
pub fn next_candidate(session: &BisectSession) -> Option<CandidatePlan> {
    next_candidate_with_dependencies(session, &HashMap::new())
}

#[must_use]
pub fn next_candidate_with_dependencies(
    session: &BisectSession,
    dependencies: &HashMap<String, Vec<String>>,
) -> Option<CandidatePlan> {
    if session.suspect_mod_ids.len() <= 1 {
        return None;
    }

    let split = session.suspect_mod_ids.len() / 2;
    let mut disabled_mod_ids: BTreeSet<String> =
        session.suspect_mod_ids[..split].iter().cloned().collect();
    let mut enabled_mod_ids: BTreeSet<String> =
        session.suspect_mod_ids[split..].iter().cloned().collect();
    let suspect_ids: HashSet<&str> = session.suspect_mod_ids.iter().map(String::as_str).collect();

    loop {
        let mut moved = Vec::new();
        for enabled in &enabled_mod_ids {
            let Some(deps) = dependencies.get(enabled) else {
                continue;
            };
            for dep in deps {
                if suspect_ids.contains(dep.as_str()) && disabled_mod_ids.contains(dep) {
                    moved.push(dep.clone());
                }
            }
        }
        if moved.is_empty() {
            break;
        }
        for dep in moved {
            disabled_mod_ids.remove(&dep);
            enabled_mod_ids.insert(dep);
        }
    }

    let disabled_mod_ids = session
        .suspect_mod_ids
        .iter()
        .filter(|id| disabled_mod_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();
    let enabled_mod_ids = session
        .suspect_mod_ids
        .iter()
        .filter(|id| enabled_mod_ids.contains(*id))
        .cloned()
        .collect::<Vec<_>>();

    Some(CandidatePlan {
        candidate_mod_ids: disabled_mod_ids.clone(),
        enabled_mod_ids,
        disabled_mod_ids,
    })
}

#[must_use]
pub fn apply_result(step: &BisectStep, result: BisectResult) -> Vec<String> {
    match result {
        BisectResult::Good => step.disabled_mod_ids.clone(),
        BisectResult::Bad => step.enabled_mod_ids.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(ids: &[&str]) -> BisectSession {
        BisectSession {
            session_id: "b".to_string(),
            game_id: GameId::from("skyrim-se"),
            source_profile_id: 1,
            source_profile_name: "main".to_string(),
            oracle: BisectOracle::Manual,
            status: BisectStatus::Active,
            suspect_mod_ids: ids.iter().map(std::string::ToString::to_string).collect(),
            known_good_mod_ids: Vec::new(),
            known_bad_mod_ids: Vec::new(),
            current_step_id: None,
            current_candidate_profile: None,
            save_safety: BisectSaveSafety::Refuse,
            keep_profiles: false,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn splits_even_sets() {
        let plan = next_candidate(&session(&["a", "b", "c", "d"])).unwrap();
        assert_eq!(plan.disabled_mod_ids, ["a", "b"]);
        assert_eq!(plan.enabled_mod_ids, ["c", "d"]);
    }

    #[test]
    fn splits_odd_sets() {
        let plan = next_candidate(&session(&["a", "b", "c", "d", "e"])).unwrap();
        assert_eq!(plan.disabled_mod_ids, ["a", "b"]);
        assert_eq!(plan.enabled_mod_ids, ["c", "d", "e"]);
    }

    #[test]
    fn no_candidate_for_single_suspect() {
        assert!(next_candidate(&session(&["a"])).is_none());
    }

    #[test]
    fn keeps_dependencies_enabled_with_dependents() {
        let mut dependencies = HashMap::new();
        dependencies.insert("patch".to_string(), vec!["master".to_string()]);

        let plan = next_candidate_with_dependencies(
            &session(&["master", "texture", "patch", "cosmetic"]),
            &dependencies,
        )
        .unwrap();

        assert_eq!(plan.disabled_mod_ids, ["texture"]);
        assert_eq!(plan.enabled_mod_ids, ["master", "patch", "cosmetic"]);
    }

    #[test]
    fn keeps_transitive_dependencies_enabled() {
        let mut dependencies = HashMap::new();
        dependencies.insert("patch".to_string(), vec!["framework".to_string()]);
        dependencies.insert("framework".to_string(), vec!["master".to_string()]);

        let plan = next_candidate_with_dependencies(
            &session(&["master", "framework", "patch", "cosmetic"]),
            &dependencies,
        )
        .unwrap();

        assert_eq!(plan.disabled_mod_ids, [] as [std::string::String; 0]);
        assert_eq!(
            plan.enabled_mod_ids,
            ["master", "framework", "patch", "cosmetic"]
        );
    }
}
