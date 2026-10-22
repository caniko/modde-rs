//! `NexusApi` REST and GraphQL wrappers against a mocked Nexus.

use modde_core::NexusModId;
use modde_sources::nexus::NexusApi;
use modde_sources::nexus::graphql::ModFeedKind;
use serde_json::{Value, json};
use wiremock::matchers::{body_partial_json, body_string_contains, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Serialises the Nexus URL overrides, which every request reads.
static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A Nexus whose REST and GraphQL endpoints share one mock server.
async fn nexus() -> (MockServer, NexusApi) {
    let server = MockServer::start().await;
    // SAFETY: every caller holds ENV_LOCK for the whole test.
    unsafe {
        std::env::set_var("MODDE_NEXUS_BASE_URL", server.uri());
        std::env::set_var(
            "MODDE_NEXUS_GRAPHQL_URL",
            format!("{}/graphql", server.uri()),
        );
    }
    (server, NexusApi::new(reqwest::Client::new(), "key".into()))
}

async fn mount(server: &MockServer, verb: &str, route: &str, response: ResponseTemplate) {
    Mock::given(method(verb))
        .and(path(route))
        .respond_with(response)
        .mount(server)
        .await;
}

fn body(value: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(value)
}

fn nexus_mod(id: u64) -> Value {
    json!({
        "mod_id": id, "name": format!("Mod {id}"), "summary": null, "version": "1.0",
        "author": "a", "domain_name": "skyrim", "endorsement_count": 7,
    })
}

fn collection(name: &str) -> Value {
    json!({
        "slug": "abc", "name": name, "summary": null, "description": null,
        "author": { "name": "a", "member_id": null },
        "game": { "id": 1, "domain_name": "skyrim", "name": "Skyrim" },
        "version": { "version": "1", "created_at": null }, "endorsements": 4, "image_url": null,
    })
}

#[tokio::test]
async fn feeds_prefer_graphql_tiles() {
    let _env = ENV_LOCK.lock().await;
    let (server, api) = nexus().await;
    mount(
        &server,
        "POST",
        "/graphql",
        body(json!({ "data": {
            "mods": { "nodes": [{ "modId": 5, "name": "Gql" }] },
            "collections": { "nodes": [{ "slug": "gql", "name": "Gql" }] },
        } })),
    )
    .await;

    let browse = api
        .browse_feed_gql("skyrim", ModFeedKind::MonthlyTop)
        .await
        .unwrap();
    assert_eq!(browse[0].mod_id, NexusModId::from(5));
    assert_eq!(
        api.search_mods_gql("skyrim", "g", 1).await.unwrap()[0].name,
        "Gql"
    );
    assert_eq!(
        api.collections_feed_gql("skyrim", Some("g")).await.unwrap()[0].slug,
        "gql"
    );
}

#[tokio::test]
async fn feeds_fall_back_to_rest_when_graphql_fails() {
    let _env = ENV_LOCK.lock().await;
    let (server, api) = nexus().await;
    mount(&server, "POST", "/graphql", ResponseTemplate::new(500)).await;
    mount(
        &server,
        "GET",
        "/games/skyrim/mods/trending.json",
        body(json!([nexus_mod(1)])),
    )
    .await;
    Mock::given(method("GET"))
        .and(path("/games/skyrim/mods/search.json"))
        .and(query_param("search", "ui"))
        .and(query_param("page", "2"))
        .respond_with(body(json!({ "results": [nexus_mod(2)], "total": 1 })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/games/skyrim/collections.json"))
        .and(query_param("search", ""))
        .respond_with(body(json!([collection("Essentials")])))
        .mount(&server)
        .await;

    let trending = api
        .browse_feed_gql("skyrim", ModFeedKind::Trending)
        .await
        .unwrap();
    assert_eq!(trending[0].mod_id, NexusModId::from(1));
    assert_eq!(trending[0].endorsements, Some(7));
    assert_eq!(trending[0].game_domain.as_deref(), Some("skyrim"));
    assert_eq!(
        api.search_mods_gql("skyrim", "ui", 2).await.unwrap()[0].name,
        "Mod 2"
    );
    let collections = api.collections_feed_gql("skyrim", None).await.unwrap();
    assert_eq!(collections[0].name, "Essentials");
    assert_eq!(collections[0].game_domain.as_deref(), Some("skyrim"));

    // With both endpoints down the REST error surfaces.
    assert!(api.search_mods_gql("skyrim", "other", 1).await.is_err());
}

#[tokio::test]
async fn social_actions_post_their_forms() {
    let _env = ENV_LOCK.lock().await;
    let (server, api) = nexus().await;
    for route in [
        "/games/skyrim/mods/1/endorse.json",
        "/games/skyrim/mods/1/abstain.json",
    ] {
        Mock::given(method("POST"))
            .and(path(route))
            .and(body_string_contains("Version=1.0"))
            .respond_with(body(json!({})))
            .expect(1)
            .mount(&server)
            .await;
    }
    for verb in ["POST", "DELETE"] {
        Mock::given(method(verb))
            .and(path("/user/tracked_mods.json"))
            .and(body_string_contains("domain_name=skyrim&mod_id=1"))
            .respond_with(body(json!({})))
            .expect(1)
            .mount(&server)
            .await;
    }
    mount(
        &server,
        "GET",
        "/user/tracked_mods.json",
        body(json!([{ "mod_id": 1, "domain_name": "skyrim" }])),
    )
    .await;
    let id = NexusModId::from(1);

    api.endorse_mod("skyrim", id, "1.0").await.unwrap();
    api.abstain_mod("skyrim", id, "1.0").await.unwrap();
    api.track_mod("skyrim", id).await.unwrap();
    api.untrack_mod("skyrim", id).await.unwrap();
    let tracked = api.get_tracked_mods().await.unwrap();
    assert_eq!(
        (tracked[0].mod_id, tracked[0].domain_name.as_str()),
        (id, "skyrim")
    );
    assert!(
        api.endorse_mod("skyrim", NexusModId::from(2), "1.0")
            .await
            .is_err()
    );
    server.verify().await;
}

#[tokio::test]
async fn requests_surface_rate_limits_and_http_errors() {
    let _env = ENV_LOCK.lock().await;
    let (server, api) = nexus().await;
    mount(
        &server,
        "GET",
        "/games/skyrim/mods/1.json",
        body(nexus_mod(1)).insert_header("x-rl-hourly-remaining", "3"),
    )
    .await;
    mount(
        &server,
        "GET",
        "/games/skyrim/mods/2.json",
        ResponseTemplate::new(429),
    )
    .await;
    mount(
        &server,
        "GET",
        "/files/a.bin",
        ResponseTemplate::new(200).set_body_bytes(b"abc".to_vec()),
    )
    .await;

    assert_eq!(
        api.get_mod("skyrim", NexusModId::from(1))
            .await
            .unwrap()
            .name,
        "Mod 1"
    );
    let limited = api
        .get_mod("skyrim", NexusModId::from(2))
        .await
        .unwrap_err();
    assert!(
        limited.to_string().contains("rate limit exceeded"),
        "{limited}"
    );
    let file = format!("{}/files/a.bin", server.uri());
    assert_eq!(api.fetch_bytes(&file).await.unwrap(), b"abc");
    let missing = format!("{}/files/missing.bin", server.uri());
    assert!(api.fetch_bytes(&missing).await.is_err());
}

#[tokio::test]
async fn collections_resolve_their_game_and_revision() {
    let _env = ENV_LOCK.lock().await;
    let (server, api) = nexus().await;
    mount(
        &server,
        "GET",
        "/collections/abc.json",
        body(json!({
            "game": { "domain_name": "skyrim" },
            "latest_published_revision": { "revision_number": 3 },
        })),
    )
    .await;
    mount(
        &server,
        "GET",
        "/collections/draft.json",
        body(json!({ "game": { "domain_name": "skyrim" } })),
    )
    .await;
    for (revision, name) in [(3, "Latest"), (5, "Pinned")] {
        mount(
            &server,
            "GET",
            &format!("/games/skyrim/collections/abc/revisions/{revision}.json"),
            body(collection(name)),
        )
        .await;
    }
    mount(
        &server,
        "GET",
        "/games/skyrim/collections/abc.json",
        body(collection("Direct")),
    )
    .await;

    assert_eq!(
        api.get_collection_by_slug("abc", None).await.unwrap().name,
        "Latest"
    );
    assert_eq!(
        api.get_collection_by_slug("abc", Some(5))
            .await
            .unwrap()
            .name,
        "Pinned"
    );
    let draft = api.get_collection_by_slug("draft", None).await.unwrap_err();
    assert!(
        draft.to_string().contains("has no published revisions"),
        "{draft}"
    );
    assert_eq!(
        api.get_collection("skyrim", "abc").await.unwrap().name,
        "Direct"
    );
}

#[tokio::test]
async fn mod_media_reads_gallery_urls() {
    let _env = ENV_LOCK.lock().await;
    let (server, api) = nexus().await;
    for (mod_id, response) in [
        (
            1,
            json!({ "data": { "mod": { "modImages": [{ "url": "a.png" }, { "url": "b.png" }, {}] } } }),
        ),
        (2, json!({ "errors": [{ "message": "nope" }] })),
        (3, json!({ "data": {} })),
    ] {
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(body_partial_json(
                json!({ "variables": { "modId": mod_id, "gameDomain": "skyrim" } }),
            ))
            .respond_with(body(response))
            .mount(&server)
            .await;
    }

    let media = |id: u64| api.get_mod_media("skyrim", NexusModId::from(id));
    assert_eq!(media(1).await.unwrap(), ["a.png", "b.png"]);
    assert!(
        media(2)
            .await
            .unwrap_err()
            .to_string()
            .contains("Nexus GraphQL errors")
    );
    assert!(
        media(3)
            .await
            .unwrap_err()
            .to_string()
            .contains("unexpected GraphQL response shape")
    );
}
