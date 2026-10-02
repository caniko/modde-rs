//! HTML mirror-listing resolution: fetch a mirror index page and extract the
//! candidate download URLs, preferring the least-loaded mirror.

use anyhow::{Context, Result, bail};
use modde_core::manifest::wabbajack::HtmlMirrorResolver;
use reqwest::{Client, Url};

#[derive(Debug, Clone)]
struct MirrorCandidate {
    url: String,
    load: Option<f64>,
    order: usize,
}

/// Fetch the mirror listing described by `resolver` and return the resolved
/// mirror URLs, ordered best-first.
///
/// # Errors
///
/// Returns an error if the listing cannot be fetched or parsed, or if it yields
/// no mirrors.
pub async fn resolve_html_mirrors(
    client: &Client,
    resolver: &HtmlMirrorResolver,
) -> Result<Vec<String>> {
    let mut req = client.get(&resolver.listing_url);
    if let Some(user_agent) = &resolver.user_agent {
        req = req.header("User-Agent", user_agent);
    }

    let html = req
        .send()
        .await
        .with_context(|| {
            format!(
                "{} failed to fetch mirror listing for {}",
                resolver.name, resolver.original_url
            )
        })?
        .error_for_status()
        .with_context(|| {
            format!(
                "{} mirror listing returned an error for {}",
                resolver.name, resolver.original_url
            )
        })?
        .text()
        .await
        .context("failed to read mirror listing body")?;

    let mirrors = extract_html_mirror_links(&html, &resolver.listing_url, &resolver.link_id)?;
    if mirrors.is_empty() {
        bail!(
            "{} found no mirrors for {} via {}",
            resolver.name,
            resolver.original_url,
            resolver.listing_url
        );
    }

    Ok(mirrors)
}

/// Parse `html` for mirror links matching `link_id`, resolving them against
/// `base_url` and ordering them best-first by reported load.
///
/// # Errors
///
/// Returns an error if `base_url` is not a valid URL.
pub fn extract_html_mirror_links(html: &str, base_url: &str, link_id: &str) -> Result<Vec<String>> {
    let base =
        Url::parse(base_url).with_context(|| format!("invalid mirror base URL {base_url}"))?;
    let mut candidates = Vec::new();

    for (order, row) in html.split("<div class=\"row\"").enumerate() {
        if let Some(href) = link_with_id(row, link_id) {
            candidates.push(MirrorCandidate {
                url: absolutize(&base, &href)?,
                load: parse_percent_load(row),
                order,
            });
        }
    }

    if candidates.is_empty() {
        for (order, tag) in html.match_indices("<a").map(|(idx, _)| idx).enumerate() {
            let Some(end) = html[tag..].find('>') else {
                continue;
            };
            let anchor = &html[tag..=(tag + end)];
            if let Some(href) = link_with_id(anchor, link_id) {
                candidates.push(MirrorCandidate {
                    url: absolutize(&base, &href)?,
                    load: None,
                    order,
                });
            }
        }
    }

    candidates.sort_by(|a, b| {
        a.load
            .partial_cmp(&b.load)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.order.cmp(&b.order))
    });
    candidates.dedup_by(|a, b| a.url == b.url);

    Ok(candidates
        .into_iter()
        .map(|candidate| candidate.url)
        .collect())
}

fn link_with_id(fragment: &str, link_id: &str) -> Option<String> {
    for anchor_start in fragment.match_indices("<a").map(|(idx, _)| idx) {
        let end = fragment[anchor_start..].find('>')?;
        let anchor = &fragment[anchor_start..=(anchor_start + end)];
        if attr_value(anchor, "id").as_deref() == Some(link_id) {
            return attr_value(anchor, "href");
        }
    }
    None
}

fn attr_value(tag: &str, attr: &str) -> Option<String> {
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

fn absolutize(base: &Url, href: &str) -> Result<String> {
    Ok(base
        .join(href)
        .with_context(|| format!("invalid mirror href {href}"))?
        .to_string())
}

fn parse_percent_load(row: &str) -> Option<f64> {
    let percent = row.find('%')?;
    let before = &row[..percent];
    let start = before
        .rfind(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .map_or(0, |idx| idx + 1);
    before[start..].parse::<f64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_downloadon_links_sorted_by_capacity() {
        let html = r#"
            <div class="row">
              <a href="/downloads/mirror/1/slow/token" id="downloadon">Start</a>
              <span class="subheading">2 downloads served, 90.0% capacity</span>
            </div>
            <div class="row">
              <a id="downloadon" href="/downloads/mirror/1/fast/token">Start</a>
              <span class="subheading">1 download served, 12.5% capacity</span>
            </div>
        "#;

        let links = extract_html_mirror_links(
            html,
            "https://www.moddb.com/downloads/start/1/all",
            "downloadon",
        )
        .unwrap();
        assert_eq!(
            links,
            vec![
                "https://www.moddb.com/downloads/mirror/1/fast/token",
                "https://www.moddb.com/downloads/mirror/1/slow/token",
            ]
        );
    }

    #[test]
    fn extracts_absolute_links_without_rows() {
        let html = r#"<a id="downloadon" href="https://cdn.example.test/file.7z">Start</a>"#;
        let links = extract_html_mirror_links(
            html,
            "https://www.moddb.com/downloads/start/1/all",
            "downloadon",
        )
        .unwrap();
        assert_eq!(links, vec!["https://cdn.example.test/file.7z"]);
    }

    #[test]
    fn challenge_page_has_no_mirrors() {
        let html = r"<html><head><title>Just a moment...</title></head></html>";
        let links = extract_html_mirror_links(
            html,
            "https://www.moddb.com/downloads/start/1/all",
            "downloadon",
        )
        .unwrap();
        assert_eq!(links, [] as [std::string::String; 0]);
    }
}
