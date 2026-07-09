//! On-disk telemetry config (path injected by the caller, typically
//! `<edge base dir>/telemetry.json` or similar — no fork-host coupling).
//!
//! Telemetry defaults to **disabled** so normal users are never affected. Every
//! field is `#[serde(default)]` for forward/backward compatibility.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    #[serde(default = "default_true")]
    pub career: bool,
    #[serde(default = "default_true")]
    pub career_extras: bool,
    #[serde(default = "default_true")]
    pub race_live: bool,
    #[serde(default = "default_true")]
    pub race_full: bool,
}

impl Default for Channels {
    fn default() -> Self {
        Self {
            career: true,
            career_extras: true,
            race_live: true,
            race_full: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Master switch. Default `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Ingest webhook URL. Only `http://host:port/path` is supported (no TLS).
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub channels: Channels,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_endpoint(),
            channels: Channels::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_endpoint() -> String {
    "http://127.0.0.1:8716/ingest".to_string()
}

impl Config {
    /// Load from `path`. A missing file or any parse error yields the disabled
    /// default (telemetry never breaks the plugin).
    #[must_use]
    pub fn load(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

/// Parsed `http://host:port/path` endpoint for the raw HTTP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub host: String,
    pub port: u16,
    pub path: String,
}

impl Endpoint {
    /// Parse an `http://host[:port]/path` URL. Returns `None` for unsupported
    /// schemes (e.g. `https`) or malformed input.
    #[must_use]
    pub fn parse(url: &str) -> Option<Self> {
        let rest = url.strip_prefix("http://")?;
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        if authority.is_empty() {
            return None;
        }
        let (host, port) = match authority.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse().ok()?),
            None => (authority.to_string(), 80u16),
        };
        if host.is_empty() {
            return None;
        }
        Some(Self {
            host,
            port,
            path: path.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_yields_disabled_default() {
        let cfg = Config::load(Path::new("/nonexistent/telemetry.json"));
        assert!(!cfg.enabled);
        assert_eq!(cfg.endpoint, "http://127.0.0.1:8716/ingest");
        assert!(cfg.channels.career && cfg.channels.race_full);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let cfg: Config = serde_json::from_str(r#"{"enabled":true}"#).expect("parse");
        assert!(cfg.enabled);
        assert_eq!(cfg.endpoint, "http://127.0.0.1:8716/ingest");
        assert!(cfg.channels.race_live);
    }

    #[test]
    fn channels_partial_override() {
        let cfg: Config = serde_json::from_str(r#"{"channels":{"race_full":false}}"#).expect("parse");
        assert!(cfg.channels.career);
        assert!(!cfg.channels.race_full);
    }

    #[test]
    fn endpoint_parses_host_port_path() {
        let e = Endpoint::parse("http://127.0.0.1:8716/ingest").expect("parse");
        assert_eq!(e.host, "127.0.0.1");
        assert_eq!(e.port, 8716);
        assert_eq!(e.path, "/ingest");
    }

    #[test]
    fn endpoint_defaults_port_and_path() {
        let e = Endpoint::parse("http://localhost").expect("parse");
        assert_eq!(e.port, 80);
        assert_eq!(e.path, "/");
    }

    #[test]
    fn endpoint_rejects_https_and_garbage() {
        assert!(Endpoint::parse("https://127.0.0.1/ingest").is_none());
        assert!(Endpoint::parse("ws://x").is_none());
        assert!(Endpoint::parse("http://").is_none());
    }
}
