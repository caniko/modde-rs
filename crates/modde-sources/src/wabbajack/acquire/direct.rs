use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures::StreamExt;
use modde_core::manifest::wabbajack::WabbajackManifest;
use reqwest::{Client, Url};
use tokio::io::AsyncWriteExt;
use xxhash_rust::xxh64::{Xxh64, xxh64};

use super::{
    AcquireResult, AcquireStatus, DirectAcquireOutcome, MissingArchive, import_acquired_archive,
};

pub async fn try_acquire_manual_direct(
    manifest: &WabbajackManifest,
    store_dir: &Path,
    download_dir: &Path,
    archive: &MissingArchive,
) -> Result<DirectAcquireOutcome> {
    let Some(url) = archive.url.as_deref() else {
        return Ok(DirectAcquireOutcome::Unsupported);
    };
    let parsed = Url::parse(url).with_context(|| format!("invalid manual URL: {url}"))?;
    let Some(host) = parsed.host_str().map(str::to_ascii_lowercase) else {
        return Ok(DirectAcquireOutcome::Unsupported);
    };

    if host == "loverslab.com" || host == "www.loverslab.com" {
        return resolve_loverslab_manual(&host, archive).await;
    }

    if !matches!(
        host.as_str(),
        "workupload.com" | "www.workupload.com" | "sharemods.com" | "www.sharemods.com"
    ) {
        return Ok(DirectAcquireOutcome::Unsupported);
    }

    tokio::fs::create_dir_all(download_dir)
        .await
        .with_context(|| format!("failed to create {}", download_dir.display()))?;
    let client = Client::builder()
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) modde/manual-acquire")
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .context("failed to build manual acquisition HTTP client")?;
    let final_path = download_dir
        .join(modde_core::fs::safe_file_name(&archive.name).unwrap_or("archive.download"));

    match resolve_manual_http_flow(&client, parsed, archive, &final_path).await? {
        Some((path, computed)) => {
            let mut result = import_acquired_archive(manifest, store_dir, archive, &path).await?;
            if matches!(
                result.status,
                AcquireStatus::Imported | AcquireStatus::AlreadyPresent
            ) {
                result.status = AcquireStatus::DirectResolved;
                result.computed_xxh64 = Some(computed);
                result.message = Some("direct HTTP flow resolved and verified".into());
            }
            Ok(DirectAcquireOutcome::Resolved(result))
        }
        None => Ok(DirectAcquireOutcome::NeedsBrowser {
            archive: archive.clone(),
            message: "direct HTTP flow did not expose a matching archive".into(),
        }),
    }
}

pub(in crate::wabbajack::acquire) async fn resolve_loverslab_manual(
    host: &str,
    archive: &MissingArchive,
) -> Result<DirectAcquireOutcome> {
    if tokio::net::lookup_host((host, 443)).await.is_err() {
        return Ok(DirectAcquireOutcome::Final(AcquireResult {
            archive: archive.clone(),
            status: AcquireStatus::DnsUnresolved,
            path: None,
            computed_xxh64: None,
            message: Some(format!(
                "{host} did not resolve; fix DNS/network access before acquiring this archive"
            )),
        }));
    }
    Ok(DirectAcquireOutcome::NeedsBrowser {
        archive: archive.clone(),
        message: "LoversLab downloads require a live browser login/session; stale manifest CSRF URLs are not trusted".into(),
    })
}

pub(in crate::wabbajack::acquire) async fn resolve_manual_http_flow(
    client: &Client,
    start_url: Url,
    archive: &MissingArchive,
    final_path: &Path,
) -> Result<Option<(PathBuf, u64)>> {
    let mut pages = vec![start_url];
    let mut seen = HashSet::new();

    for _ in 0..4 {
        let Some(url) = pages.pop() else {
            break;
        };
        if !seen.insert(url.clone()) {
            continue;
        }

        let html = client
            .get(url.clone())
            .send()
            .await
            .with_context(|| format!("failed to fetch {url}"))?
            .error_for_status()
            .with_context(|| format!("{url} returned an HTTP error"))?
            .text()
            .await
            .with_context(|| format!("failed to read {url}"))?;

        for action in extract_manual_actions(&html, &url, archive)? {
            match action.method {
                ManualActionMethod::Get => {
                    if let Some(download) =
                        try_direct_download(client, action.url, final_path, archive.hash).await?
                    {
                        return Ok(Some(download));
                    }
                }
                ManualActionMethod::Post(fields) => {
                    let response = client
                        .post(action.url.clone())
                        .form(&fields)
                        .send()
                        .await
                        .with_context(|| format!("failed to submit {}", action.url))?
                        .error_for_status()
                        .with_context(|| format!("{} returned an HTTP error", action.url))?;
                    let body = response.bytes().await.with_context(|| {
                        format!("failed to read form response from {}", action.url)
                    })?;
                    if xxh64(&body, 0) == archive.hash
                        && let Some(download) =
                            write_verified_bytes(&body, final_path, archive.hash).await?
                    {
                        return Ok(Some(download));
                    }
                    if response_looks_like_html_bytes(&body) {
                        let html = String::from_utf8_lossy(&body);
                        for nested in extract_manual_actions(&html, &action.url, archive)? {
                            match nested.method {
                                ManualActionMethod::Get => {
                                    if let Some(download) = try_direct_download(
                                        client,
                                        nested.url,
                                        final_path,
                                        archive.hash,
                                    )
                                    .await?
                                    {
                                        return Ok(Some(download));
                                    }
                                }
                                ManualActionMethod::Post(_) => {
                                    pages.push(nested.url);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(None)
}

#[derive(Debug, Clone)]
pub(in crate::wabbajack::acquire) struct ManualAction {
    pub(in crate::wabbajack::acquire) url: Url,
    pub(in crate::wabbajack::acquire) method: ManualActionMethod,
}

#[derive(Debug, Clone)]
pub(in crate::wabbajack::acquire) enum ManualActionMethod {
    Get,
    Post(Vec<(String, String)>),
}

pub(in crate::wabbajack::acquire) fn extract_manual_actions(
    html: &str,
    base_url: &Url,
    archive: &MissingArchive,
) -> Result<Vec<ManualAction>> {
    let mut actions = Vec::new();
    actions.extend(
        extract_page_links(html, base_url, archive)?
            .into_iter()
            .map(|url| ManualAction {
                url,
                method: ManualActionMethod::Get,
            }),
    );

    let mut rest = html;
    while let Some(start) = rest.find("<form") {
        let fragment = &rest[start..];
        let Some(open_end) = fragment.find('>') else {
            break;
        };
        let tag = &fragment[..=open_end];
        let Some(close) = fragment.find("</form>") else {
            rest = &fragment[open_end + 1..];
            continue;
        };
        let form = &fragment[..close + "</form>".len()];
        rest = &fragment[close + "</form>".len()..];
        if !fragment_looks_download_related(form, archive) {
            continue;
        }
        let action = attr_value(tag, "action").unwrap_or_else(|| base_url.as_str().to_string());
        let url = absolutize_url(base_url, &action)?;
        let method = attr_value(tag, "method").unwrap_or_else(|| "get".into());
        if method.eq_ignore_ascii_case("post") {
            actions.push(ManualAction {
                url,
                method: ManualActionMethod::Post(extract_form_fields(form)),
            });
        } else {
            actions.push(ManualAction {
                url,
                method: ManualActionMethod::Get,
            });
        }
    }

    Ok(actions)
}

pub(in crate::wabbajack::acquire) fn extract_page_links(
    html: &str,
    base_url: &Url,
    archive: &MissingArchive,
) -> Result<Vec<Url>> {
    let mut urls = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find("<a") {
        let fragment = &rest[start..];
        let Some(end) = fragment.find('>') else {
            break;
        };
        let tag = &fragment[..=end];
        let link_fragment = fragment
            .find("</a>")
            .map_or(tag, |close| &fragment[..close + "</a>".len()]);
        rest = &fragment[end + 1..];
        if !fragment_looks_download_related(link_fragment, archive) {
            continue;
        }
        let Some(href) = attr_value(tag, "href") else {
            continue;
        };
        if href.starts_with('#')
            || href.starts_with("javascript:")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
        {
            continue;
        }
        urls.push(absolutize_url(base_url, &href)?);
    }
    Ok(urls)
}

pub(in crate::wabbajack::acquire) fn fragment_looks_download_related(
    fragment: &str,
    archive: &MissingArchive,
) -> bool {
    let lower = fragment.to_ascii_lowercase();
    lower.contains("download")
        || lower.contains("create download link")
        || lower.contains("start download")
        || lower.contains(&archive.name.to_ascii_lowercase())
        || lower.contains(".7z")
        || lower.contains(".zip")
        || lower.contains(".rar")
}

pub(in crate::wabbajack::acquire) fn extract_form_fields(form: &str) -> Vec<(String, String)> {
    let mut fields = Vec::new();
    let mut rest = form;
    while let Some(start) = rest.find("<input") {
        let fragment = &rest[start..];
        let Some(end) = fragment.find('>') else {
            break;
        };
        let tag = &fragment[..=end];
        rest = &fragment[end + 1..];
        let Some(name) = attr_value(tag, "name") else {
            continue;
        };
        let value = attr_value(tag, "value").unwrap_or_default();
        fields.push((name, value));
    }
    fields
}

pub(in crate::wabbajack::acquire) async fn try_direct_download(
    client: &Client,
    url: Url,
    final_path: &Path,
    expected_hash: u64,
) -> Result<Option<(PathBuf, u64)>> {
    let response = client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("failed to fetch candidate download {url}"))?
        .error_for_status()
        .with_context(|| format!("candidate download {url} returned an HTTP error"))?;
    if response_looks_like_html(&response) {
        return Ok(None);
    }
    stream_response_to_verified_path(response, final_path, expected_hash).await
}

pub(in crate::wabbajack::acquire) async fn stream_response_to_verified_path(
    response: reqwest::Response,
    final_path: &Path,
    expected_hash: u64,
) -> Result<Option<(PathBuf, u64)>> {
    let Some(parent) = final_path.parent() else {
        anyhow::bail!("download path has no parent: {}", final_path.display());
    };
    tokio::fs::create_dir_all(parent).await?;
    let temp_path = final_path.with_extension("modde-direct-part");
    let mut file = tokio::fs::File::create(&temp_path)
        .await
        .with_context(|| format!("failed to create {}", temp_path.display()))?;
    let mut hasher = Xxh64::new(0);
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read direct download chunk")?;
        hasher.update(&chunk);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);

    let computed = hasher.digest();
    if computed != expected_hash {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Ok(None);
    }
    if final_path.exists() {
        tokio::fs::remove_file(final_path)
            .await
            .with_context(|| format!("failed to replace {}", final_path.display()))?;
    }
    tokio::fs::rename(&temp_path, final_path)
        .await
        .with_context(|| {
            format!(
                "failed to move {} to {}",
                temp_path.display(),
                final_path.display()
            )
        })?;
    Ok(Some((final_path.to_path_buf(), computed)))
}

pub(in crate::wabbajack::acquire) async fn write_verified_bytes(
    bytes: &[u8],
    final_path: &Path,
    expected_hash: u64,
) -> Result<Option<(PathBuf, u64)>> {
    let computed = xxh64(bytes, 0);
    if computed != expected_hash {
        return Ok(None);
    }
    if let Some(parent) = final_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(final_path, bytes)
        .await
        .with_context(|| format!("failed to write {}", final_path.display()))?;
    Ok(Some((final_path.to_path_buf(), computed)))
}

pub(in crate::wabbajack::acquire) fn response_looks_like_html(
    response: &reqwest::Response,
) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("text/html"))
}

pub(in crate::wabbajack::acquire) fn response_looks_like_html_bytes(bytes: &[u8]) -> bool {
    let prefix_len = bytes.len().min(1024);
    let prefix = String::from_utf8_lossy(&bytes[..prefix_len]).to_ascii_lowercase();
    prefix.contains("<html") || prefix.contains("<form") || prefix.contains("<a ")
}

pub(in crate::wabbajack::acquire) fn absolutize_url(base: &Url, href: &str) -> Result<Url> {
    base.join(href)
        .with_context(|| format!("invalid manual download href {href}"))
}

pub(in crate::wabbajack::acquire) fn attr_value(tag: &str, attr: &str) -> Option<String> {
    let mut rest = tag;
    loop {
        let idx = rest.find(attr)?;
        let before = rest[..idx].chars().next_back();
        let after = rest[idx + attr.len()..].chars().next();
        if before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            || after.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '-')
        {
            rest = &rest[idx + attr.len()..];
            continue;
        }

        let mut value = rest[idx + attr.len()..].trim_start();
        if !value.starts_with('=') {
            rest = &rest[idx + attr.len()..];
            continue;
        }
        value = value[1..].trim_start();
        let quote = value.chars().next()?;
        if quote == '"' || quote == '\'' {
            let value = &value[quote.len_utf8()..];
            let end = value.find(quote)?;
            return Some(value[..end].to_string());
        }
        let end = value
            .find(|ch: char| ch.is_whitespace() || ch == '>')
            .unwrap_or(value.len());
        return Some(value[..end].to_string());
    }
}
