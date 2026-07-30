//! Ingest integration tests: auth, content negotiation, protobuf decoding,
//! idempotent commits, size limits, the health contract, and a real loopback
//! TCP round trip. No WebView or window is ever created.

mod common;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use common::make_turn;
use honse_dashboard::ingest::{self, IngestConfig, CONTENT_TYPE_PROTOBUF, MAX_BODY_BYTES};
use honse_dashboard::storage::Storage;
use honse_dashboard::{pb, storage, AppEvent};
use http_body_util::BodyExt;
use prost::Message;
use secrecy::SecretString;
use tokio::sync::mpsc::UnboundedReceiver;
use tower::ServiceExt;

const TOKEN: &str = "test-token-0123456789abcdef";

fn test_router(dir: &std::path::Path) -> (axum::Router, Storage, UnboundedReceiver<AppEvent>) {
    let db = storage::open(&dir.join("t.db")).expect("open storage");
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let config = IngestConfig {
        bind: "127.0.0.1:0".parse().expect("addr"),
        auth_token: SecretString::from(TOKEN),
    };
    (ingest::router(&config, db.clone(), tx), db, rx)
}

fn envelope_body(turn: &pb::SettledTurn) -> Vec<u8> {
    pb::Envelope {
        sent_at_ms: 1,
        seq: 1,
        source: "training-tracker".to_string(),
        payload: Some(pb::envelope::Payload::SettledTurn(turn.clone())),
    }
    .encode_to_vec()
}

fn turn_request(body: Vec<u8>, token: Option<&str>, content_type: &str) -> Request<Body> {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/v1/turns")
        .header(header::CONTENT_TYPE, content_type);
    if let Some(token) = token {
        builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    builder.body(Body::from(body)).expect("request")
}

#[tokio::test]
async fn new_capture_commits_with_201_and_notifies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, mut rx) = test_router(dir.path());

    let turn = make_turn("cap-1", 1001, 5, 10, 1_000);
    let res = router
        .oneshot(turn_request(envelope_body(&turn), Some(TOKEN), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::CREATED);

    assert_eq!(db.totals().expect("totals").captures, 1);
    let event = rx.try_recv().expect("committed event");
    assert!(
        matches!(event, AppEvent::TurnCommitted { turn: 10, ref capture_id, .. } if capture_id == "cap-1"),
        "got {event:?}"
    );
}

#[tokio::test]
async fn bare_settled_turn_body_is_accepted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, _rx) = test_router(dir.path());

    let body = make_turn("cap-bare", 1001, 5, 10, 1_000).encode_to_vec();
    let res = router
        .oneshot(turn_request(body, Some(TOKEN), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::CREATED);
    assert_eq!(db.totals().expect("totals").captures, 1);
}

#[tokio::test]
async fn duplicate_capture_returns_200_with_single_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, mut rx) = test_router(dir.path());

    let body = envelope_body(&make_turn("cap-1", 1001, 5, 10, 1_000));
    let first = router
        .clone()
        .oneshot(turn_request(body.clone(), Some(TOKEN), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("first");
    assert_eq!(first.status(), StatusCode::CREATED);

    let second = router
        .oneshot(turn_request(body, Some(TOKEN), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("second");
    assert_eq!(second.status(), StatusCode::OK, "duplicate acknowledged as success");

    assert_eq!(db.totals().expect("totals").captures, 1, "exactly one row");
    let _ = rx.try_recv().expect("committed event");
    let dup = rx.try_recv().expect("duplicate event");
    assert!(matches!(dup, AppEvent::DuplicateDiscarded { ref capture_id } if capture_id == "cap-1"));
}

#[tokio::test]
async fn missing_or_wrong_token_gets_401_and_stores_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, _rx) = test_router(dir.path());
    let body = envelope_body(&make_turn("cap-1", 1001, 5, 10, 1_000));

    let missing = router
        .clone()
        .oneshot(turn_request(body.clone(), None, CONTENT_TYPE_PROTOBUF))
        .await
        .expect("missing");
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let wrong = router
        .oneshot(turn_request(body, Some("wrong-token"), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("wrong");
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);

    assert_eq!(db.totals().expect("totals").captures, 0);
}

#[tokio::test]
async fn wrong_content_type_and_malformed_protobuf_get_400() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, _rx) = test_router(dir.path());

    let wrong_ct = router
        .clone()
        .oneshot(turn_request(
            envelope_body(&make_turn("cap-1", 1001, 5, 10, 1_000)),
            Some(TOKEN),
            "application/json",
        ))
        .await
        .expect("wrong ct");
    assert_eq!(wrong_ct.status(), StatusCode::BAD_REQUEST);

    let garbage = router
        .clone()
        .oneshot(turn_request(vec![0xff; 64], Some(TOKEN), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("garbage");
    assert_eq!(garbage.status(), StatusCode::BAD_REQUEST);

    // Envelope with a non-settled-turn payload is rejected too.
    let legacy = pb::Envelope {
        sent_at_ms: 1,
        seq: 1,
        source: "training-tracker".to_string(),
        payload: Some(pb::envelope::Payload::CareerSnapshot(pb::CareerSnapshot::default())),
    }
    .encode_to_vec();
    let legacy_res = router
        .oneshot(turn_request(legacy, Some(TOKEN), CONTENT_TYPE_PROTOBUF))
        .await
        .expect("legacy");
    assert_eq!(legacy_res.status(), StatusCode::BAD_REQUEST);

    assert_eq!(db.totals().expect("totals").captures, 0);
}

#[tokio::test]
async fn oversized_body_is_rejected_with_413() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, _rx) = test_router(dir.path());

    let res = router
        .oneshot(turn_request(
            vec![0u8; MAX_BODY_BYTES + 1],
            Some(TOKEN),
            CONTENT_TYPE_PROTOBUF,
        ))
        .await
        .expect("oversized");
    assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(db.totals().expect("totals").captures, 0);
}

#[tokio::test]
async fn turn_rewind_over_http_creates_a_new_career() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, db, _rx) = test_router(dir.path());

    for (id, turn_no, at) in [("cap-1", 30, 1_000u64), ("cap-2", 2, 2_000)] {
        let res = router
            .clone()
            .oneshot(turn_request(
                envelope_body(&make_turn(id, 1001, 5, turn_no, at)),
                Some(TOKEN),
                CONTENT_TYPE_PROTOBUF,
            ))
            .await
            .expect("response");
        assert_eq!(res.status(), StatusCode::CREATED);
    }
    let history = db
        .career_history(honse_dashboard::storage::HistoryQuery::default())
        .expect("history");
    assert_eq!(history.len(), 2, "rewind starts a new run");
}

#[tokio::test]
async fn healthz_reports_identity_and_requires_auth() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (router, _db, _rx) = test_router(dir.path());

    let unauth = router
        .clone()
        .oneshot(Request::builder().uri("/healthz").body(Body::empty()).expect("request"))
        .await
        .expect("unauth");
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED, "wrong token reads unhealthy");

    let ok = router
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .header(header::AUTHORIZATION, format!("Bearer {TOKEN}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("ok");
    assert_eq!(ok.status(), StatusCode::OK);
    let bytes = ok.into_body().collect().await.expect("body").to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["app"], "honse-dashboard");
    assert_eq!(json["version"], honse_dashboard::APP_VERSION);
    assert_eq!(json["ingest_protocol"], honse_dashboard::INGEST_PROTOCOL);
    assert!(json["db_size_bytes"].as_i64().is_some_and(|n| n > 0));
    assert!(json["uptime_ms"].as_u64().is_some());
}

/// Full loopback TCP round trip on an ephemeral port, using the same raw
/// HTTP/1.1 framing as the DLL transport (`Connection: close`).
#[tokio::test]
async fn real_loopback_socket_accepts_a_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let db = storage::open(&dir.path().join("t.db")).expect("open storage");
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let config = IngestConfig {
        bind: "127.0.0.1:0".parse().expect("addr"),
        auth_token: SecretString::from(TOKEN),
    };
    let router = ingest::router(&config, db.clone(), tx);
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .expect("bind ephemeral");
    let addr = listener.local_addr().expect("local addr");
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .expect("serve");
    });

    let body = envelope_body(&make_turn("cap-tcp", 1001, 5, 10, 1_000));
    let status = tokio::task::spawn_blocking(move || raw_post(addr, &body))
        .await
        .expect("client task");
    assert_eq!(status, 201);
    assert_eq!(db.totals().expect("totals").captures, 1);

    let _ = shutdown_tx.send(());
    server.await.expect("server exits");
}

/// Minimal blocking HTTP client mirroring `honse-telemetry/src/transport.rs`.
fn raw_post(addr: std::net::SocketAddr, body: &[u8]) -> u16 {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    let header = format!(
        "POST /v1/turns HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        addr, TOKEN, CONTENT_TYPE_PROTOBUF, body.len()
    );
    stream.write_all(header.as_bytes()).expect("write header");
    stream.write_all(body).expect("write body");
    let mut response = Vec::new();
    stream.read_to_end(&mut response).expect("read");
    let text = String::from_utf8_lossy(&response);
    text.split_whitespace()
        .nth(1)
        .expect("status code")
        .parse()
        .expect("numeric status")
}
