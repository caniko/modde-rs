use std::env;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use semver::Version;
use serde::{Deserialize, Serialize};
use tokio::fs;

use crate::settings::AppSettings;

const DEFAULT_RELEASE_URL: &str =
    "https://codeberg.org/api/v1/repos/caniko/rs-modde/releases/latest";
const CACHE_TTL: Duration = Duration::from_hours(24);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ReleaseResponse {
    tag_name: String,
    #[serde(default)]
    html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateCheckCache {
    checked_at: u64,
    latest_version: String,
    release_url: String,
}

#[must_use]
pub fn update_checks_enabled(settings: &AppSettings) -> bool {
    if env::var("MODDE_NO_UPDATE_CHECK").is_ok_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    }) {
        return false;
    }
    settings.update_check.enabled
}

pub async fn check_latest() -> Result<Option<UpdateInfo>> {
    check_latest_with_settings(&AppSettings::load()).await
}

pub async fn check_latest_with_settings(settings: &AppSettings) -> Result<Option<UpdateInfo>> {
    if !update_checks_enabled(settings) {
        return Ok(None);
    }

    if let Some(cache) = read_fresh_cache().await? {
        return compare_cached(cache);
    }

    let latest = fetch_latest_release().await?;
    write_cache(&latest).await?;
    compare_cached(latest)
}

pub async fn check_latest_uncached(settings: &AppSettings) -> Result<Option<UpdateInfo>> {
    if !update_checks_enabled(settings) {
        return Ok(None);
    }
    let latest = fetch_latest_release().await?;
    write_cache(&latest).await?;
    compare_cached(latest)
}

fn compare_cached(cache: UpdateCheckCache) -> Result<Option<UpdateInfo>> {
    let current = Version::parse(env!("CARGO_PKG_VERSION")).context("invalid compiled version")?;
    let latest = Version::parse(cache.latest_version.trim_start_matches('v'))
        .with_context(|| format!("invalid latest release version '{}'", cache.latest_version))?;

    if latest > current {
        Ok(Some(UpdateInfo {
            current_version: current.to_string(),
            latest_version: latest.to_string(),
            release_url: cache.release_url,
        }))
    } else {
        Ok(None)
    }
}

async fn fetch_latest_release() -> Result<UpdateCheckCache> {
    let endpoint =
        env::var("MODDE_UPDATE_CHECK_URL").unwrap_or_else(|_| DEFAULT_RELEASE_URL.into());
    let client = reqwest::Client::builder()
        .user_agent(concat!("modde/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build update-check HTTP client")?;

    let release = client
        .get(endpoint)
        .send()
        .await
        .context("failed to query latest modde release")?
        .error_for_status()
        .context("latest modde release endpoint returned an error")?
        .json::<ReleaseResponse>()
        .await
        .context("failed to parse latest modde release response")?;

    let fallback_url = format!(
        "https://github.com/caniko/rs-modde/releases/tag/{}",
        release.tag_name
    );

    Ok(UpdateCheckCache {
        checked_at: now_unix_secs()?,
        latest_version: release.tag_name,
        release_url: release.html_url.unwrap_or(fallback_url),
    })
}

async fn read_fresh_cache() -> Result<Option<UpdateCheckCache>> {
    let path = cache_path();
    let Ok(bytes) = fs::read(&path).await else {
        return Ok(None);
    };
    let cache: UpdateCheckCache = serde_json::from_slice(&bytes)
        .with_context(|| format!("failed to parse update-check cache at {}", path.display()))?;
    let now = now_unix_secs()?;
    if now.saturating_sub(cache.checked_at) <= CACHE_TTL.as_secs() {
        Ok(Some(cache))
    } else {
        Ok(None)
    }
}

async fn write_cache(cache: &UpdateCheckCache) -> Result<()> {
    let path = cache_path();
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("update-check cache path has no parent"))?;
    fs::create_dir_all(parent)
        .await
        .with_context(|| format!("failed to create cache directory {}", parent.display()))?;

    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(cache).context("failed to serialize update cache")?;
    fs::write(&tmp, bytes)
        .await
        .with_context(|| format!("failed to write temporary cache {}", tmp.display()))?;
    fs::rename(&tmp, &path)
        .await
        .with_context(|| format!("failed to replace update cache {}", path.display()))?;
    Ok(())
}

fn cache_path() -> PathBuf {
    crate::paths::modde_cache_dir().join("update-check.json")
}

fn now_unix_secs() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn env_opt_out_accepts_truthy_values() {
        let _guard = ENV_LOCK.lock().unwrap();
        let mut settings = AppSettings::default();
        settings.update_check.enabled = true;
        // SAFETY: update-check env mutation is serialized by ENV_LOCK and is
        // restored before the test releases the lock.
        unsafe {
            env::set_var("MODDE_NO_UPDATE_CHECK", "1");
        }
        assert!(!update_checks_enabled(&settings));
        // SAFETY: guarded by ENV_LOCK; removes the variable set just above.
        unsafe {
            env::remove_var("MODDE_NO_UPDATE_CHECK");
        }
    }

    #[test]
    fn config_opt_out_disables_checks() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: update-check env mutation is serialized by ENV_LOCK for all
        // tests in this module.
        unsafe {
            env::remove_var("MODDE_NO_UPDATE_CHECK");
        }
        let mut settings = AppSettings::default();
        settings.update_check.enabled = false;
        assert!(!update_checks_enabled(&settings));
    }
}
