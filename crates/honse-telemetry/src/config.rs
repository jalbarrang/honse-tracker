//! On-disk telemetry config (path injected by the caller, typically
//! `<edge base dir>/telemetry.json` or similar — no fork-host coupling).
//!
//! Telemetry defaults to **disabled** so normal users are never affected. Every
//! field is `#[serde(default)]` for forward/backward compatibility.
//!
//! Authentication: the sidecar owns the per-install bearer token. It generates
//! it once and persists it as `auth_token` inside `install.json` under its data
//! root (`%LOCALAPPDATA%\dreki-gg\honse-tracker\data`). This crate only *reads*
//! that file — it never generates, rewrites, or logs the token.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channels {
    #[serde(default = "default_true")]
    pub career: bool,
    #[serde(default = "default_true")]
    pub career_extras: bool,
}

impl Default for Channels {
    fn default() -> Self {
        Self {
            career: true,
            career_extras: true,
        }
    }
}

/// Authentication settings. The token itself never lives in `telemetry.json`;
/// only the location of the sidecar-owned `install.json` can be overridden.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Auth {
    /// Path to the sidecar's `install.json` (the file holding `auth_token`).
    /// `None` resolves the default sidecar data root under `%LOCALAPPDATA%`.
    #[serde(default)]
    pub token_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Master switch. Default `false`.
    #[serde(default)]
    pub enabled: bool,
    /// Sidecar ingest URL. Only `http://host:port/path` is supported (no TLS;
    /// the sidecar binds loopback only).
    #[serde(default = "default_endpoint")]
    pub endpoint: String,
    #[serde(default)]
    pub channels: Channels,
    #[serde(default)]
    pub auth: Auth,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: default_endpoint(),
            channels: Channels::default(),
            auth: Auth::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_endpoint() -> String {
    "http://127.0.0.1:24210/api/ingest".to_string()
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

    /// The `install.json` this config reads the bearer token from: the explicit
    /// `auth.token_file` override, else the default sidecar install file.
    #[must_use]
    pub fn token_file(&self) -> Option<PathBuf> {
        self.auth.token_file.clone().or_else(default_token_file)
    }
}

/// Default sidecar install file:
/// `%LOCALAPPDATA%\dreki-gg\honse-tracker\data\install.json` (the same path the
/// dashboard's `platform::load_or_create_token` writes). `None` only when
/// `LOCALAPPDATA` is unset.
#[must_use]
pub fn default_token_file() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty())?;
    Some(
        PathBuf::from(local)
            .join("dreki-gg")
            .join("honse-tracker")
            .join("data")
            .join("install.json"),
    )
}

/// Read the per-install bearer token (`auth_token`) from a sidecar
/// `install.json`. Returns `None` for a missing/unreadable file, malformed
/// JSON, or an absent/empty token. The token is never logged.
#[must_use]
pub fn load_token(path: &Path) -> Option<BearerToken> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: serde_json::Value = serde_json::from_str(&text).ok()?;
    let token = doc.get("auth_token")?.as_str()?;
    if token.is_empty() {
        return None;
    }
    Some(BearerToken::new(token.to_string()))
}

/// Per-install bearer token with a redacted `Debug` so the secret cannot leak
/// through logging or panic formatting.
#[derive(Clone, PartialEq, Eq)]
pub struct BearerToken(String);

impl BearerToken {
    #[must_use]
    pub fn new(token: String) -> Self {
        Self(token)
    }

    /// The raw token, for constructing the `Authorization` header only.
    #[must_use]
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for BearerToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BearerToken(<redacted>)")
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
        assert_eq!(cfg.endpoint, "http://127.0.0.1:24210/api/ingest");
        assert!(cfg.channels.career && cfg.channels.career_extras);
        assert!(cfg.auth.token_file.is_none());
    }

    #[test]
    fn partial_json_fills_defaults() {
        let cfg: Config = serde_json::from_str(r#"{"enabled":true}"#).expect("parse");
        assert!(cfg.enabled);
        assert_eq!(cfg.endpoint, "http://127.0.0.1:24210/api/ingest");
        assert!(cfg.channels.career_extras);
    }

    #[test]
    fn token_file_override_wins_over_default() {
        let cfg: Config = serde_json::from_str(r#"{"auth":{"token_file":"C:/custom/install.json"}}"#).expect("parse");
        assert_eq!(cfg.token_file(), Some(PathBuf::from("C:/custom/install.json")));
    }

    #[test]
    fn default_token_file_is_the_sidecar_install_file() {
        // Skip on machines without LOCALAPPDATA (non-Windows CI).
        if let Some(path) = default_token_file() {
            let s = path.to_string_lossy().replace('\\', "/");
            assert!(
                s.ends_with("dreki-gg/honse-tracker/data/install.json"),
                "unexpected default token file: {s}"
            );
        }
    }

    #[test]
    fn load_token_reads_auth_token_and_rejects_bad_files() {
        let dir = std::env::temp_dir().join(format!("honse-telemetry-token-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let file = dir.join("install.json");

        std::fs::write(&file, r#"{"auth_token":"abc123","installed_version":"0.1.0"}"#).expect("write");
        let token = load_token(&file).expect("token present");
        assert_eq!(token.expose(), "abc123");

        std::fs::write(&file, r#"{"auth_token":""}"#).expect("write");
        assert!(load_token(&file).is_none(), "empty token is rejected");

        std::fs::write(&file, "not json").expect("write");
        assert!(load_token(&file).is_none(), "malformed json is rejected");

        assert!(load_token(&dir.join("missing.json")).is_none(), "missing file is None");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bearer_token_debug_is_redacted() {
        let token = BearerToken::new("super-secret".to_string());
        let debug = format!("{token:?}");
        assert!(!debug.contains("super-secret"), "debug must not leak the token");
        assert!(debug.contains("redacted"));
    }

    #[test]
    fn channels_partial_override() {
        let cfg: Config = serde_json::from_str(r#"{"channels":{"career_extras":false}}"#).expect("parse");
        assert!(cfg.channels.career);
        assert!(!cfg.channels.career_extras);
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
