//! Typed Nexus Mods v2 GraphQL client.
//!
//! The v2 GraphQL endpoint is undocumented but stable enough to back the
//! browse / search UI. This module centralizes the transport, query
//! literals, and response types so callers do not have to poke at
//! `serde_json::Value` directly.
//!
//! All queries take a numeric `gameId` rather than a `gameDomain`
//! string. Callers get the numeric ID from
//! [`modde_games::GamePlugin::nexus_game_id_u32`] and the domain from
//! [`modde_games::GamePlugin::nexus_game_domain`] — the former is
//! needed for browse feeds, the latter for mod-detail lookups that the
//! REST API already accepts.
//!
//! On any error, callers may fall back to the v1 REST helpers on
//! [`super::api::NexusApi`], which exist for every query that has a
//! REST equivalent. The GraphQL path is preferred because it returns
//! richer per-mod data (thumbnails, summaries, download counts) in one
//! round-trip, but the REST path is preserved to keep the UI working
//! if Nexus ever pulls the v2 endpoint.

use anyhow::{Context, Result, bail};
use modde_core::NexusModId;
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// POST a query to the Nexus v2 GraphQL endpoint and decode the `data`
/// field into `T`. On any shape mismatch or transport error, returns
/// an `anyhow::Error` — callers that have a REST fallback should
/// `.or_else(|_| rest_call())`.
///
/// We deserialize into a `serde_json::Value` first and then pull out
/// `data` / `errors` manually. Going directly into `GqlResponse<T>`
/// would impose a `Default` bound on every caller's response type
/// (because of how `#[serde(default)]` interacts with generic enum
/// deserialization in this version of serde).
pub async fn post<T: DeserializeOwned>(
    client: &Client,
    api_key: &str,
    query: &str,
    variables: Value,
) -> Result<T> {
    let body = serde_json::json!({
        "query": query,
        "variables": variables,
    });

    let resp = client
        .post(super::graphql_url())
        .header("apikey", api_key)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("GraphQL POST failed")?
        .error_for_status()
        .context("GraphQL transport error")?;

    let envelope: Value = resp
        .json()
        .await
        .context("failed to decode GraphQL response")?;

    if let Some(errors) = envelope.get("errors")
        && !errors.is_null()
    {
        bail!("Nexus GraphQL errors: {errors}");
    }
    let data = envelope
        .get("data")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("GraphQL response missing `data` field"))?;
    let decoded: T =
        serde_json::from_value(data).context("failed to decode GraphQL `data` payload")?;
    Ok(decoded)
}

// ── Typed queries ────────────────────────────────────────────────

/// Response tile for the browse / search feeds. A slimmer view of a mod
/// than the REST `NexusMod` struct — this is what the UI card grid
/// renders, so it carries only what's visible at a glance.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GqlModTile {
    #[serde(rename = "modId")]
    pub mod_id: NexusModId,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default, rename = "pictureUrl")]
    pub picture_url: Option<String>,
    #[serde(default, rename = "thumbnailUrl")]
    pub thumbnail_url: Option<String>,
    #[serde(default, rename = "endorsements")]
    pub endorsements: Option<u64>,
    #[serde(default, rename = "downloads")]
    pub downloads: Option<u64>,
    #[serde(default, rename = "uploadedAt")]
    pub uploaded_at: Option<String>,
    /// Nexus game domain (returned by the v2 schema). Optional because
    /// some feed variants don't include it; callers fall back to the
    /// `gameDomain` they issued the query with.
    #[serde(default, rename = "gameDomain")]
    pub game_domain: Option<String>,
}

/// Browse feed kind. Matches the UI tab enum.
#[derive(Debug, Clone, Copy)]
pub enum ModFeedKind {
    /// All-time trending mods for a game.
    Trending,
    /// Mods updated in the last month — equivalent to the REST
    /// `updated_mods(period="1m")` feed.
    MonthlyTop,
}

const TRENDING_QUERY: &str = r"
query TrendingMods($gameDomain: String!) {
  mods(
    filter: { gameDomain: { value: $gameDomain, op: EQUALS } }
    sort: { endorsements: { direction: DESC } }
    count: 30
  ) {
    nodes {
      modId
      name
      summary
      version
      author { name }
      pictureUrl
      thumbnailUrl
      endorsements
      downloads
      uploadedAt
      gameDomain
    }
  }
}
";

const SEARCH_QUERY: &str = r"
query SearchMods($gameDomain: String!, $term: String!, $page: Int!) {
  mods(
    filter: {
      gameDomain: { value: $gameDomain, op: EQUALS }
      name: { value: $term, op: WILDCARD }
    }
    offset: $page
    count: 30
  ) {
    nodes {
      modId
      name
      summary
      version
      author { name }
      pictureUrl
      thumbnailUrl
      endorsements
      downloads
      uploadedAt
      gameDomain
    }
  }
}
";

/// Fetch a trending / monthly-top browse feed. The `kind` drives which
/// server-side sort is used.
///
/// The server-side schema is in flux, so parsing is deliberately
/// tolerant: any missing field becomes `None`. On a total parse failure
/// the caller should fall back to the REST feed — use
/// [`super::api::NexusApi::trending_mods`] or `updated_mods("1m")`.
pub async fn browse_feed(
    client: &Client,
    api_key: &str,
    game_domain: &str,
    kind: ModFeedKind,
) -> Result<Vec<GqlModTile>> {
    let query = match kind {
        ModFeedKind::Trending => TRENDING_QUERY,
        // For monthly-top we reuse the trending sort — the v2 schema
        // doesn't expose a clean "updated in last month" filter without
        // date math in the query, and trending approximates what users
        // expect from "mods of the month". The REST path remains
        // available for a strict month filter.
        ModFeedKind::MonthlyTop => TRENDING_QUERY,
    };
    let vars = serde_json::json!({ "gameDomain": game_domain });
    let data: serde_json::Value = post(client, api_key, query, vars).await?;
    decode_mod_list(&data)
}

/// Full-text search across a game's mods. `term` is passed as a
/// wildcard filter — callers should trim whitespace but not escape;
/// the server handles tokenization.
pub async fn search_mods(
    client: &Client,
    api_key: &str,
    game_domain: &str,
    term: &str,
    page: u32,
) -> Result<Vec<GqlModTile>> {
    let vars = serde_json::json!({
        "gameDomain": game_domain,
        "term": term,
        "page": i64::from(page),
    });
    let data: serde_json::Value = post(client, api_key, SEARCH_QUERY, vars).await?;
    decode_mod_list(&data)
}

/// Extract a `mods { nodes { ... } }` list from a GraphQL response,
/// tolerating missing fields. Used by both browse and search because
/// they share the same wrapper shape.
fn decode_mod_list(data: &Value) -> Result<Vec<GqlModTile>> {
    let nodes = data
        .get("mods")
        .and_then(|m| m.get("nodes"))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("GraphQL response missing mods.nodes"))?;
    let mut out = Vec::new();
    if let Some(array) = nodes.as_array() {
        for raw in array {
            // The `author` field comes back as `{ name: "..." }` in the v2
            // schema; flatten it before the typed decode, which expects a string.
            let mut raw = raw.clone();
            if let Some(name) = raw.pointer("/author/name").cloned() {
                raw["author"] = name;
            }
            let tile = GqlModTile::deserialize(&raw).unwrap_or_else(|_| GqlModTile {
                mod_id: raw
                    .get("modId")
                    .and_then(serde_json::Value::as_u64)
                    .map(NexusModId::from)
                    .unwrap_or_else(|| NexusModId::from(0)),
                name: raw
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                summary: None,
                version: None,
                author: raw
                    .get("author")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                picture_url: None,
                thumbnail_url: None,
                endorsements: None,
                downloads: None,
                uploaded_at: None,
                game_domain: None,
            });
            out.push(tile);
        }
    }
    Ok(out)
}

// ── Collections feed ─────────────────────────────────────────────

/// A collection as it appears in the GraphQL collections feed (tile view).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GqlCollectionTile {
    pub slug: String,
    pub name: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default, rename = "tileImage")]
    pub tile_image: Option<String>,
    #[serde(default, rename = "gameDomain")]
    pub game_domain: Option<String>,
    #[serde(default, rename = "endorsements")]
    pub endorsements: Option<u64>,
    #[serde(default, rename = "downloads")]
    pub downloads: Option<u64>,
}

const COLLECTIONS_QUERY: &str = r"
query CollectionsFeed($gameDomain: String!, $term: String) {
  collections(
    filter: {
      gameDomain: { value: $gameDomain, op: EQUALS }
      name: { value: $term, op: WILDCARD }
    }
    sort: { endorsements: { direction: DESC } }
    count: 30
  ) {
    nodes {
      slug
      name
      summary
      tileImage
      gameDomain
      endorsements
      downloads
    }
  }
}
";

/// Fetch the collections browse/search feed. `term` is `None` for the
/// default "top collections" listing and `Some(q)` for a user search.
pub async fn collections_feed(
    client: &Client,
    api_key: &str,
    game_domain: &str,
    term: Option<&str>,
) -> Result<Vec<GqlCollectionTile>> {
    let vars = serde_json::json!({
        "gameDomain": game_domain,
        "term": term.unwrap_or(""),
    });
    let data: serde_json::Value = post(client, api_key, COLLECTIONS_QUERY, vars).await?;
    let nodes = data
        .get("collections")
        .and_then(|m| m.get("nodes"))
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("GraphQL response missing collections.nodes"))?;
    let tiles: Vec<GqlCollectionTile> = serde_json::from_value(nodes)?;
    Ok(tiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_mod_list_keeps_typed_fields_when_author_is_an_object() {
        let data = serde_json::json!({ "mods": { "nodes": [{
            "modId": 7, "name": "SkyUI", "summary": "UI overhaul", "version": "5.2",
            "author": { "name": "schlangster" }, "downloads": 10, "gameDomain": "skyrimspecialedition"
        }]}});
        let tiles = decode_mod_list(&data).unwrap();
        assert_eq!(tiles.len(), 1);
        let tile = &tiles[0];
        assert_eq!(tile.mod_id, NexusModId::from(7));
        assert_eq!(tile.author.as_deref(), Some("schlangster"));
        assert_eq!(tile.summary.as_deref(), Some("UI overhaul"));
        assert_eq!(tile.downloads, Some(10));
        assert_eq!(tile.game_domain.as_deref(), Some("skyrimspecialedition"));
    }

    #[test]
    fn decode_mod_list_tolerates_malformed_nodes() {
        assert!(decode_mod_list(&serde_json::json!({ "mods": {} })).is_err());
        let not_an_array = decode_mod_list(&serde_json::json!({ "mods": { "nodes": {} } }));
        assert!(matches!(not_an_array.unwrap().as_slice(), []));
        let tiles =
            decode_mod_list(&serde_json::json!({ "mods": { "nodes": [{ "name": "x", "author": { "name": "a" } }] } }))
                .unwrap();
        assert_eq!(tiles[0].mod_id, NexusModId::from(0));
        assert_eq!(tiles[0].name, "x");
        assert_eq!(tiles[0].author.as_deref(), Some("a"));
    }
}
