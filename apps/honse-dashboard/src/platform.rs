//! Platform integration: data-root resolution, the per-install auth token,
//! the single-instance guard, and the WebView2 availability policy.
//!
//! Contracts shared with the bootstrap plan (`secure-sidecar-bootstrap`):
//! - Data root defaults to `%LOCALAPPDATA%\dreki-gg\honse-tracker\data`,
//!   overridable with `--data-root` or `HONSE_DATA_ROOT`.
//! - The auth token arrives via the `HONSE_INGEST_TOKEN` environment variable
//!   (narrow launch contract, never on the command line), else is read from
//!   `install.json` in the data root, else is generated once and persisted
//!   there. The token is never logged.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use secrecy::SecretString;

/// Environment variable carrying the ingest token from the launching DLL.
pub const TOKEN_ENV: &str = "HONSE_INGEST_TOKEN";
/// Environment variable overriding the data root.
pub const DATA_ROOT_ENV: &str = "HONSE_DATA_ROOT";
/// Database file name inside the data root (bootstrap plan layout).
pub const DB_FILE: &str = "honse.db";
/// Install metadata file inside the data root. Holds the auth token; never log
/// its contents.
pub const INSTALL_FILE: &str = "install.json";

/// Resolve the data root: explicit argument, then `HONSE_DATA_ROOT`, then
/// `%LOCALAPPDATA%\dreki-gg\honse-tracker\data`.
pub fn resolve_data_root(cli_override: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = cli_override {
        return Ok(p.to_path_buf());
    }
    if let Some(p) = std::env::var_os(DATA_ROOT_ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let local = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow!("LOCALAPPDATA is not set; pass --data-root or set {DATA_ROOT_ENV}"))?;
    Ok(PathBuf::from(local).join("dreki-gg").join("honse-tracker").join("data"))
}

/// Database path inside a data root.
#[must_use]
pub fn db_path(data_root: &Path) -> PathBuf {
    data_root.join(DB_FILE)
}

/// Log directory inside a data root.
#[must_use]
pub fn log_dir(data_root: &Path) -> PathBuf {
    data_root.join("logs")
}

/// Load the ingest token: environment first, then `install.json`, else
/// generate a 32-byte random token and persist it atomically.
pub fn load_or_create_token(data_root: &Path) -> Result<SecretString> {
    if let Ok(tok) = std::env::var(TOKEN_ENV) {
        if !tok.is_empty() {
            return Ok(SecretString::from(tok));
        }
    }
    let install = data_root.join(INSTALL_FILE);
    let mut doc: serde_json::Value = match std::fs::read_to_string(&install) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({})),
        Err(_) => serde_json::json!({}),
    };
    if let Some(tok) = doc.get("auth_token").and_then(|v| v.as_str()) {
        if !tok.is_empty() {
            return Ok(SecretString::from(tok.to_string()));
        }
    }

    let token = generate_token()?;
    doc["auth_token"] = serde_json::Value::String(token.clone());
    std::fs::create_dir_all(data_root).with_context(|| format!("create data root {}", data_root.display()))?;
    let tmp = install.with_extension("json.part");
    std::fs::write(&tmp, serde_json::to_vec_pretty(&doc)?).context("write install.json.part")?;
    std::fs::rename(&tmp, &install).context("commit install.json")?;
    Ok(SecretString::from(token))
}

fn generate_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow!("system RNG unavailable: {e}"))?;
    let mut out = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(out, "{b:02x}");
    }
    Ok(out)
}

/// Held for the process lifetime by the first instance.
pub struct SingleInstanceGuard {
    #[cfg(windows)]
    handle: isize,
    #[cfg(not(windows))]
    _lock: std::fs::File,
}

// SAFETY: the mutex handle is only closed on drop and is valid process-wide.
#[cfg(windows)]
unsafe impl Send for SingleInstanceGuard {}

/// Try to become the single running instance for `name`. Returns `Ok(None)`
/// when another instance already holds the guard (the caller should exit).
#[cfg(windows)]
pub fn acquire_single_instance(name: &str) -> Result<Option<SingleInstanceGuard>> {
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;

    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is a valid NUL-terminated UTF-16 string outliving the call.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(anyhow!("CreateMutexW failed"));
    }
    // SAFETY: plain last-error read after a successful handle-returning call.
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        // SAFETY: `handle` is the live mutex handle we just received.
        unsafe { CloseHandle(handle) };
        return Ok(None);
    }
    Ok(Some(SingleInstanceGuard {
        handle: handle as isize,
    }))
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::Foundation::CloseHandle;
        // SAFETY: the handle was created by CreateMutexW and not closed before.
        unsafe { CloseHandle(self.handle as *mut core::ffi::c_void) };
    }
}

/// Non-Windows fallback used only for development: an exclusive lock file in
/// the temp directory keyed by `name`.
#[cfg(not(windows))]
pub fn acquire_single_instance(name: &str) -> Result<Option<SingleInstanceGuard>> {
    let path = std::env::temp_dir().join(format!("{}.lock", name.replace(['\\', '/'], "_")));
    let lock = std::fs::OpenOptions::new().create(true).write(true).open(&path)?;
    // Advisory create-new marker: good enough for a dev-only fallback.
    let marker = path.with_extension("pid");
    match std::fs::OpenOptions::new().create_new(true).write(true).open(&marker) {
        Ok(_) => Ok(Some(SingleInstanceGuard { _lock: lock })),
        Err(_) => Ok(None),
    }
}

/// WebView2 runtime availability, per the explicit missing-runtime policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebViewRuntime {
    /// Runtime present; contains the reported `pv` version string.
    Available(String),
    /// No usable runtime registration was found.
    Missing,
}

/// Classify a raw registry `pv` value. Microsoft documents `""` and
/// `"0.0.0.0"` as "not installed".
#[must_use]
pub fn classify_webview_pv(pv: Option<&str>) -> WebViewRuntime {
    match pv {
        Some(v) if !v.trim().is_empty() && v.trim() != "0.0.0.0" => WebViewRuntime::Available(v.trim().to_string()),
        _ => WebViewRuntime::Missing,
    }
}

/// Detect the WebView2 evergreen runtime via its documented registry keys.
/// `HONSE_SKIP_WEBVIEW_CHECK=1` bypasses the check (development escape hatch).
#[cfg(windows)]
pub fn detect_webview2() -> WebViewRuntime {
    if std::env::var("HONSE_SKIP_WEBVIEW_CHECK").is_ok_and(|v| v == "1") {
        return WebViewRuntime::Available("skipped".to_string());
    }
    const CLIENT_KEY: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let candidates = [
        (
            true,
            format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{CLIENT_KEY}"),
        ),
        (true, format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{CLIENT_KEY}")),
        (false, format!(r"Software\Microsoft\EdgeUpdate\Clients\{CLIENT_KEY}")),
    ];
    for (machine, subkey) in candidates {
        if let Some(pv) = read_registry_string(machine, &subkey, "pv") {
            if let WebViewRuntime::Available(v) = classify_webview_pv(Some(&pv)) {
                return WebViewRuntime::Available(v);
            }
        }
    }
    WebViewRuntime::Missing
}

#[cfg(not(windows))]
pub fn detect_webview2() -> WebViewRuntime {
    // Non-Windows dev builds use the platform webkit; treat as available.
    WebViewRuntime::Available("native".to_string())
}

#[cfg(windows)]
fn read_registry_string(machine: bool, subkey: &str, value: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};

    let root = if machine { HKEY_LOCAL_MACHINE } else { HKEY_CURRENT_USER };
    let subkey_w: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
    let value_w: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let mut buf = [0u16; 128];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: all pointers reference live, NUL-terminated buffers; `len` is
    // the byte capacity of `buf` as RegGetValueW requires.
    let status = unsafe {
        RegGetValueW(
            root,
            subkey_w.as_ptr(),
            value_w.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buf.as_mut_ptr().cast(),
            &mut len,
        )
    };
    if status != 0 {
        return None;
    }
    let chars = (len as usize / 2).saturating_sub(1);
    Some(String::from_utf16_lossy(&buf[..chars.min(buf.len())]))
}

/// Show a blocking native error dialog (used when WebView2 is missing).
#[cfg(windows)]
pub fn show_error_dialog(title: &str, message: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
    let title_w: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    let msg_w: Vec<u16> = message.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: both strings are valid NUL-terminated UTF-16 for the call.
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            msg_w.as_ptr(),
            title_w.as_ptr(),
            MB_OK | MB_ICONERROR,
        )
    };
}

#[cfg(not(windows))]
pub fn show_error_dialog(_title: &str, message: &str) {
    eprintln!("{message}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use secrecy::ExposeSecret;

    #[test]
    fn data_root_prefers_cli_override() {
        let root = resolve_data_root(Some(Path::new(r"C:\custom"))).expect("resolve");
        assert_eq!(root, PathBuf::from(r"C:\custom"));
    }

    #[test]
    fn db_and_log_paths_are_inside_root() {
        let root = Path::new(r"C:\data");
        assert_eq!(db_path(root), Path::new(r"C:\data\honse.db"));
        assert_eq!(log_dir(root), Path::new(r"C:\data\logs"));
    }

    #[test]
    fn token_is_generated_persisted_and_reloaded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = load_or_create_token(dir.path()).expect("create");
        assert_eq!(first.expose_secret().len(), 64, "32 random bytes hex-encoded");
        let installed = std::fs::read_to_string(dir.path().join(INSTALL_FILE)).expect("install.json written");
        assert!(installed.contains(first.expose_secret()));
        let second = load_or_create_token(dir.path()).expect("reload");
        assert_eq!(
            first.expose_secret(),
            second.expose_secret(),
            "token is stable per install"
        );
    }

    #[test]
    fn token_preserves_other_install_fields() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join(INSTALL_FILE), r#"{"installed_version":"0.1.0"}"#).expect("seed");
        let _ = load_or_create_token(dir.path()).expect("create");
        let doc: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.path().join(INSTALL_FILE)).expect("read")).expect("json");
        assert_eq!(doc["installed_version"], "0.1.0");
        assert!(doc["auth_token"].as_str().is_some_and(|t| t.len() == 64));
    }

    #[test]
    fn webview_pv_classification() {
        assert_eq!(classify_webview_pv(None), WebViewRuntime::Missing);
        assert_eq!(classify_webview_pv(Some("")), WebViewRuntime::Missing);
        assert_eq!(classify_webview_pv(Some("0.0.0.0")), WebViewRuntime::Missing);
        assert_eq!(
            classify_webview_pv(Some("120.0.2210.61")),
            WebViewRuntime::Available("120.0.2210.61".to_string())
        );
    }

    #[test]
    fn single_instance_guard_blocks_second_acquire() {
        let name = format!("Local\\honse-dashboard-test-{}", std::process::id());
        let first = acquire_single_instance(&name).expect("first acquire");
        assert!(first.is_some(), "first instance acquires the guard");
        let second = acquire_single_instance(&name).expect("second acquire");
        assert!(second.is_none(), "second instance is refused while the first lives");
        drop(first);
        let third = acquire_single_instance(&name).expect("third acquire");
        assert!(third.is_some(), "guard is released on drop");
    }
}
