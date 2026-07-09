//! Hosted-data sync: download blake3-manifest snapshots into edge's data dir.
//!
//! Ported from fork `apps/hachimi/src/core/hosted_data/`. Three sets keep their
//! exact default URLs pointed at hachimi-redux. Config/base-URL override is
//! plumbed as `Option<String>` until t-006 lands a real config file.

mod cache;
mod client;
mod updater;

pub use cache::{is_safe_filename, is_safe_relpath, CacheManifest};
pub use client::{Fetcher, HostedManifest, UreqFetcher};
pub use updater::{files_needing_fetch, Updater};

use std::path::PathBuf;

use edge_sdk::Sdk;

/// Subdir for GameTora catalog snapshots (fork `GAMETORA_DATA_SUBDIR`).
pub const GAMETORA_DATA_SUBDIR: &str = "gametora";

/// Descriptor for one hosted data set.
pub struct DataSet {
    /// Log target (e.g. `"gametora_data"`).
    pub log_target: &'static str,
    /// Subdir under the game data dir where snapshots cache; `""` = data-dir root.
    pub subdir: &'static str,
    /// Filename of the local cache manifest within the cache dir.
    pub cache_filename: &'static str,
    /// Default hosted base URL (no trailing slash).
    pub default_url: &'static str,
    /// `true` for binary snapshots (icon PNGs).
    pub binary: bool,
    /// `true` if manifest keys may be `/`-separated relative paths.
    pub allow_subdirs: bool,
}

/// GameTora catalog snapshots (`data/gametora/`), cached under `gametora/`.
pub static GAMETORA: DataSet = DataSet {
    log_target: "gametora_data",
    subdir: GAMETORA_DATA_SUBDIR,
    cache_filename: ".gametora_cache.json",
    default_url: "https://raw.githubusercontent.com/jalbarrang/hachimi-redux/main/data/gametora",
    binary: false,
    allow_subdirs: false,
};

/// Training-tracker generated resources (`data/`), cached flat in the data-dir root.
pub static TRACKER: DataSet = DataSet {
    log_target: "tracker_data",
    subdir: "",
    cache_filename: ".tracker_cache.json",
    default_url: "https://raw.githubusercontent.com/jalbarrang/hachimi-redux/main/data",
    binary: false,
    allow_subdirs: false,
};

/// Career-panel icon sprites (`data/icons/**`), cached under `icons/`.
pub static ICONS: DataSet = DataSet {
    log_target: "icons_data",
    subdir: "icons",
    cache_filename: ".icons_cache.json",
    default_url: "https://raw.githubusercontent.com/jalbarrang/hachimi-redux/main/data/icons",
    binary: true,
    allow_subdirs: true,
};

/// Resolve `rel` under the host data path. Returns `None` if the path is unsafe,
/// the host data dir is unavailable, or the resolved path does not exist yet
/// (prefer downloaded copy; absent → None — HANDOFF / compat semantics).
#[must_use]
pub fn host_data_path(rel: &str) -> Option<PathBuf> {
    let sdk = Sdk::try_get()?;
    let path = sdk.data_path(rel)?;
    path.exists().then_some(path)
}

/// GameTora cache directory, if present on disk.
#[must_use]
pub fn gametora_data_dir() -> Option<PathBuf> {
    host_data_path(GAMETORA_DATA_SUBDIR)
}

/// Sync all three data sets on a background thread (post-game-initialized).
///
/// `url_overrides` is `(gametora, tracker, icons)` — each `None` uses the set's
/// default URL. Wired from t-006 config when that lands.
pub fn sync_all(url_overrides: [Option<String>; 3], notify: bool) {
    let sets = [
        (&GAMETORA, url_overrides[0].clone()),
        (&TRACKER, url_overrides[1].clone()),
        (&ICONS, url_overrides[2].clone()),
    ];
    for (set, url_override) in sets {
        let updater = std::sync::Arc::new(Updater::new(set, url_override));
        updater.sync(notify);
    }
}

#[cfg(test)]
mod tests {
    use super::{GAMETORA, ICONS, TRACKER};

    #[test]
    fn json_sets_are_flat_text() {
        for set in [&GAMETORA, &TRACKER] {
            assert!(!set.binary, "{} must fetch as text/JSON", set.log_target);
            assert!(!set.allow_subdirs, "{} must stay flat", set.log_target);
        }
    }

    #[test]
    fn icons_set_is_binary_nested() {
        assert!(ICONS.binary);
        assert!(ICONS.allow_subdirs);
        assert_eq!(ICONS.subdir, "icons");
    }

    #[test]
    fn default_urls_point_at_hachimi_redux() {
        assert!(GAMETORA.default_url.contains("jalbarrang/hachimi-redux"));
        assert!(TRACKER.default_url.ends_with("/main/data"));
        assert!(ICONS.default_url.ends_with("/main/data/icons"));
    }
}
