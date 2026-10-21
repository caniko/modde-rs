#![allow(clippy::wildcard_imports)]
//! Archive, Nexus collection, and installer utility helpers.

use super::*;

pub(super) fn build_http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_mins(5))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")
}

pub(super) async fn fetch_collection(
    client: &reqwest::Client,
    api_key: &str,
    slug: &str,
    version: Option<&str>,
) -> Result<CollectionManifest> {
    let api = NexusApi::new(client.clone(), api_key.to_string());

    let parsed_version = version.and_then(|v| v.parse::<u64>().ok());

    api.get_collection_by_slug(slug, parsed_version)
        .await
        .with_context(|| format!("failed to fetch collection '{slug}' from Nexus API"))
}

/// Download a file from a URL to a destination path.
pub(super) async fn download_file(client: &reqwest::Client, url: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let resp = modde_sources::error::status_error(
        client
            .get(url)
            .send()
            .await
            .context("download request failed")?,
    )
    .context("download returned error status")?;

    let bytes = resp.bytes().await.context("failed to read download body")?;
    tokio::fs::write(dest, &bytes)
        .await
        .context("failed to write downloaded file")?;

    Ok(())
}

/// Extract a zip archive to a destination directory.
pub(super) fn extract_archive(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive: {}", archive_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("failed to read zip archive: {}", archive_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(name) = entry.enclosed_name() else {
            warn!("skipping archive entry with unsafe path");
            continue;
        };
        let out_path = dest.join(name);

        if entry.is_dir() {
            std::fs::create_dir_all(&out_path)?;
        } else {
            if let Some(parent) = out_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }

    Ok(())
}

/// Find the FOMOD ModuleConfig.xml path in a mod directory (case-insensitive).
pub fn find_fomod_config(mod_dir: &Path) -> Option<std::path::PathBuf> {
    let config_path = mod_dir.join("fomod").join("ModuleConfig.xml");
    if config_path.exists() {
        return Some(config_path);
    }
    // Case-insensitive fallback
    let Ok(entries) = std::fs::read_dir(mod_dir) else {
        return None;
    };
    for entry in entries.flatten() {
        if entry.file_name().eq_ignore_ascii_case("fomod") && entry.path().is_dir() {
            let Ok(inner) = std::fs::read_dir(entry.path()) else {
                continue;
            };
            for inner_entry in inner.flatten() {
                if inner_entry
                    .file_name()
                    .eq_ignore_ascii_case("moduleconfig.xml")
                {
                    return Some(inner_entry.path());
                }
            }
        }
    }
    None
}

/// Parse a Nexus mod URL to extract `game_domain`, `mod_id`, and optional `file_id`.
///
/// Supports URLs like:
/// - `https://www.nexusmods.com/skyrimspecialedition/mods/12345`
/// - `https://www.nexusmods.com/skyrimspecialedition/mods/12345?tab=files&file_id=67890`
pub(super) fn parse_nexus_url(url: &str) -> Result<(String, NexusModId, Option<NexusFileId>)> {
    // Extract path segments: /GAME/mods/MOD_ID
    let url_parsed = url::Url::parse(url).context("invalid URL")?;

    let segments: Vec<&str> = url_parsed
        .path_segments()
        .map(std::iter::Iterator::collect)
        .unwrap_or_default();

    let [game_domain, "mods", mod_id, rest @ ..] = segments.as_slice() else {
        bail!("URL does not look like a Nexus mod URL: {url}");
    };
    if game_domain.is_empty() || mod_id.is_empty() || !rest.iter().all(|segment| segment.is_empty())
    {
        bail!("URL does not look like a Nexus mod URL: {url}");
    }

    modde_core::nexus_id::validate_game_domain(game_domain)?;
    let game_domain = (*game_domain).to_string();
    let mod_id = mod_id
        .parse::<u64>()
        .map(NexusModId::from)
        .with_context(|| format!("invalid mod ID in URL: {mod_id}"))?;

    // Check for file_id in query params
    let file_id = url_parsed
        .query_pairs()
        .find(|(k, _)| k == "file_id")
        .and_then(|(_, v)| v.parse::<u64>().ok().map(NexusFileId::from));

    Ok((game_domain, mod_id, file_id))
}
