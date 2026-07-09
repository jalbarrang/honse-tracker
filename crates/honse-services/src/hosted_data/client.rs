//! Hosted-data HTTP client with an injectable fetch seam.
//!
//! The fork's `client.rs` has concrete ureq calls with no injection seam
//! (verified). We introduce [`Fetcher`] so unit tests supply an in-memory
//! implementation and never hit the network.
//!
//! // DEVIATION from fork: injected fetch seam for network-free tests

use std::{collections::HashMap, io::Read};

use serde::Deserialize;

/// User-Agent pattern from the fork, updated to point at honse-tracker.
const USER_AGENT: &str = concat!(
    "honse-tracker/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/jalbarrang/honse-tracker; hosted-data)"
);

/// Max bytes for a single hosted binary snapshot (icon PNG).
const MAX_BINARY_BYTES: u64 = 16 * 1024 * 1024;

/// Published manifest (`manifest.json`): `filename -> blake3 hash`.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct HostedManifest {
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub files: HashMap<String, String>,
}

#[derive(Debug)]
pub enum FetchError {
    Http(String),
    Json(String),
    Io(String),
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(s) | Self::Json(s) | Self::Io(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for FetchError {}

/// // DEVIATION from fork: injected fetch seam for network-free tests
pub trait Fetcher: Send + Sync {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError>;
}

/// Production fetcher using `ureq` v2.
pub struct UreqFetcher;

impl Fetcher for UreqFetcher {
    fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
        let res = ureq::Agent::new()
            .get(url)
            .set("User-Agent", USER_AGENT)
            .call()
            .map_err(|e| FetchError::Http(e.to_string()))?;
        let mut buf = Vec::new();
        res.into_reader()
            .take(MAX_BINARY_BYTES)
            .read_to_end(&mut buf)
            .map_err(|e| FetchError::Io(e.to_string()))?;
        Ok(buf)
    }
}

/// Download the hosted `manifest.json` from `base`.
pub fn load_manifest(fetcher: &dyn Fetcher, base: &str) -> Result<HostedManifest, FetchError> {
    let url = format!("{}/manifest.json", base.trim_end_matches('/'));
    let bytes = fetcher.get(&url)?;
    let text = String::from_utf8(bytes).map_err(|e| FetchError::Io(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| FetchError::Json(e.to_string()))
}

/// Download a JSON snapshot; validate JSON before returning.
pub fn fetch_snapshot(fetcher: &dyn Fetcher, base: &str, file: &str) -> Result<String, FetchError> {
    let url = format!("{}/{}", base.trim_end_matches('/'), file);
    let bytes = fetcher.get(&url)?;
    let text = String::from_utf8(bytes).map_err(|e| FetchError::Io(e.to_string()))?;
    serde_json::from_str::<serde::de::IgnoredAny>(&text).map_err(|e| FetchError::Json(e.to_string()))?;
    Ok(text)
}

/// Download a binary snapshot (no JSON validation).
pub fn fetch_snapshot_bytes(fetcher: &dyn Fetcher, base: &str, file: &str) -> Result<Vec<u8>, FetchError> {
    let url = format!("{}/{}", base.trim_end_matches('/'), file);
    fetcher.get(&url)
}
