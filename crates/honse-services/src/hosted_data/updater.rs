//! Hosted-data sync orchestration: hash-diff, download changed, persist cache.
//!
//! Ported from fork `hosted_data/updater.rs`. Notifications go through
//! edge-sdk `Sdk::show_notification`. Data-dir root comes from edge-sdk
//! `data_path` / `hachimi_get_data_path`.

use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use edge_sdk::Sdk;

use super::{
    cache::{is_safe_filename, is_safe_relpath, CacheManifest},
    client::{self, FetchError, Fetcher, UreqFetcher},
    DataSet,
};

/// Pure hash-diff: which manifest files need (re)download.
///
/// Returns `(filename, remote_hash)` pairs. Used by tests without touching disk
/// beyond the provided `file_exists` predicate.
pub fn files_needing_fetch(
    remote: &[(String, String)],
    cache: &CacheManifest,
    allow_subdirs: bool,
    file_exists: &dyn Fn(&str) -> bool,
) -> Vec<(String, String)> {
    let mut pending = Vec::new();
    for (file, remote_hash) in remote {
        let safe = if allow_subdirs {
            is_safe_relpath(file)
        } else {
            is_safe_filename(file)
        };
        if !safe {
            continue;
        }
        let cached_hash = cache.files.get(file);
        let needs = cached_hash.is_none_or(|h| h != remote_hash) || !file_exists(file);
        if needs {
            pending.push((file.clone(), remote_hash.clone()));
        }
    }
    pending
}

pub struct Updater {
    set: &'static DataSet,
    url_override: Option<String>,
    sync_mutex: Mutex<()>,
}

impl Updater {
    pub fn new(set: &'static DataSet, url_override: Option<String>) -> Self {
        Self {
            set,
            url_override,
            sync_mutex: Mutex::new(()),
        }
    }

    /// Spawn a background sync with the production [`UreqFetcher`].
    pub fn sync(self: Arc<Self>, notify: bool) {
        let log_target = self.set.log_target;
        let _ = std::thread::Builder::new()
            .name(format!("{log_target}_sync"))
            .spawn(move || {
                let fetcher = UreqFetcher;
                if let Err(e) = self.sync_with_fetcher(&fetcher, notify) {
                    log::warn!(target: log_target, "data sync failed: {e}");
                    if notify {
                        Self::notify(&format!("{} sync failed: {e}", self.set.log_target));
                    }
                }
            });
    }

    /// Sync using an injected fetcher (tests + production).
    pub fn sync_with_fetcher(&self, fetcher: &dyn Fetcher, notify: bool) -> Result<usize, FetchError> {
        let Ok(_guard) = self.sync_mutex.try_lock() else {
            return Ok(0);
        };
        let set = self.set;
        let log_target = set.log_target;

        let base = self.url_override.as_deref().unwrap_or(set.default_url);

        let data_dir = resolve_data_dir(set.subdir)
            .ok_or_else(|| FetchError::Io("host data path unavailable (Sdk not initialized?)".into()))?;
        let cache_path = data_dir.join(set.cache_filename);

        let mut cache: CacheManifest = if fs::metadata(&cache_path).is_ok() {
            serde_json::from_str(&fs::read_to_string(&cache_path).map_err(|e| FetchError::Io(e.to_string()))?)
                .unwrap_or_default()
        } else {
            CacheManifest::default()
        };

        log::info!(target: log_target, "Checking hosted-data manifest...");
        let manifest = client::load_manifest(fetcher, base)?;

        let remote: Vec<(String, String)> = manifest.files.into_iter().collect();
        let data_dir_for_exists = data_dir.clone();
        let pending = files_needing_fetch(&remote, &cache, set.allow_subdirs, &|file| {
            data_dir_for_exists.join(file).is_file()
        });

        if pending.is_empty() {
            log::info!(target: log_target, "hosted data already up to date");
            if notify {
                Self::notify(&format!("{} up to date", set.log_target));
            }
            return Ok(0);
        }

        fs::create_dir_all(&data_dir).map_err(|e| FetchError::Io(e.to_string()))?;
        log::info!(target: log_target, "Syncing {} snapshot(s)...", pending.len());
        if notify {
            Self::notify(&format!("Syncing {}…", set.log_target));
        }

        let mut updated = 0usize;
        for (file, remote_hash) in pending {
            let out_path = data_dir.join(&file);
            if set.allow_subdirs {
                if let Some(parent) = out_path.parent() {
                    if let Err(e) = fs::create_dir_all(parent) {
                        log::warn!(target: log_target, "Failed to create dir for '{file}': {e}");
                        continue;
                    }
                }
            }
            let fetched = if set.binary {
                client::fetch_snapshot_bytes(fetcher, base, &file)
            } else {
                client::fetch_snapshot(fetcher, base, &file).map(String::into_bytes)
            };
            match fetched {
                Ok(bytes) => {
                    // Optional integrity check when hash looks like blake3 hex.
                    if remote_hash.len() == 64 {
                        let actual = blake3::hash(&bytes).to_hex().to_string();
                        if actual != *remote_hash {
                            log::warn!(
                                target: log_target,
                                "hash mismatch for '{file}': expected {remote_hash}, got {actual}"
                            );
                            continue;
                        }
                    }
                    fs::write(&out_path, bytes).map_err(|e| FetchError::Io(e.to_string()))?;
                    cache.files.insert(file.clone(), remote_hash);
                    updated += 1;
                }
                Err(e) => {
                    log::warn!(target: log_target, "Failed to fetch '{file}': {e}");
                }
            }
        }

        if updated > 0 {
            cache.synced_at = chrono::Utc::now().to_rfc3339();
            write_json_atomic(&cache_path, &cache)?;
            log::info!(target: log_target, "hosted data sync complete ({updated} updated)");
        }
        if notify {
            Self::notify(&format!("{} sync complete ({updated} updated)", set.log_target));
        }
        Ok(updated)
    }

    fn notify(message: &str) {
        if let Some(sdk) = Sdk::try_get() {
            let _ = sdk.show_notification(message);
        }
    }
}

fn resolve_data_dir(subdir: &str) -> Option<PathBuf> {
    let sdk = Sdk::try_get()?;
    if subdir.is_empty() {
        // data_path("") joins "" → data root; but our sanitizer rejects empty?
        // Use data_path(".") then parent, or call get_data_path via empty join.
        // edge-sdk data_path rejects ".." but "" has no root — Path::new("").join works.
        // Safer: resolve a sentinel then take parent.
        let probe = sdk.data_path("_")?;
        probe.parent().map(Path::to_path_buf)
    } else {
        sdk.data_path(subdir)
    }
}

fn write_json_atomic(path: &Path, value: &impl serde::Serialize) -> Result<(), FetchError> {
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(value).map_err(|e| FetchError::Json(e.to_string()))?;
    fs::write(&tmp, text).map_err(|e| FetchError::Io(e.to_string()))?;
    fs::rename(&tmp, path).map_err(|e| FetchError::Io(e.to_string()))?;
    Ok(())
}

/// Test-only sync against an explicit data directory (no Sdk required).
#[cfg(test)]
pub fn sync_to_dir(set: &DataSet, data_dir: &Path, base_url: &str, fetcher: &dyn Fetcher) -> Result<usize, FetchError> {
    let cache_path = data_dir.join(set.cache_filename);
    let mut cache: CacheManifest = if cache_path.is_file() {
        serde_json::from_str(&fs::read_to_string(&cache_path).map_err(|e| FetchError::Io(e.to_string()))?)
            .unwrap_or_default()
    } else {
        CacheManifest::default()
    };

    let manifest = client::load_manifest(fetcher, base_url)?;
    let remote: Vec<(String, String)> = manifest.files.into_iter().collect();
    let pending = files_needing_fetch(&remote, &cache, set.allow_subdirs, &|file| {
        data_dir.join(file).is_file()
    });
    if pending.is_empty() {
        return Ok(0);
    }
    fs::create_dir_all(data_dir).map_err(|e| FetchError::Io(e.to_string()))?;
    let mut updated = 0usize;
    for (file, remote_hash) in pending {
        let out_path = data_dir.join(&file);
        if set.allow_subdirs {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent).map_err(|e| FetchError::Io(e.to_string()))?;
            }
        }
        let bytes = if set.binary {
            client::fetch_snapshot_bytes(fetcher, base_url, &file)?
        } else {
            client::fetch_snapshot(fetcher, base_url, &file)?.into_bytes()
        };
        fs::write(&out_path, &bytes).map_err(|e| FetchError::Io(e.to_string()))?;
        cache.files.insert(file, remote_hash);
        updated += 1;
    }
    if updated > 0 {
        cache.synced_at = "test".into();
        write_json_atomic(&cache_path, &cache)?;
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex as StdMutex;

    use crate::hosted_data::{HostedManifest, GAMETORA, TRACKER};

    struct MemFetcher {
        map: StdMutex<HashMap<String, Vec<u8>>>,
    }

    impl MemFetcher {
        fn new(entries: HashMap<String, Vec<u8>>) -> Self {
            Self {
                map: StdMutex::new(entries),
            }
        }
    }

    impl Fetcher for MemFetcher {
        fn get(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.map
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::Http(format!("missing {url}")))
        }
    }

    #[test]
    fn hash_diff_unchanged_skipped_changed_selected_missing_refetched() {
        let mut cache = CacheManifest::default();
        cache.files.insert("a.json".into(), "hash_a".into());
        cache.files.insert("b.json".into(), "hash_b_old".into());

        let remote = vec![
            ("a.json".into(), "hash_a".into()),     // unchanged + exists → skip
            ("b.json".into(), "hash_b_new".into()), // changed → fetch
            ("c.json".into(), "hash_c".into()),     // missing from cache → fetch
            ("d.json".into(), "hash_d".into()),     // cached but file missing → fetch
        ];
        let mut cache2 = cache.clone();
        cache2.files.insert("d.json".into(), "hash_d".into());

        let exists = |f: &str| f != "d.json" && f != "c.json";
        let pending = files_needing_fetch(&remote, &cache2, false, &exists);
        let names: Vec<_> = pending.iter().map(|(n, _)| n.as_str()).collect();
        assert!(!names.contains(&"a.json"));
        assert!(names.contains(&"b.json"));
        assert!(names.contains(&"c.json"));
        assert!(names.contains(&"d.json"));
    }

    #[test]
    fn sync_via_memory_fetcher_and_tempdir() {
        let dir = tempfile::tempdir().unwrap();
        let body = br#"{"ok":true}"#;
        let hash = blake3::hash(body).to_hex().to_string();
        let manifest = format!(r#"{{"generated_at":"t","source":"test","files":{{"skills.json":"{hash}"}}}}"#);
        let base = "mem://data";
        let mut map = HashMap::new();
        map.insert(format!("{base}/manifest.json"), manifest.into_bytes());
        map.insert(format!("{base}/skills.json"), body.to_vec());
        let fetcher = MemFetcher::new(map);

        let n = sync_to_dir(&GAMETORA, dir.path(), base, &fetcher).unwrap();
        assert_eq!(n, 1);
        assert!(dir.path().join("skills.json").is_file());

        // Second sync: unchanged → 0.
        let n2 = sync_to_dir(&GAMETORA, dir.path(), base, &fetcher).unwrap();
        assert_eq!(n2, 0);
    }

    #[test]
    fn manifest_round_trip() {
        let m = HostedManifest {
            generated_at: Some("2024-01-01".into()),
            source: Some("test".into()),
            files: HashMap::from([("a.json".into(), "abc".into())]),
        };
        // Re-parse via serde to confirm shape.
        let json = serde_json::to_string(&serde_json::json!({
            "generated_at": m.generated_at,
            "source": m.source,
            "files": m.files,
        }))
        .unwrap();
        let parsed: client::HostedManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.files.get("a.json").map(String::as_str), Some("abc"));
        let _ = TRACKER; // keep import used
    }
}
