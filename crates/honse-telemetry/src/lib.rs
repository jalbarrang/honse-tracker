//! Shared protobuf telemetry emitter for Hachimi plugins.
//!
//! Plugins call [`init`] once (passing the resolved path to `telemetry.json`),
//! then [`publish`] envelopes from their existing data-refresh points, and
//! [`shutdown`] from their `SHUTDOWN` handler. Everything is a cheap no-op when
//! telemetry is disabled (the default).
//!
//! Transport is an HTTP POST of the protobuf-encoded [`pb::Envelope`] to a
//! configurable localhost webhook; a Bun/Hono backend re-broadcasts to browsers
//! over WebSocket. See the `telemetry-dashboard` initiative.

mod config;
mod publisher;
mod transport;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;

pub use config::{Channels, Config};

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

/// Initialize telemetry from `telemetry.json` at `cfg_path`. A `None` path or a
/// missing/disabled config leaves telemetry off. Safe to call once at plugin init.
pub fn init(cfg_path: Option<PathBuf>) {
    let cfg = match cfg_path {
        Some(p) => Config::load(&p),
        None => Config::default(),
    };
    let _ = CHANNELS.set(cfg.channels.clone());
    if !cfg.enabled {
        return;
    }
    let Some(endpoint) = publisher::endpoint_from(&cfg) else {
        return;
    };
    publisher::start(endpoint);
    ENABLED.store(true, Ordering::Release);
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

/// Stop the sender thread and flush state. Call from the plugin SHUTDOWN handler.
pub fn shutdown() {
    ENABLED.store(false, Ordering::Release);
    publisher::stop();
}

/// Envelopes dropped due to a full queue (diagnostics).
#[must_use]
pub fn dropped_count() -> u64 {
    publisher::dropped_count()
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
