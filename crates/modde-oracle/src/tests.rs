use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode};
use modde_oracle_api::{
    CURRENT_SCHEMA_VERSION, CompatEvent, CompatEventBatch, CompatEventKind, CompatQueryResponse,
};
use tower::ServiceExt;

use std::net::SocketAddr;
use std::time::Duration;

use crate::{AppState, router};

fn event(kind: CompatEventKind, pair_hash: &str, observed_at_unix: i64) -> CompatEvent {
    CompatEvent {
        game_id: "skyrim-se".to_string(),
        platform: "x86_64-linux".to_string(),
        salt_epoch: "epoch-1".to_string(),
        mod_set_hash: "a".repeat(64),
        mod_hashes: vec!["b".repeat(64), "c".repeat(64)],
        pair_hashes: vec![pair_hash.to_string()],
        crash_signature_hash: (kind == CompatEventKind::Crash).then(|| "d".repeat(64)),
        kind,
        observed_at_unix,
    }
}

#[tokio::test]
async fn route_rejects_raw_path_payload() {
    let app = router(AppState::memory_for_tests(1));
    let batch = CompatEventBatch {
        schema_version: CURRENT_SCHEMA_VERSION,
        client_version: "0.3.9".to_string(),
        events: vec![CompatEvent {
            game_id: "/home/user/crash.log".to_string(),
            ..event(CompatEventKind::Session, &"e".repeat(64), 1)
        }],
    };
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/compat/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn query_suppresses_small_cohorts() {
    let app = router(AppState::memory_for_tests(3));
    let pair = "e".repeat(64);
    let batch = CompatEventBatch {
        schema_version: CURRENT_SCHEMA_VERSION,
        client_version: "0.3.9".to_string(),
        events: vec![
            event(CompatEventKind::Session, &pair, 1),
            event(CompatEventKind::Crash, &pair, 2),
        ],
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/compat/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ok(response).await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/v1/compat/query?game_id=skyrim-se&pair_hash={pair}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let response = assert_ok(response).await;
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: CompatQueryResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        parsed.stats,
        [] as [modde_oracle_api::CompatAggregateStat; 0]
    );
}

#[tokio::test]
async fn aggregate_math_reports_lift() {
    let app = router(AppState::memory_for_tests(1));
    let pair = "f".repeat(64);
    let other = "a".repeat(64);
    let batch = CompatEventBatch {
        schema_version: CURRENT_SCHEMA_VERSION,
        client_version: "0.3.9".to_string(),
        events: vec![
            event(CompatEventKind::Session, &pair, 1),
            event(CompatEventKind::Crash, &pair, 2),
            event(CompatEventKind::Session, &other, 3),
            event(CompatEventKind::Session, &other, 4),
        ],
    };
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/v1/compat/events")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&batch).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_ok(response).await;

    let response = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!(
                    "/v1/compat/query?game_id=skyrim-se&pair_hash={pair}"
                ))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let parsed: CompatQueryResponse = serde_json::from_slice(&body).unwrap();
    let stat = parsed.stats.first().expect("thresholded stat");
    assert_eq!(stat.coinstall_count, 2);
    assert!((stat.crash_signature_rate - 0.5).abs() < f64::EPSILON);
    assert!((stat.baseline_rate - 0.25).abs() < f64::EPSILON);
    assert!((stat.lift - 2.0).abs() < f64::EPSILON);
}

fn peer(addr: &str) -> ConnectInfo<SocketAddr> {
    ConnectInfo(addr.parse().expect("valid socket address"))
}

fn events_request(body: &[u8], peer_addr: &str, forwarded_for: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri("/v1/compat/events")
        .header("content-type", "application/json")
        .extension(peer(peer_addr));
    if let Some(forwarded_for) = forwarded_for {
        builder = builder.header("x-forwarded-for", forwarded_for);
    }
    builder.body(Body::from(body.to_vec())).unwrap()
}

fn rate_limited_batch_body() -> Vec<u8> {
    let pair = "9".repeat(64);
    let batch = CompatEventBatch {
        schema_version: CURRENT_SCHEMA_VERSION,
        client_version: "0.3.9".to_string(),
        events: vec![event(CompatEventKind::Session, &pair, 1)],
    };
    serde_json::to_vec(&batch).unwrap()
}

#[tokio::test]
async fn post_events_rate_limits_by_peer_address() {
    let app = router(AppState::memory_for_tests(1).with_rate_limit(1, Duration::from_mins(1)));
    let body = rate_limited_batch_body();
    let first = app
        .clone()
        .oneshot(events_request(&body, "203.0.113.10:50000", None))
        .await
        .unwrap();
    assert_ok(first).await;

    let second = app
        .oneshot(events_request(&body, "203.0.113.10:50001", None))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn spoofed_forwarded_for_is_ignored_without_trusted_proxy() {
    let app = router(AppState::memory_for_tests(1).with_rate_limit(1, Duration::from_mins(1)));
    let body = rate_limited_batch_body();
    let first = app
        .clone()
        .oneshot(events_request(
            &body,
            "203.0.113.10:50000",
            Some("198.51.100.1"),
        ))
        .await
        .unwrap();
    assert_ok(first).await;

    let second = app
        .oneshot(events_request(
            &body,
            "203.0.113.10:50001",
            Some("198.51.100.2"),
        ))
        .await
        .unwrap();
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn trusted_proxy_honors_forwarded_for() {
    let app = router(
        AppState::memory_for_tests(1)
            .with_rate_limit(1, Duration::from_mins(1))
            .with_trusted_proxy(true),
    );
    let body = rate_limited_batch_body();
    let first = app
        .clone()
        .oneshot(events_request(
            &body,
            "127.0.0.1:50000",
            Some("198.51.100.1"),
        ))
        .await
        .unwrap();
    assert_ok(first).await;

    let second = app
        .clone()
        .oneshot(events_request(
            &body,
            "127.0.0.1:50001",
            Some("198.51.100.2"),
        ))
        .await
        .unwrap();
    assert_ok(second).await;

    let third = app
        .oneshot(events_request(
            &body,
            "127.0.0.1:50002",
            Some("198.51.100.1"),
        ))
        .await
        .unwrap();
    assert_eq!(third.status(), StatusCode::TOO_MANY_REQUESTS);
}

async fn assert_ok(response: axum::response::Response) -> axum::response::Response {
    let status = response.status();
    if status != StatusCode::OK {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        panic!(
            "expected 200, got {status}: {}",
            String::from_utf8_lossy(&body)
        );
    }
    response
}
