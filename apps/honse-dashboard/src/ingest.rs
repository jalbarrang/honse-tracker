//! Authenticated loopback HTTP ingest server (axum).
//!
//! Contract (see HANDOFF and the bootstrap plan):
//! - `GET  /healthz` — authenticated; JSON identity/health for the bootstrap
//!   probe. A wrong or missing token gets `401`, so a probe with the wrong
//!   token reads as "unhealthy".
//! - `POST /v1/turns` — `Authorization: Bearer <token>`,
//!   `Content-Type: application/x-protobuf`. The body is either a
//!   `hachimi.telemetry.v1.Envelope` whose payload is `settled_turn` (what the
//!   DLL publisher actually sends) or a bare `SettledTurn`. `201` = committed,
//!   `200` = duplicate `capture_id`, `400` malformed, `401` bad token,
//!   `413` oversized, `500` storage failure. Success is returned only after
//!   the SQLite transaction commits.
//!
//! The server binds loopback only; a non-loopback bind address is refused.

use std::net::SocketAddr;
use std::time::Instant;

use anyhow::{anyhow, Context, Result};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use prost::Message;
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::mpsc::UnboundedSender;

use crate::storage::{InsertOutcome, Storage};
use crate::{pb, AppEvent, APP_NAME, APP_VERSION, INGEST_PROTOCOL};

/// Largest accepted request body. A settled turn is a few KiB; 2 MiB leaves
/// generous headroom while bounding memory.
pub const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
/// Required content type for turn payloads.
pub const CONTENT_TYPE_PROTOBUF: &str = "application/x-protobuf";

/// Ingest server configuration.
#[derive(Debug, Clone)]
pub struct IngestConfig {
    /// Bind address; must be loopback.
    pub bind: SocketAddr,
    /// Per-install bearer token.
    pub auth_token: SecretString,
}

#[derive(Clone)]
struct IngestState {
    storage: Storage,
    events: UnboundedSender<AppEvent>,
    /// BLAKE3 of the expected token; comparison via `blake3::Hash` is
    /// constant-time, and the raw token never sits in server state.
    token_hash: blake3::Hash,
    started: Instant,
}

/// Build the router. Exposed for integration tests (`tower::ServiceExt`).
pub fn router(config: &IngestConfig, storage: Storage, events: UnboundedSender<AppEvent>) -> Router {
    let state = IngestState {
        storage,
        events,
        token_hash: blake3::hash(config.auth_token.expose_secret().as_bytes()),
        started: Instant::now(),
    };
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/turns", post(post_turn))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

/// Bind `config.bind` (loopback only) and serve until `shutdown` resolves.
pub async fn serve_with_shutdown(
    config: IngestConfig,
    storage: Storage,
    events: UnboundedSender<AppEvent>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> Result<()> {
    if !config.bind.ip().is_loopback() {
        return Err(anyhow!("refusing non-loopback ingest bind {}", config.bind));
    }
    let app = router(&config, storage, events);
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .with_context(|| format!("bind ingest listener on {}", config.bind))?;
    let local = listener.local_addr()?;
    tracing::info!(addr = %local, "ingest listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .context("ingest server failed")
}

/// Bind and serve forever (production entry point per the HANDOFF signature).
pub async fn serve(config: IngestConfig, storage: Storage, events: UnboundedSender<AppEvent>) -> Result<()> {
    serve_with_shutdown(config, storage, events, std::future::pending()).await
}

fn authorized(state: &IngestState, headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    // Hash-then-compare: blake3::Hash equality is constant-time.
    blake3::hash(token.as_bytes()) == state.token_hash
}

async fn healthz(State(state): State<IngestState>, headers: HeaderMap) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }
    let db_size_bytes = {
        let storage = state.storage.clone();
        tokio::task::spawn_blocking(move || storage.totals().map(|t| t.db_size_bytes))
            .await
            .ok()
            .and_then(Result::ok)
    };
    Json(serde_json::json!({
        "status": "ok",
        "app": APP_NAME,
        "version": APP_VERSION,
        "ingest_protocol": INGEST_PROTOCOL,
        "uptime_ms": state.started.elapsed().as_millis() as u64,
        "db_size_bytes": db_size_bytes,
    }))
    .into_response()
}

async fn post_turn(State(state): State<IngestState>, headers: HeaderMap, body: Bytes) -> Response {
    if !authorized(&state, &headers) {
        return unauthorized();
    }

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if content_type != CONTENT_TYPE_PROTOBUF {
        return bad_request("content-type must be application/x-protobuf");
    }

    let turn = match decode_turn(&body) {
        Ok(turn) => turn,
        Err(msg) => return bad_request(&msg),
    };
    let event_capture_id = turn.capture_id.clone();
    let event_captured_at_ms = turn.captured_at_ms;

    let storage = state.storage.clone();
    let outcome = tokio::task::spawn_blocking(move || storage.insert_settled_turn(&turn)).await;
    match outcome {
        Ok(Ok(InsertOutcome::Committed { career_id, turn })) => {
            // The row is durable; notify the UI. A send failure only means the
            // UI receiver is gone, which must not fail ingestion.
            let _ = state.events.send(AppEvent::TurnCommitted {
                career_id,
                turn,
                capture_id: event_capture_id,
                captured_at_ms: event_captured_at_ms,
            });
            (StatusCode::CREATED, Json(serde_json::json!({ "status": "committed" }))).into_response()
        }
        Ok(Ok(InsertOutcome::Duplicate { capture_id })) => {
            let _ = state.events.send(AppEvent::DuplicateDiscarded { capture_id });
            (StatusCode::OK, Json(serde_json::json!({ "status": "duplicate" }))).into_response()
        }
        Ok(Err(err)) => {
            // Validation failures (empty capture_id, missing snapshot) are the
            // client's fault; anything else is a storage failure.
            let msg = err.to_string();
            if msg.contains("capture_id is empty") || msg.contains("no snapshot") {
                bad_request(&msg)
            } else {
                tracing::error!(error = %msg, "turn insert failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({ "status": "error" })),
                )
                    .into_response()
            }
        }
        Err(join_err) => {
            tracing::error!(error = %join_err, "storage task panicked");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "status": "error" })),
            )
                .into_response()
        }
    }
}

/// Decode a request body into a `SettledTurn`. Accepts the Envelope framing
/// the DLL publisher sends, or a bare `SettledTurn`.
fn decode_turn(body: &[u8]) -> std::result::Result<pb::SettledTurn, String> {
    if let Ok(envelope) = pb::Envelope::decode(body) {
        return match envelope.payload {
            Some(pb::envelope::Payload::SettledTurn(turn)) => Ok(turn),
            Some(_) => Err("envelope payload is not settled_turn".to_string()),
            // An all-defaults Envelope also decodes from garbage that happens
            // to skip fields; fall through to the bare decode in that case.
            None => pb::SettledTurn::decode(body).map_err(|e| format!("malformed protobuf: {e}")),
        };
    }
    pb::SettledTurn::decode(body).map_err(|e| format!("malformed protobuf: {e}"))
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Bearer")],
        Json(serde_json::json!({ "status": "unauthorized" })),
    )
        .into_response()
}

fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "status": "rejected", "reason": msg })),
    )
        .into_response()
}
