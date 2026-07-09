//! Per-plugin config file helper (horse-act convention).
//!
//! horse-act writes `<plugin_dir>/hachimi/horseACTConfig.json` (see
//! `horse-act/src/config.rs` `init_paths`). Here the directory is edge's base
//! dir via [`edge_sdk::Sdk::base_dir`], and the filename is caller-chosen
//! (e.g. `HonseTrackerConfig.json`) — task contract:
//! `<edge base dir>/<FileName>.json`.
//!
//! Behavior:
//! - missing file → write `T::default()` (auto-create)
//! - parse error → log warning, back up to `<name>.json.bak`, use defaults
//!   (never silently overwrite the corrupt file without a backup)
//! - [`PluginConfig::save`] writes pretty JSON atomically (temp + rename)

use std::{
    fs,
    io::Write,
    marker::PhantomData,
    path::{Path, PathBuf},
};

use serde::{de::DeserializeOwned, Serialize};

use edge_sdk::Sdk;

/// Hosted-data URL overrides stored in the plugin config (wires t-005).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct HostedDataUrls {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gametora_data_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracker_data_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icons_data_url: Option<String>,
}

impl HostedDataUrls {
    /// Convert to the `[gametora, tracker, icons]` array expected by [`crate::sync_all`].
    #[must_use]
    pub fn as_overrides(&self) -> [Option<String>; 3] {
        [
            self.gametora_data_url.clone(),
            self.tracker_data_url.clone(),
            self.icons_data_url.clone(),
        ]
    }
}

/// Generic plugin config loaded from `<base_dir>/<file_name>`.
pub struct PluginConfig<T: Serialize + DeserializeOwned + Default> {
    path: PathBuf,
    pub value: T,
    _marker: PhantomData<T>,
}

impl<T: Serialize + DeserializeOwned + Default> PluginConfig<T> {
    /// Load from edge `base_dir()` / `file_name`. Returns `None` if base_dir is unavailable.
    pub fn load(file_name: &str) -> Option<Self> {
        let base = Sdk::try_get()?.base_dir()?;
        Some(Self::load_from_path(base.join(file_name)))
    }

    /// Load from an explicit path (tests + callers that already resolved the dir).
    pub fn load_from_path(path: PathBuf) -> Self {
        let value = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(text) => match serde_json::from_str::<T>(&text) {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!(
                            "honse-services: failed to parse config {}: {e}; backing up to .bak and using defaults",
                            path.display()
                        );
                        let bak = backup_path(&path);
                        if let Err(be) = fs::copy(&path, &bak) {
                            log::warn!("honse-services: failed to write config backup {}: {be}", bak.display());
                        }
                        let defaults = T::default();
                        // Write defaults to the main path so next load succeeds;
                        // corrupt original is preserved as .bak.
                        let _ = write_atomic(&path, &defaults);
                        defaults
                    }
                },
                Err(e) => {
                    log::warn!(
                        "honse-services: failed to read config {}: {e}; using defaults",
                        path.display()
                    );
                    T::default()
                }
            }
        } else {
            let defaults = T::default();
            if let Err(e) = write_atomic(&path, &defaults) {
                log::warn!("honse-services: failed to auto-create config {}: {e}", path.display());
            }
            defaults
        };
        Self {
            path,
            value,
            _marker: PhantomData,
        }
    }

    /// Persist current value as pretty JSON (atomic temp + rename).
    pub fn save(&self) -> Result<(), std::io::Error> {
        write_atomic(&self.path, &self.value).map_err(|e| std::io::Error::other(e.to_string()))
    }

    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

fn write_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = {
        let mut t = path.as_os_str().to_owned();
        t.push(".tmp");
        PathBuf::from(t)
    };
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    {
        let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
        f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
        f.write_all(b"\n").map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    // Ensure no leftover temp (rename should have moved it).
    let _ = fs::remove_file(&tmp);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Demo {
        #[serde(default)]
        name: String,
        #[serde(default)]
        count: u32,
        #[serde(default)]
        hosted: HostedDataUrls,
    }

    #[test]
    fn missing_file_writes_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DemoConfig.json");
        assert!(!path.exists());
        let cfg = PluginConfig::<Demo>::load_from_path(path.clone());
        assert_eq!(cfg.value, Demo::default());
        assert!(path.is_file());
        let loaded: Demo = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(loaded, Demo::default());
    }

    #[test]
    fn corrupted_file_backs_up_and_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DemoConfig.json");
        fs::write(&path, "{not json!!!").unwrap();
        let cfg = PluginConfig::<Demo>::load_from_path(path.clone());
        assert_eq!(cfg.value, Demo::default());
        let bak = PathBuf::from(format!("{}.bak", path.display()));
        assert!(bak.is_file());
        let bak_text = fs::read_to_string(&bak).unwrap();
        assert!(bak_text.contains("not json"));
        // Main file rewritten with defaults.
        let main: Demo = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(main, Demo::default());
    }

    #[test]
    fn save_load_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DemoConfig.json");
        let mut cfg = PluginConfig::<Demo>::load_from_path(path.clone());
        cfg.value.name = "honse".into();
        cfg.value.count = 42;
        cfg.value.hosted.gametora_data_url = Some("https://example.test/gt".into());
        cfg.save().unwrap();

        let cfg2 = PluginConfig::<Demo>::load_from_path(path);
        assert_eq!(cfg2.value.name, "honse");
        assert_eq!(cfg2.value.count, 42);
        assert_eq!(
            cfg2.value.hosted.gametora_data_url.as_deref(),
            Some("https://example.test/gt")
        );
    }

    #[test]
    fn atomic_write_leaves_no_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("DemoConfig.json");
        let mut cfg = PluginConfig::<Demo>::load_from_path(path.clone());
        cfg.value.count = 7;
        cfg.save().unwrap();
        let tmp = PathBuf::from(format!("{}.tmp", path.display()));
        assert!(!tmp.exists(), "temp file must not remain after save");
        assert!(path.is_file());
    }

    #[test]
    fn hosted_data_urls_as_overrides() {
        let u = HostedDataUrls {
            gametora_data_url: Some("a".into()),
            tracker_data_url: None,
            icons_data_url: Some("c".into()),
        };
        assert_eq!(u.as_overrides(), [Some("a".into()), None, Some("c".into())]);
    }
}
