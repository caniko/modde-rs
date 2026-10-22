use std::io::Write;

use anyhow::{Context, Result};
use tracing::info;

use modde_sources::SourceError;
use modde_sources::nexus::auth;

use crate::NexusAction;

pub async fn handle(action: NexusAction) -> Result<()> {
    match action {
        NexusAction::Auth => handle_auth().await,
        NexusAction::Status => handle_status().await,
    }
}

/// Build a reqwest client with Nexus-appropriate timeouts.
fn nexus_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}

/// Map Nexus API errors to user-friendly messages with actionable advice.
fn map_nexus_error(e: anyhow::Error, rejected_advice: &str) -> anyhow::Error {
    if matches!(
        e.downcast_ref::<SourceError>(),
        Some(SourceError::Unauthorized { .. })
    ) {
        return anyhow::anyhow!(
            "API key rejected by Nexus (HTTP 401/403). {rejected_advice}\n\
             Cause: {e}"
        );
    }
    if matches!(
        e.downcast_ref::<SourceError>(),
        Some(SourceError::RateLimited { .. })
    ) {
        return anyhow::anyhow!(
            "Nexus API rate limit exceeded (HTTP 429). Wait a few minutes and try again.\n\
             Cause: {e}"
        );
    }

    // Other statuses arrive untyped; their text includes the request URL, so
    // matching "401" or "429" in it would misread ports and ids as statuses.
    anyhow::anyhow!("API key validation failed: {e}")
}

async fn handle_auth() -> Result<()> {
    let (api_key, source) = read_auth_api_key()?;

    let client = nexus_client()?;
    let is_premium = auth::check_premium(&client, &api_key).await.map_err(|e| {
        map_nexus_error(
            e,
            "Verify your key at https://www.nexusmods.com/users/myaccount?tab=api+access",
        )
    })?;

    auth::write_config_api_key(&api_key).context("failed to write API key to config")?;

    let key_path = auth::config_api_key_path();
    info!(key_path = %key_path.display(), "API key stored");

    if source == AuthKeySource::Environment {
        println!("Loaded Nexus API key from NEXUS_API_KEY.");
    }
    println!("API key validated and saved to {}", key_path.display());
    if is_premium {
        println!("Account type: Premium (automated downloads enabled)");
    } else {
        println!("Account type: Free (manual download links only)");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKeySource {
    Environment,
    Prompt,
}

fn read_auth_api_key() -> Result<(String, AuthKeySource)> {
    if let Ok(api_key) = std::env::var("NEXUS_API_KEY") {
        return select_auth_api_key(Some(api_key), None);
    }

    print!("Enter your Nexus API key: ");
    std::io::stdout().flush()?;

    let mut api_key = String::new();
    std::io::stdin()
        .read_line(&mut api_key)
        .context("failed to read API key from stdin")?;
    select_auth_api_key(None, Some(api_key))
}

fn select_auth_api_key(
    env_key: Option<String>,
    prompted_key: Option<String>,
) -> Result<(String, AuthKeySource)> {
    if let Some(key) = env_key {
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Ok((key, AuthKeySource::Environment));
        }
    }

    if let Some(key) = prompted_key {
        let key = key.trim().to_string();
        if !key.is_empty() {
            return Ok((key, AuthKeySource::Prompt));
        }
    }

    anyhow::bail!("API key cannot be empty");
}

async fn handle_status() -> Result<()> {
    let api_key =
        auth::load_api_key().context("no API key configured; run `modde nexus auth` first")?;

    let client = nexus_client()?;
    let is_premium = auth::check_premium(&client, &api_key).await.map_err(|e| {
        map_nexus_error(
            e,
            "Your stored key may have been revoked. Re-run `modde nexus auth` to set a new key.",
        )
    })?;

    println!("Nexus API key: valid");
    if is_premium {
        println!("Account type: Premium");
    } else {
        println!("Account type: Free");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AuthKeySource, SourceError, map_nexus_error, select_auth_api_key};

    #[test]
    fn auth_prefers_non_empty_env_key() {
        let (key, source) =
            select_auth_api_key(Some(" env-key \n".into()), Some("prompt-key".into())).unwrap();
        assert_eq!(key, "env-key");
        assert_eq!(source, AuthKeySource::Environment);
    }

    #[test]
    fn auth_falls_back_to_prompt_when_env_key_is_empty() {
        let (key, source) =
            select_auth_api_key(Some(" \n".into()), Some(" prompt-key ".into())).unwrap();
        assert_eq!(key, "prompt-key");
        assert_eq!(source, AuthKeySource::Prompt);
    }

    #[test]
    fn auth_rejects_empty_keys() {
        let err = select_auth_api_key(Some(String::new()), Some(" \n".into())).unwrap_err();
        assert!(format!("{err:#}").contains("API key cannot be empty"));
    }

    #[test]
    fn only_typed_statuses_are_reported_as_rejected_or_rate_limited() {
        let message = |error: anyhow::Error| format!("{:#}", map_nexus_error(error, "advice"));
        let url = "http://127.0.0.1:44291/users/validate.json".to_string();

        // A 500 from a server on port 44291 is neither a 401 nor a 429.
        let server_error = message(anyhow::anyhow!(
            "HTTP status server error (500) for url ({url})"
        ));
        assert!(
            server_error.starts_with("API key validation failed"),
            "{server_error}"
        );
        let limited = message(
            SourceError::RateLimited {
                url: url.clone(),
                retry_after: None,
            }
            .into(),
        );
        assert!(
            limited.contains("rate limit exceeded (HTTP 429)"),
            "{limited}"
        );
        let rejected = message(SourceError::Unauthorized { url }.into());
        assert!(
            rejected.contains("API key rejected by Nexus (HTTP 401/403). advice"),
            "{rejected}"
        );
    }
}
