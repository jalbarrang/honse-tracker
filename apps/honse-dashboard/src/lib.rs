//! Honse dashboard sidecar library.
//!
//! The binary (`main.rs`) wires these modules together; they are a library so
//! unit and integration tests can exercise ingest, storage, and state without
//! ever opening a Dioxus window.
//!
//! Boundaries (keep them distinct):
//! - [`ingest`] — authenticated loopback HTTP server (axum), protobuf decode.
//! - [`storage`] — durable SQLite behind a dedicated worker thread.
//! - [`state`] — deterministic view-model service between storage and the UI.
//! - [`ui`] — Dioxus components rendering the approved prototype.
//! - [`platform`] — data paths, auth token, single instance, WebView2 policy.

pub mod ingest;
pub mod platform;
pub mod state;
pub mod storage;
pub mod ui;

/// Re-export of the shared generated protobuf types (`hachimi.telemetry.v1`).
pub use hachimi_telemetry::pb;

/// Application version reported by `--version`, `/healthz`, and the UI.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Application identifier reported by `/healthz` for the bootstrap contract.
pub const APP_NAME: &str = "honse-dashboard";
/// Ingest wire-contract version reported by `/healthz`. Bump when the HTTP
/// surface (paths, auth scheme, request/response shapes) changes.
pub const INGEST_PROTOCOL: u32 = 1;

/// Events flowing from the ingest server into the reactive UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppEvent {
    /// A new capture was committed to SQLite (never sent for duplicates).
    TurnCommitted {
        career_id: i64,
        turn: i32,
        capture_id: String,
        captured_at_ms: u64,
    },
    /// A retry replay was discarded because its `capture_id` already exists.
    DuplicateDiscarded { capture_id: String },
}
