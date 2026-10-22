//! Nexus v2 GraphQL feeds against a mocked endpoint.

use modde_core::NexusModId;
use modde_sources::nexus::graphql::{self, ModFeedKind};
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Serialises `MODDE_NEXUS_GRAPHQL_URL`, which every request reads.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Point the GraphQL client at a server that answers a POST carrying
/// `variables` with `response`.
async fn endpoint(variables: Value, response: ResponseTemplate) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .and(header("apikey", "key"))
        .and(body_partial_json(json!({ "variables": variables })))
        .respond_with(response)
        .mount(&server)
        .await;
    // SAFETY: every caller holds ENV_LOCK for the whole test.
    unsafe {
        std::env::set_var(
            "MODDE_NEXUS_GRAPHQL_URL",
            format!("{}/graphql", server.uri()),
        );
    }
    server
}

fn data(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

#[tokio::test]
async fn feeds_decode_tiles_and_tolerate_malformed_ones() {
    let _env = ENV_LOCK.lock().await;
    let nodes = json!({ "data": { "mods": { "nodes": [
        {
            "modId": 1, "name": "SkyUI", "summary": "s", "version": "5.2",
            "author": { "name": "schlangster" }, "downloads": 10, "gameDomain": "skyrim",
        },
        { "modId": 2, "name": "Broken", "downloads": "many", "author": "a" },
    ] } } });
    let _server = endpoint(json!({ "gameDomain": "skyrim" }), data(nodes)).await;
    let client = reqwest::Client::new();

    for kind in [ModFeedKind::Trending, ModFeedKind::MonthlyTop] {
        let tiles = graphql::browse_feed(&client, "key", "skyrim", kind)
            .await
            .unwrap();
        assert_eq!(tiles.len(), 2);
        assert_eq!(tiles[0].author.as_deref(), Some("schlangster"));
        assert_eq!(tiles[0].downloads, Some(10));
        assert_eq!(tiles[1].mod_id, NexusModId::from(2));
        assert_eq!(tiles[1].name, "Broken");
        assert_eq!(tiles[1].author.as_deref(), Some("a"));
        assert_eq!(tiles[1].downloads, None);
    }
}

#[tokio::test]
async fn search_sends_term_and_page() {
    let _env = ENV_LOCK.lock().await;
    let _server = endpoint(
        json!({ "gameDomain": "skyrim", "term": "ui", "page": 2 }),
        data(json!({ "data": { "mods": { "nodes": [{ "modId": 3, "name": "UI" }] } } })),
    )
    .await;

    let tiles = graphql::search_mods(&reqwest::Client::new(), "key", "skyrim", "ui", 2)
        .await
        .unwrap();
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].name, "UI");
}

#[tokio::test]
async fn collections_feed_defaults_to_an_empty_term() {
    let _env = ENV_LOCK.lock().await;
    let _server = endpoint(
        json!({ "gameDomain": "skyrim", "term": "" }),
        data(json!({ "data": { "collections": { "nodes": [
            { "slug": "abc", "name": "Essentials", "endorsements": 5 },
        ] } } })),
    )
    .await;

    let tiles = graphql::collections_feed(&reqwest::Client::new(), "key", "skyrim", None)
        .await
        .unwrap();
    assert_eq!(tiles.len(), 1);
    assert_eq!(tiles[0].slug, "abc");
    assert_eq!(tiles[0].endorsements, Some(5));
}

#[tokio::test]
async fn malformed_responses_are_errors() {
    let _env = ENV_LOCK.lock().await;
    let client = reqwest::Client::new();
    let any = json!({ "gameDomain": "skyrim" });

    for (response, expected) in [
        (
            data(json!({ "errors": [{ "message": "bad query" }] })),
            "Nexus GraphQL errors",
        ),
        (data(json!({ "errors": null })), "missing `data`"),
        (data(json!({ "data": {} })), "missing mods.nodes"),
        (ResponseTemplate::new(500), "GraphQL transport error"),
        (
            ResponseTemplate::new(200).set_body_string("not json"),
            "failed to decode GraphQL response",
        ),
    ] {
        let _server = endpoint(any.clone(), response).await;
        let err = graphql::browse_feed(&client, "key", "skyrim", ModFeedKind::Trending)
            .await
            .unwrap_err();
        assert!(format!("{err:#}").contains(expected), "{err:#}");
    }

    let _server = endpoint(any, data(json!({ "data": { "mods": [] } }))).await;
    let err = graphql::collections_feed(&client, "key", "skyrim", Some("x"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("collections.nodes"), "{err}");
}
