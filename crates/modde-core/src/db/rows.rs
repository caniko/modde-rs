//! Row mapping and persistence encoding helpers.
#![allow(clippy::wildcard_imports)]

use super::*;

pub(super) fn map_enabled_mod(r: &dyn DbRow) -> Result<EnabledMod> {
    let nexus_mod_id = r.opt_i64(5)?.map(NexusModId::try_from).transpose()?;
    let nexus_file_id = r.opt_i64(6)?.map(NexusFileId::try_from).transpose()?;
    Ok(EnabledMod {
        mod_id: r.string(0)?,
        display_name: r.opt_string(1)?,
        enabled: r.bool(2)?,
        version: r.opt_string(3)?,
        fomod_config: r.opt_string(4)?,
        nexus_mod_id,
        nexus_file_id,
        nexus_game_domain: r.opt_string(7)?,
        installed_timestamp: r.opt_i64(8)?,
        category_id: r.opt_i64(9)?,
        notes: r.opt_string(10)?,
        tags: decode_tags(r.opt_string(11)?.as_deref())?,
        lock: decode_lock_reason(r.opt_string(12)?.as_deref())?,
        install_method: decode_install_method(r.opt_string(13)?.as_deref())?,
        source_archive_hash: r.opt_string(14)?,
        install_status: decode_install_status(r.opt_string(15)?.as_deref())?,
    })
}

pub(super) fn executable_from_row(r: &dyn DbRow) -> Result<ExecutableConfigRow> {
    Ok(ExecutableConfigRow {
        game_id: r.string(0)?,
        name: r.string(1)?,
        executable_path: PathBuf::from(r.string(2)?),
        arguments_json: r.string(3)?,
        working_dir: r.opt_string(4)?.map(PathBuf::from),
        environment_json: r.string(5)?,
        wine_dll_overrides: r.opt_string(6)?,
        output_mod: r.string(7)?,
        enabled: r.bool(8)?,
    })
}

pub(super) fn performance_run_from_row(r: &dyn DbRow) -> Result<PerformanceRunRow> {
    Ok(PerformanceRunRow {
        run_id: r.string(0)?,
        game_id: GameId::from(r.string(1)?),
        profile_id: r.opt_i64(2)?,
        profile_name: r.string(3)?,
        mod_set_hash: r.string(4)?,
        mod_snapshot_json: r.string(5)?,
        experiment_depth: r.i64(6)?.max(0) as usize,
        label: r.opt_string(7)?,
        status: r.string(8)?,
        started_at: r.string(9)?,
        finished_at: r.opt_string(10)?,
        mangohud_csv_path: r.opt_string(11)?.map(PathBuf::from),
        exit_status: r.opt_i64(12)?,
        summary: PerformanceSummary {
            sample_count: r.opt_i64(13)?.unwrap_or(0).max(0) as usize,
            duration_seconds: r.opt_f64(14)?,
            median_fps: r.opt_f64(15)?,
            average_fps: r.opt_f64(16)?,
            one_percent_low_fps: r.opt_f64(17)?,
            point_one_percent_low_fps: r.opt_f64(18)?,
            median_frame_time_ms: r.opt_f64(19)?,
            p95_frame_time_ms: r.opt_f64(20)?,
            p99_frame_time_ms: r.opt_f64(21)?,
        },
    })
}

pub(super) fn performance_sample_from_row(r: &dyn DbRow) -> Result<PerformanceSample> {
    Ok(PerformanceSample {
        elapsed_seconds: r.opt_f64(0)?,
        fps: r.opt_f64(1)?.unwrap_or_default(),
        frame_time_ms: r.opt_f64(2)?,
        cpu_load: r.opt_f64(3)?,
        gpu_load: r.opt_f64(4)?,
    })
}

pub(super) fn bisect_session_from_row(r: &dyn DbRow) -> Result<BisectSession> {
    let status_raw = r.string(5)?;
    let status = BisectStatus::parse(&status_raw).ok_or_else(|| {
        CoreError::Other(format!("invalid bisect session status: {status_raw}").into())
    })?;
    let safety_raw = r.string(11)?;
    let save_safety = match safety_raw.as_str() {
        "refuse" => BisectSaveSafety::Refuse,
        "force" => BisectSaveSafety::Force,
        _ => {
            return Err(CoreError::Other(
                format!("invalid bisect save safety mode: {safety_raw}").into(),
            ));
        }
    };
    Ok(BisectSession {
        session_id: r.string(0)?,
        game_id: GameId::from(r.string(1)?),
        source_profile_id: r.i64(2)?,
        source_profile_name: r.string(3)?,
        oracle: decode_json(&r.string(4)?, "bisect oracle")?,
        status,
        suspect_mod_ids: decode_json(&r.string(6)?, "bisect suspect mod ids")?,
        known_good_mod_ids: decode_json(&r.string(7)?, "bisect known good mod ids")?,
        known_bad_mod_ids: decode_json(&r.string(8)?, "bisect known bad mod ids")?,
        current_step_id: r.opt_i64(9)?,
        current_candidate_profile: r.opt_string(10)?,
        save_safety,
        keep_profiles: r.bool(12)?,
        created_at: r.string(13)?,
        updated_at: r.string(14)?,
    })
}

pub(super) fn bisect_step_from_row(r: &dyn DbRow) -> Result<BisectStep> {
    let result = match r.opt_string(7)?.as_deref() {
        None => None,
        Some("good") => Some(BisectResult::Good),
        Some("bad") => Some(BisectResult::Bad),
        Some(raw) => {
            return Err(CoreError::Other(
                format!("invalid bisect step result: {raw}").into(),
            ));
        }
    };
    Ok(BisectStep {
        id: r.i64(0)?,
        session_id: r.string(1)?,
        step_index: r.i64(2)?.max(0) as usize,
        candidate_profile: r.string(3)?,
        candidate_mod_ids: decode_json(&r.string(4)?, "bisect candidate mod ids")?,
        enabled_mod_ids: decode_json(&r.string(5)?, "bisect enabled mod ids")?,
        disabled_mod_ids: decode_json(&r.string(6)?, "bisect disabled mod ids")?,
        result,
        observed_signal: r.opt_string(8)?,
        notes: r.opt_string(9)?,
        launched_at: r.string(10)?,
    })
}

pub(super) fn encode_json(value: &(impl serde::Serialize + ?Sized), label: &str) -> Result<String> {
    serde_json::to_string(value)
        .map_err(|e| CoreError::Other(format!("failed to encode {label}: {e}").into()))
}

pub(super) fn decode_json<T: serde::de::DeserializeOwned>(raw: &str, label: &str) -> Result<T> {
    serde_json::from_str(raw)
        .map_err(|e| CoreError::Other(format!("failed to decode {label}: {e}").into()))
}

pub(super) fn patcher_stage_from_row(r: &dyn DbRow) -> Result<PatcherStageRow> {
    let stage_kind = PatcherStageKind::parse(&r.string(2)?)?;
    let settings_json = r.string(5)?;
    let settings = serde_json::from_str::<PatcherStageSettings>(&settings_json).map_err(|e| {
        CoreError::Other(format!("failed to parse patcher stage settings: {e}").into())
    })?;
    if settings.kind() != stage_kind {
        return Err(CoreError::Validation(
            format!(
                "patcher stage kind '{}' does not match serialized settings kind '{}'",
                stage_kind.as_str(),
                settings.kind().as_str()
            )
            .into(),
        ));
    }
    Ok(PatcherStageRow {
        profile_id: r.i64(0)?,
        name: r.string(1)?,
        stage_kind,
        enabled: r.bool(3)?,
        sort_index: r.i64(4)?,
        settings,
        output_mod: r.string(6)?,
        last_cache_key: r.opt_string(7)?,
        last_success_at: r.opt_string(8)?,
        timeout_seconds: r.opt_i64(9)?.unwrap_or(1_800).max(1) as u64,
    })
}

pub(super) fn patcher_stage_output_from_row(r: &dyn DbRow) -> Result<PatcherStageOutputRow> {
    Ok(PatcherStageOutputRow {
        profile_id: r.i64(0)?,
        stage_name: r.string(1)?,
        rel_path: r.string(2)?,
    })
}

// ── Source encoding ──────────────────────────────────────────

pub(super) fn encode_source(source: &ProfileSource) -> (&'static str, Option<String>) {
    match source {
        ProfileSource::Manual => ("manual", None),
        ProfileSource::NexusCollection { slug, version } => {
            // `toml::Value` renders valid TOML strings; Rust `{:?}` escapes are not.
            let data = format!(
                "slug = {}\nversion = {}",
                toml::Value::from(slug.as_str()),
                toml::Value::from(version.as_str())
            );
            ("nexus_collection", Some(data))
        }
        ProfileSource::Wabbajack { manifest_hash } => {
            let data = format!(
                "manifest_hash = {}",
                toml::Value::from(manifest_hash.as_str())
            );
            ("wabbajack", Some(data))
        }
    }
}

pub(super) fn decode_source(source_type: &str, source_data: Option<&str>) -> Result<ProfileSource> {
    match source_type {
        "manual" => Ok(ProfileSource::Manual),
        "nexus_collection" => {
            let data = source_data.unwrap_or_default();
            let table: toml::Table = toml::from_str(data).map_err(|e| {
                CoreError::Other(
                    format!("failed to parse nexus_collection source data: {e}").into(),
                )
            })?;
            let slug = table
                .get("slug")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let version = table
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(ProfileSource::NexusCollection { slug, version })
        }
        "wabbajack" => {
            let data = source_data.unwrap_or_default();
            let table: toml::Table = toml::from_str(data).map_err(|e| {
                CoreError::Other(format!("failed to parse wabbajack source data: {e}").into())
            })?;
            let manifest_hash = table
                .get("manifest_hash")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            Ok(ProfileSource::Wabbajack { manifest_hash })
        }
        other => Err(CoreError::Other(
            format!("unknown profile source type: {other}").into(),
        )),
    }
}

// ── Load order lock encoding ─────────────────────────────────

pub(super) fn encode_lock(lock: Option<&LoadOrderLock>) -> Option<String> {
    lock.map(|l| toml::to_string(l).expect("LoadOrderLock should always serialize"))
}

pub(super) fn decode_lock(raw: Option<&str>) -> Result<Option<LoadOrderLock>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => toml::from_str::<LoadOrderLock>(s)
            .map(Some)
            .map_err(|e| CoreError::Other(format!("failed to parse load_order_lock: {e}").into())),
    }
}

pub(super) fn encode_lock_reason(reason: Option<&LockReason>) -> Option<String> {
    reason.map(|r| toml::to_string(r).expect("LockReason should always serialize"))
}

pub(super) fn decode_lock_reason(raw: Option<&str>) -> Result<Option<LockReason>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => toml::from_str::<LockReason>(s)
            .map(Some)
            .map_err(|e| CoreError::Other(format!("failed to parse lock_reason: {e}").into())),
    }
}

// ── Installer method encoding (V8) ───────────────────────────

pub(super) fn encode_install_method(method: &InstallMethod) -> Result<String> {
    toml::to_string(method)
        .map_err(|e| CoreError::Other(format!("failed to encode install_method: {e}").into()))
}

/// Parse the TOML-encoded `install_method` column back into a typed
/// [`InstallMethod`]. Returns `None` for NULL / empty strings.
pub fn decode_install_method(raw: Option<&str>) -> Result<Option<InstallMethod>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => toml::from_str::<InstallMethod>(s)
            .map(Some)
            .map_err(|e| CoreError::Other(format!("failed to parse install_method: {e}").into())),
    }
}

pub(super) fn encode_tags(tags: &[String]) -> Result<Option<String>> {
    if tags.is_empty() {
        Ok(None)
    } else {
        serde_json::to_string(tags)
            .map(Some)
            .map_err(CoreError::Json)
    }
}

pub(super) fn decode_tags(raw: Option<&str>) -> Result<Vec<String>> {
    match raw {
        None => Ok(Vec::new()),
        Some(s) if s.is_empty() => Ok(Vec::new()),
        Some(s) => serde_json::from_str::<Vec<String>>(s)
            .map_err(|e| CoreError::Other(format!("failed to parse tags JSON: {e}").into())),
    }
}

pub(super) fn decode_install_status(raw: Option<&str>) -> Result<Option<InstallStatus>> {
    match raw {
        None => Ok(None),
        Some(s) if s.is_empty() => Ok(None),
        Some(s) => InstallStatus::parse(s)
            .map(Some)
            .ok_or_else(|| CoreError::Other(format!("unknown install_status: {s}").into())),
    }
}

pub(super) fn new_tool_setting_node_id(game_id: &GameId, tool_id: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let game = sanitize_node_id_part(game_id.as_str());
    let tool = sanitize_node_id_part(tool_id);
    format!("tool-{game}-{tool}-{nanos}-{}", std::process::id())
}

pub(super) fn sanitize_node_id_part(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect()
}
