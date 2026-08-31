//! End-to-end delivery test over a real loopback socket: `init` reads the
//! config + sidecar `install.json` from disk, `publish` enqueues a settled
//! turn, and the sender thread POSTs it with the bearer token to the exact
//! configured path. This pins the full DLL-side wire contract
//! (`POST /v1/turns`, `Authorization: Bearer <install.json token>`,
//! `application/x-protobuf`, Envelope-with-settled_turn body) without any
//! game or sidecar process.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use hachimi_telemetry::{pb, Message};

/// One accepted request: head text plus raw body bytes.
struct CapturedRequest {
    head: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut std::net::TcpStream) -> CapturedRequest {
    stream.set_read_timeout(Some(Duration::from_secs(2))).expect("timeout");
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    let head_end = loop {
        let n = stream.read(&mut chunk).expect("read request");
        assert!(n > 0, "connection closed before full head");
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let content_length: usize = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(str::trim)
                .map(String::from)
        })
        .and_then(|v| v.parse().ok())
        .expect("content-length header");
    let mut body = buf[head_end..].to_vec();
    while body.len() < content_length {
        let n = stream.read(&mut chunk).expect("read body");
        assert!(n > 0, "connection closed before full body");
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(content_length);
    CapturedRequest { head, body }
}

#[test]
fn publish_delivers_authenticated_settled_turn_to_configured_endpoint() {
    let dir = std::env::temp_dir().join(format!("honse-telemetry-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tempdir");

    // Fake sidecar: one-shot loopback listener standing in for /v1/turns.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let req = read_request(&mut stream);
        stream
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .expect("respond");
        req
    });

    // The sidecar-owned install.json (same shape platform::load_or_create_token
    // writes) and a telemetry.json pointing at it.
    let install = dir.join("install.json");
    std::fs::write(
        &install,
        r#"{"auth_token":"e2e-test-token","installed_version":"0.1.0"}"#,
    )
    .expect("install");
    let cfg = dir.join("telemetry.json");
    std::fs::write(
        &cfg,
        format!(
            r#"{{"enabled":true,"endpoint":"http://127.0.0.1:{port}/v1/turns","auth":{{"token_file":{}}}}}"#,
            serde_json::to_string(&install).expect("path json")
        ),
    )
    .expect("config");

    let outcome = hachimi_telemetry::init(Some(cfg));
    assert!(
        matches!(outcome, hachimi_telemetry::InitOutcome::Enabled { .. }),
        "init must enable against the fake sidecar config, got {outcome:?}"
    );
    assert!(hachimi_telemetry::is_enabled());

    hachimi_telemetry::publish(
        "training-tracker",
        pb::envelope::Payload::SettledTurn(pb::SettledTurn {
            capture_id: "e2e-t7-cafe".to_string(),
            captured_at_ms: 1_700_000_000_000,
            snapshot: Some(pb::CareerSnapshot {
                is_playing: true,
                current_turn: 7,
                ..Default::default()
            }),
            extras: Some(pb::CareerExtras::default()),
        }),
    );

    let req = server.join().expect("server thread");

    // Wire contract: exact path, bearer token from install.json, protobuf type.
    assert!(
        req.head.starts_with("POST /v1/turns HTTP/1.1\r\n"),
        "unexpected request line in: {}",
        req.head
    );
    assert!(req.head.contains("Authorization: Bearer e2e-test-token\r\n"));
    assert!(req.head.contains("Content-Type: application/x-protobuf\r\n"));

    // The body is the Envelope framing the sidecar ingests.
    let envelope = pb::Envelope::decode(req.body.as_slice()).expect("decode envelope");
    assert_eq!(envelope.source, "training-tracker");
    let Some(pb::envelope::Payload::SettledTurn(turn)) = envelope.payload else {
        panic!("payload must be settled_turn");
    };
    assert_eq!(turn.capture_id, "e2e-t7-cafe");
    assert_eq!(turn.snapshot.expect("snapshot").current_turn, 7);

    // The delivery counter confirms the send was accounted as successful.
    let deadline = Instant::now() + Duration::from_secs(2);
    while hachimi_telemetry::metrics().sent < 1 {
        assert!(Instant::now() < deadline, "sent counter never incremented");
        std::thread::sleep(Duration::from_millis(10));
    }

    hachimi_telemetry::shutdown();
    assert!(!hachimi_telemetry::is_enabled());
    let _ = std::fs::remove_dir_all(&dir);
}
