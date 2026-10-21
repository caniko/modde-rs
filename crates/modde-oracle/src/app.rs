use std::net::SocketAddr;
use std::time::Duration;

use axum::extract::rejection::ExtensionRejection;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use modde_oracle_api::{
    CompatEventAccepted, CompatEventBatch, CompatQueryResponse, OracleConfigResponse,
};
use serde::Deserialize;
use sqlx::postgres::PgPoolOptions;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::MAX_REQUEST_BYTES;
use crate::error::ApiError;
use crate::postgres::migrate;
use crate::rate_limit::RateLimiter;
use crate::store::{MemoryStore, Store};

#[derive(Clone)]
pub struct AppState {
    store: Store,
    config: OracleConfigResponse,
    rate_limiter: RateLimiter,
}

impl AppState {
    #[must_use]
    pub fn memory_for_tests(min_cohort: i64) -> Self {
        Self {
            store: Store::Memory(MemoryStore::default()),
            config: OracleConfigResponse {
                min_cohort,
                ..OracleConfigResponse::default()
            },
            rate_limiter: RateLimiter::from_env(),
        }
    }

    pub async fn postgres(database_url: &str, min_cohort: i64) -> anyhow::Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(database_url)
            .await?;
        migrate(&pool).await?;
        Ok(Self {
            store: Store::Postgres(pool),
            config: OracleConfigResponse {
                min_cohort,
                ..OracleConfigResponse::default()
            },
            rate_limiter: RateLimiter::from_env(),
        })
    }

    #[must_use]
    pub fn with_rate_limit(mut self, burst: u32, window: Duration) -> Self {
        self.rate_limiter = RateLimiter::new(burst, window, self.rate_limiter.trusted_proxy);
        self
    }

    #[must_use]
    pub fn with_trusted_proxy(mut self, trusted_proxy: bool) -> Self {
        self.rate_limiter.trusted_proxy = trusted_proxy;
        self
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/compat/config", get(config))
        .route("/v1/compat/events", post(post_events))
        .route("/v1/compat/query", get(query_compat))
        .layer(RequestBodyLimitLayer::new(MAX_REQUEST_BYTES))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, "modde compatibility oracle listening");
    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await?;
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}

async fn healthz() -> &'static str {
    "ok"
}

async fn config(State(state): State<AppState>) -> Json<OracleConfigResponse> {
    Json(state.config)
}

async fn post_events(
    State(state): State<AppState>,
    peer: Result<ConnectInfo<SocketAddr>, ExtensionRejection>,
    headers: HeaderMap,
    Json(batch): Json<CompatEventBatch>,
) -> Result<Json<CompatEventAccepted>, ApiError> {
    let key = rate_limit_key(
        &headers,
        peer.ok().map(|ConnectInfo(addr)| addr),
        state.rate_limiter.trusted_proxy,
    );
    state
        .rate_limiter
        .check(&key)
        .map_err(|()| ApiError::RateLimited)?;
    batch.validate()?;
    let accepted = state
        .store
        .ingest_events(&batch.events, state.config.min_cohort)
        .await?;
    Ok(Json(CompatEventAccepted { accepted }))
}

pub(super) fn rate_limit_key(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    trusted_proxy: bool,
) -> String {
    if trusted_proxy {
        let forwarded = headers
            .get("x-forwarded-for")
            .or_else(|| headers.get("x-real-ip"))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(forwarded) = forwarded {
            return forwarded.to_string();
        }
    }
    peer.map_or_else(|| "unknown".to_string(), |addr| addr.ip().to_string())
}

#[derive(Debug, Deserialize)]
struct CompatQuery {
    game_id: String,
    pair_hash: String,
}

async fn query_compat(
    State(state): State<AppState>,
    Query(query): Query<CompatQuery>,
) -> Result<Json<CompatQueryResponse>, ApiError> {
    let stats = state
        .store
        .query_stats(
            &query.game_id,
            &query
                .pair_hash
                .split(',')
                .filter(|hash| !hash.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
            state.config.min_cohort,
        )
        .await?;
    Ok(Json(CompatQueryResponse {
        game_id: query.game_id,
        stats,
    }))
}
