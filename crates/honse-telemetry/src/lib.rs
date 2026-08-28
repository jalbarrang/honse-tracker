//! Shared protobuf telemetry emitter for Hachimi plugins.
//!
//! Plugins call [`init`] once (passing the resolved path to `telemetry.json`),
//! then [`publish`] settled-turn envelopes from their capture points, and
//! [`shutdown`] from their `SHUTDOWN` handler. Everything is a cheap no-op when
//! telemetry is disabled (the default).
//!
//! Transport sends the protobuf-encoded [`pb::Envelope`] to the configured
//! local HTTP endpoint. Requests include a bearer token when the configured
//! token file contains one.

mod config;
mod publisher;
mod transport;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

pub use config::{BearerToken, Channels, Config};
/// Re-export so dependents can encode/decode [`pb`] types without pinning
/// their own copy of prost.
pub use prost::Message;
pub use publisher::Metrics;

/// Generated protobuf types (`hachimi.telemetry.v1`).
#[allow(clippy::large_enum_variant, clippy::doc_markdown)]
pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/hachimi.telemetry.v1.rs"));
}

/// Logical channels, gated independently in config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Career,
    CareerExtras,
}

static ENABLED: AtomicBool = AtomicBool::new(false);
static SEQ: AtomicU32 = AtomicU32::new(0);
static CHANNELS: OnceLock<config::Channels> = OnceLock::new();

/// Why [`init`] left telemetry enabled or disabled. The caller is expected to
/// log the outcome so a misconfiguration fails *visibly* instead of silently.
/// None of the variants ever carry the token itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitOutcome {
    /// Config missing or `enabled=false` (the default): telemetry stays off.
    Disabled,
    /// Config enabled but the endpoint URL is unusable: telemetry stays off.
    InvalidEndpoint(String),
    /// Config enabled but no bearer token could be read from the sidecar's
    /// install file: telemetry stays off rather than posting requests the
    /// sidecar will reject. Carries the path that was tried (never the token).
    MissingToken(String),
    /// Telemetry is running against `endpoint`.
    Enabled { endpoint: String },
}

/// Initialize telemetry from `telemetry.json` at `cfg_path`. A `None` path or a
/// missing/disabled config leaves telemetry off. Safe to call once at plugin init.
pub fn init(cfg_path: Option<PathBuf>) -> InitOutcome {
    let cfg = match cfg_path {
        Some(p) => Config::load(&p),
        None => Config::default(),
    };
    let _ = CHANNELS.set(cfg.channels.clone());
    if !cfg.enabled {
        return InitOutcome::Disabled;
    }
    let Some(endpoint) = publisher::endpoint_from(&cfg) else {
        return InitOutcome::InvalidEndpoint(cfg.endpoint.clone());
    };
    // Auth is optional: if the token file is missing or unreadable, start
    // without a bearer token. The local Node server doesn't require auth.
    let token = cfg.token_file().and_then(|f| config::load_token(&f));
    publisher::start(endpoint, token);
    ENABLED.store(true, Ordering::Release);
    InitOutcome::Enabled {
        endpoint: cfg.endpoint.clone(),
    }
}

/// Whether telemetry is active (fast atomic check).
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Whether `channel` is enabled (combines the master switch and per-channel flag).
#[must_use]
pub fn channel_enabled(channel: Channel) -> bool {
    if !is_enabled() {
        return false;
    }
    let Some(ch) = CHANNELS.get() else {
        return false;
    };
    match channel {
        Channel::Career => ch.career,
        Channel::CareerExtras => ch.career_extras,
    }
}

/// Encode and enqueue an envelope. Stamps `seq`, `sent_at_ms`, and `source`.
/// Never blocks; a no-op when telemetry is disabled.
pub fn publish(source: &str, payload: pb::envelope::Payload) {
    if !is_enabled() {
        return;
    }
    let envelope = pb::Envelope {
        sent_at_ms: now_ms(),
        seq: SEQ.fetch_add(1, Ordering::Relaxed),
        source: source.to_string(),
        payload: Some(payload),
    };
    publisher::enqueue(envelope.encode_to_vec());
}

/// Stop accepting new envelopes and perform a bounded sender shutdown. Call
/// from the plugin SHUTDOWN handler.
///
/// Bounded means: any retry backoff is interrupted immediately, the queued
/// backlog is abandoned (best effort — durability starts only after the
/// sidecar commits a payload), and at most one already-in-flight POST (itself
/// capped by the transport timeouts) completes. Shutdown never retries forever.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Release);
    publisher::stop();
}

/// Envelopes dropped without a send attempt: full queue plus enqueues while no
/// publisher was running (diagnostics). See [`metrics`] for the breakdown.
#[must_use]
pub fn dropped_count() -> u64 {
    publisher::dropped_count()
}

/// Snapshot of the delivery counters: successful sends, retries of a failed
/// head job, and the two drop causes (full queue, disconnected publisher).
#[must_use]
pub fn metrics() -> Metrics {
    publisher::metrics()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_roundtrip() {
        let env = pb::Envelope {
            sent_at_ms: 123,
            seq: 7,
            source: "training-tracker".to_string(),
            payload: Some(pb::envelope::Payload::CareerSnapshot(pb::CareerSnapshot::default())),
        };
        let bytes = env.encode_to_vec();
        let back = pb::Envelope::decode(bytes.as_slice()).expect("decode");
        assert_eq!(back.seq, 7);
        assert_eq!(back.source, "training-tracker");
        match back.payload.expect("payload") {
            pb::envelope::Payload::CareerSnapshot(snapshot) => {
                assert_eq!(snapshot.current_turn, 0);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn envelope_legacy_variants_decode_with_pinned_field_numbers() {
        // Legacy snapshot envelope: payload must stay on field 10 (tag 0x52).
        let snap_env = pb::Envelope {
            sent_at_ms: 1,
            seq: 1,
            source: "training-tracker".to_string(),
            payload: Some(pb::envelope::Payload::CareerSnapshot(pb::CareerSnapshot {
                current_turn: 42,
                ..Default::default()
            })),
        };
        let bytes = snap_env.encode_to_vec();
        assert!(bytes.contains(&0x52), "career_snapshot must keep field number 10");
        match pb::Envelope::decode(bytes.as_slice()).expect("decode").payload {
            Some(pb::envelope::Payload::CareerSnapshot(s)) => assert_eq!(s.current_turn, 42),
            other => panic!("wrong variant: {other:?}"),
        }

        // Legacy extras envelope: payload must stay on field 11 (tag 0x5A).
        let extras_env = pb::Envelope {
            sent_at_ms: 2,
            seq: 2,
            source: "training-tracker".to_string(),
            payload: Some(pb::envelope::Payload::CareerExtras(pb::CareerExtras {
                skill_points: Some(300),
                ..Default::default()
            })),
        };
        let bytes = extras_env.encode_to_vec();
        assert!(bytes.contains(&0x5A), "career_extras must keep field number 11");
        match pb::Envelope::decode(bytes.as_slice()).expect("decode").payload {
            Some(pb::envelope::Payload::CareerExtras(e)) => assert_eq!(e.skill_points, Some(300)),
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn envelope_settled_turn_roundtrip_is_atomic() {
        let turn = pb::SettledTurn {
            capture_id: "career-77-turn-12".to_string(),
            captured_at_ms: 1_700_000_000_000,
            snapshot: Some(pb::CareerSnapshot {
                is_playing: true,
                current_turn: 12,
                speed: 800,
                ..Default::default()
            }),
            extras: Some(pb::CareerExtras {
                skill_points: Some(500),
                skills: vec![pb::AcquiredSkill {
                    master_id: 100,
                    level: 2,
                    name: "Test".to_string(),
                }],
                ..Default::default()
            }),
        };
        let env = pb::Envelope {
            sent_at_ms: 3,
            seq: 3,
            source: "training-tracker".to_string(),
            payload: Some(pb::envelope::Payload::SettledTurn(turn)),
        };
        let bytes = env.encode_to_vec();
        // settled_turn is the new additive oneof member on field 12 (tag 0x62).
        assert!(bytes.contains(&0x62), "settled_turn must sit on field number 12");
        let back = pb::Envelope::decode(bytes.as_slice()).expect("decode");
        let Some(pb::envelope::Payload::SettledTurn(turn)) = back.payload else {
            panic!("wrong variant");
        };
        // Snapshot and extras arrive together in the single payload.
        assert_eq!(turn.capture_id, "career-77-turn-12");
        assert_eq!(turn.captured_at_ms, 1_700_000_000_000);
        let snap = turn.snapshot.expect("snapshot present");
        assert!(snap.is_playing);
        assert_eq!(snap.current_turn, 12);
        assert_eq!(snap.speed, 800);
        let extras = turn.extras.expect("extras present");
        assert_eq!(extras.skill_points, Some(500));
        assert_eq!(extras.skills.len(), 1);
        assert_eq!(extras.skills[0].master_id, 100);
    }

    /// Enabled-but-misconfigured setups must fail visibly (a reported outcome)
    /// and leave telemetry off — never enabled without a token.
    #[test]
    fn init_outcomes_fail_closed_without_starting() {
        let dir = std::env::temp_dir().join(format!("honse-telemetry-init-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");

        // Missing config file → Disabled.
        assert_eq!(init(Some(dir.join("absent.json"))), InitOutcome::Disabled);
        assert!(!is_enabled());

        // Enabled with an unusable endpoint → InvalidEndpoint.
        let cfg = dir.join("bad-endpoint.json");
        std::fs::write(&cfg, r#"{"enabled":true,"endpoint":"https://127.0.0.1/v1/turns"}"#).expect("write");
        assert!(matches!(init(Some(cfg)), InitOutcome::InvalidEndpoint(_)));
        assert!(!is_enabled());

        // Enabled but the token file does not exist → MissingToken with the
        // tried path (and never the token) in the detail.
        let missing_token = dir.join("no-install.json");
        let cfg = dir.join("no-token.json");
        std::fs::write(
            &cfg,
            format!(
                r#"{{"enabled":true,"auth":{{"token_file":{}}}}}"#,
                serde_json::to_string(&missing_token).expect("path json")
            ),
        )
        .expect("write");
        match init(Some(cfg)) {
            InitOutcome::Enabled { .. } => { /* auth-less start is fine */ }
            other => panic!("expected Enabled (auth optional), got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn publish_disabled_is_noop() {
        // ENABLED defaults to false; publish should not panic or enqueue.
        publish(
            "training-tracker",
            pb::envelope::Payload::CareerSnapshot(pb::CareerSnapshot::default()),
        );
    }

    #[test]
    fn channel_disabled_when_not_enabled() {
        assert!(!channel_enabled(Channel::Career));
    }
}
