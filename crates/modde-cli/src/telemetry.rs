use std::{
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::Context;
use modde_core::crash::{CrashCorrelationReport, CrashTokenKind};
use modde_oracle_api::{
    CURRENT_SCHEMA_VERSION, CompatEvent, CompatEventBatch, CompatEventKind, OracleConfigResponse,
};
use sha2::{Digest, Sha256};
use uuid::Uuid;

const INSTALL_ID_FILE: &str = "install-id";
const COMPAT_QUEUE_FILE: &str = "compat-oracle-queue.jsonl";

pub fn persistent_install_id() -> anyhow::Result<Uuid> {
    persistent_install_id_in(data_dir()?)
}

fn data_dir() -> anyhow::Result<PathBuf> {
    dirs::data_local_dir()
        .map(|dir| dir.join("modde-rs"))
        .context("could not determine local data directory for telemetry install id")
}

fn persistent_install_id_in(dir: PathBuf) -> anyhow::Result<Uuid> {
    let path = dir.join(INSTALL_ID_FILE);
    if let Some(id) = read_install_id(&path)? {
        Ok(id)
    } else {
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create telemetry data dir {}", dir.display()))?;
        let id = Uuid::new_v4();
        fs::write(&path, format!("{id}\n"))
            .with_context(|| format!("failed to write telemetry install id {}", path.display()))?;
        Ok(id)
    }
}

fn read_install_id(path: &Path) -> anyhow::Result<Option<Uuid>> {
    match fs::read_to_string(path) {
        Ok(value) => Uuid::parse_str(value.trim())
            .map(Some)
            .with_context(|| format!("invalid telemetry install id in {}", path.display())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .with_context(|| format!("failed to read telemetry install id {}", path.display())),
    }
}

pub fn telemetry_dir() -> anyhow::Result<PathBuf> {
    data_dir().map(|dir| dir.join("telemetry"))
}

pub async fn try_report_compatibility_crash(report: &CrashCorrelationReport) -> anyhow::Result<()> {
    let Some(endpoint) = compatibility_oracle_endpoint()? else {
        return Ok(());
    };
    if !compatibility_oracle_enabled() {
        return Ok(());
    }

    let client = reqwest::Client::new();
    let config = fetch_oracle_config(&client, &endpoint)
        .await
        .unwrap_or_default();
    flush_compatibility_queue(&client, &endpoint).await?;
    let Some(batch) = compatibility_batch_from_report(report, &config) else {
        return Ok(());
    };
    if let Err(error) = send_compatibility_batch(&client, &endpoint, &batch).await {
        queue_compatibility_batch(&batch)?;
        return Err(error);
    }
    Ok(())
}

fn compatibility_oracle_enabled() -> bool {
    std::env::var("MODDE_COMPAT_ORACLE_OPT_IN")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

fn compatibility_oracle_endpoint() -> anyhow::Result<Option<url::Url>> {
    let Some(value) = std::env::var("MODDE_COMPAT_ORACLE_ENDPOINT").ok() else {
        return Ok(None);
    };
    Ok(Some(
        url::Url::parse(&value).context("invalid MODDE_COMPAT_ORACLE_ENDPOINT")?,
    ))
}

async fn fetch_oracle_config(
    client: &reqwest::Client,
    endpoint: &url::Url,
) -> anyhow::Result<OracleConfigResponse> {
    let url = endpoint.join("v1/compat/config")?;
    let response = client.get(url).send().await?.error_for_status()?;
    Ok(response.json().await?)
}

fn compatibility_batch_from_report(
    report: &CrashCorrelationReport,
    config: &OracleConfigResponse,
) -> Option<CompatEventBatch> {
    let mut mod_hashes = report
        .suspects
        .iter()
        .filter_map(|suspect| {
            let identity = if let (Some(domain), Some(mod_id)) =
                (suspect.nexus_game_domain.as_deref(), suspect.nexus_mod_id)
            {
                format!(
                    "nexus:{}:{}:{}:{}",
                    normalize_identity(domain),
                    mod_id,
                    suspect
                        .nexus_file_id
                        .map(|id| id.to_string())
                        .unwrap_or_default(),
                    suspect.version.as_deref().unwrap_or("")
                )
            } else {
                let local_id = suspect
                    .mod_id
                    .as_deref()
                    .or(suspect.plugin_name.as_deref())?;
                format!(
                    "local:{}:{}:{}",
                    config.salt_epoch,
                    normalize_identity(local_id),
                    suspect.version.as_deref().unwrap_or("")
                )
            };
            Some(hash_hex(identity.as_bytes()))
        })
        .collect::<Vec<_>>();
    mod_hashes.sort();
    mod_hashes.dedup();
    if mod_hashes.is_empty() {
        return None;
    }

    let pair_hashes = pair_hashes(&mod_hashes);
    let event = CompatEvent {
        game_id: sanitize_game_id(&report.game_id),
        platform: target_platform(),
        salt_epoch: config.salt_epoch.clone(),
        mod_set_hash: hash_hex(mod_hashes.join("\0").as_bytes()),
        mod_hashes,
        pair_hashes,
        crash_signature_hash: Some(crash_signature_hash(report)),
        kind: CompatEventKind::Crash,
        observed_at_unix: now_unix(),
    };
    let batch = CompatEventBatch {
        schema_version: CURRENT_SCHEMA_VERSION,
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        events: vec![event],
    };
    batch.validate().ok()?;
    Some(batch)
}

fn sanitize_game_id(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .take(64)
        .collect()
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn crash_signature_hash(report: &CrashCorrelationReport) -> String {
    let mut parts = vec![report.format.as_str().to_string()];
    if let Some(line) = &report.signature.exception_line {
        parts.push(hash_hex(line.as_bytes()));
    }
    for token in &report.signature.tokens {
        let kind = match token.kind {
            CrashTokenKind::Plugin => "plugin",
            CrashTokenKind::Dll => "dll",
            CrashTokenKind::AssetPath => "asset",
            CrashTokenKind::FormId => "form",
        };
        parts.push(format!("{kind}:{}", hash_hex(token.value.as_bytes())));
    }
    parts.sort();
    hash_hex(parts.join("\0").as_bytes())
}

fn pair_hashes(mod_hashes: &[String]) -> Vec<String> {
    let mut pairs = Vec::new();
    for (idx, left) in mod_hashes.iter().enumerate() {
        for right in mod_hashes.iter().skip(idx + 1) {
            pairs.push(hash_hex(format!("{left}\0{right}").as_bytes()));
        }
    }
    pairs
}

fn hash_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(&mut output, "{byte:02x}").expect("writing to String cannot fail");
            output
        })
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

async fn flush_compatibility_queue(
    client: &reqwest::Client,
    endpoint: &url::Url,
) -> anyhow::Result<()> {
    let path = telemetry_dir()?.join(COMPAT_QUEUE_FILE);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    let mut retained = Vec::new();
    for line in content.lines().filter(|line| !line.trim().is_empty()) {
        let batch: CompatEventBatch = serde_json::from_str(line)?;
        if let Err(error) = send_compatibility_batch(client, endpoint, &batch).await {
            retained.push(line.to_string());
            tracing::warn!(%error, "retaining queued compatibility oracle batch");
        }
    }
    if retained.is_empty() {
        fs::remove_file(path).ok();
    } else {
        fs::write(path, format!("{}\n", retained.join("\n")))?;
    }
    Ok(())
}

async fn send_compatibility_batch(
    client: &reqwest::Client,
    endpoint: &url::Url,
    batch: &CompatEventBatch,
) -> anyhow::Result<()> {
    let url = endpoint.join("v1/compat/events")?;
    client
        .post(url)
        .json(batch)
        .send()
        .await?
        .error_for_status()?;
    Ok(())
}

fn queue_compatibility_batch(batch: &CompatEventBatch) -> anyhow::Result<()> {
    let dir = telemetry_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(COMPAT_QUEUE_FILE);
    let mut line = serde_json::to_string(batch)?;
    line.push('\n');
    let mut options = fs::OpenOptions::new();
    options.create(true).append(true);
    use std::io::Write;
    options.open(path)?.write_all(line.as_bytes())?;
    Ok(())
}

fn target_platform() -> String {
    format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS)
}

#[cfg(test)]
mod tests {
    use modde_core::crash::{
        CrashConfidence, CrashCorrelationReport, CrashLogFormat, CrashSignature, CrashSuspect,
    };

    use super::{
        compatibility_batch_from_report, compatibility_oracle_enabled, persistent_install_id_in,
    };

    #[test]
    fn persistent_install_id_round_trips() {
        let dir = tempfile::tempdir().expect("temp dir");

        let first = persistent_install_id_in(dir.path().to_path_buf()).expect("first id");
        let second = persistent_install_id_in(dir.path().to_path_buf()).expect("second id");

        assert_eq!(first, second);
    }

    #[test]
    fn compatibility_oracle_is_disabled_by_default() {
        // SAFETY: this is the only test in this module that mutates this
        // opt-in variable, and it removes it before reading the setting.
        unsafe {
            std::env::remove_var("MODDE_COMPAT_ORACLE_OPT_IN");
        }
        assert!(!compatibility_oracle_enabled());
    }

    #[test]
    fn compatibility_payload_excludes_raw_names() {
        let report = CrashCorrelationReport {
            game_id: "skyrim-se".to_string(),
            profile_name: "private-profile".to_string(),
            source_path: "/home/user/crash.log".into(),
            raw_sha256: "a".repeat(64),
            format: CrashLogFormat::CrashLoggerSse,
            signature: CrashSignature {
                detected_format: CrashLogFormat::CrashLoggerSse,
                exception_line: Some("Unhandled exception at SecretMod.dll".to_string()),
                tokens: Vec::new(),
            },
            suspects: vec![
                CrashSuspect {
                    mod_id: Some("Secret Local Mod".to_string()),
                    display_name: Some("Secret Local Mod".to_string()),
                    version: Some("1.0".to_string()),
                    nexus_mod_id: None,
                    nexus_file_id: None,
                    nexus_game_domain: None,
                    installed_timestamp: None,
                    plugin_name: Some("SecretMod.esp".to_string()),
                    plugin_load_index: Some(1),
                    evidence: Vec::new(),
                    confidence: CrashConfidence::High,
                    summary: String::new(),
                },
                CrashSuspect {
                    mod_id: Some("Public Nexus Mod".to_string()),
                    display_name: Some("Public Nexus Mod".to_string()),
                    version: Some("2.0".to_string()),
                    nexus_mod_id: Some(123),
                    nexus_file_id: Some(456),
                    nexus_game_domain: Some("skyrimspecialedition".to_string()),
                    installed_timestamp: None,
                    plugin_name: None,
                    plugin_load_index: None,
                    evidence: Vec::new(),
                    confidence: CrashConfidence::High,
                    summary: String::new(),
                },
            ],
        };

        let batch = compatibility_batch_from_report(&report, &Default::default())
            .expect("compatibility batch");
        let json = serde_json::to_string(&batch).expect("json");
        assert!(!json.contains("Secret"));
        assert!(!json.contains("private-profile"));
        assert!(!json.contains("/home/user"));
        assert!(!json.contains("Public Nexus Mod"));
        assert!(batch.validate().is_ok());
    }
}
