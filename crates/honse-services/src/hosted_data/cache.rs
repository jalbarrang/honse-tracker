//! On-disk cache manifest tracking the last-synced hash per snapshot filename.
//!
//! Ported from fork `hosted_data/cache.rs` (path-sanitize tests included).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Persisted record of the last successful sync: content hash per filename.
#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct CacheManifest {
    #[serde(default)]
    pub synced_at: String,
    #[serde(default)]
    pub files: HashMap<String, String>,
}

/// Reject filenames that could escape the cache dir or nest into subdirs.
pub fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !std::path::Path::new(name).is_absolute()
}

/// Accept a `/`-separated relative path for nested sets, rejecting traversal.
pub fn is_safe_relpath(name: &str) -> bool {
    if name.is_empty() || name.contains('\\') || std::path::Path::new(name).is_absolute() {
        return false;
    }
    let mut any = false;
    for comp in name.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return false;
        }
        any = true;
    }
    any
}

#[cfg(test)]
mod tests {
    use super::{is_safe_filename, is_safe_relpath};

    #[test]
    fn flat_names_stay_flat() {
        assert!(is_safe_filename("skills.json"));
        assert!(!is_safe_filename("chara/x.png"));
        assert!(!is_safe_filename(".."));
    }

    #[test]
    fn relpath_allows_nested_rejects_traversal() {
        assert!(is_safe_relpath("10011.png"));
        assert!(is_safe_relpath("chara/chr_icon_1001.png"));
        assert!(is_safe_relpath("statusrank/ui_statusrank_08.png"));
        assert!(!is_safe_relpath("../secret"));
        assert!(!is_safe_relpath("chara/../../etc/passwd"));
        assert!(!is_safe_relpath("a//b.png"));
        assert!(!is_safe_relpath("chara\\x.png"));
        assert!(!is_safe_relpath("/abs/path.png"));
        assert!(!is_safe_relpath(""));
        assert!(!is_safe_relpath("."));
        assert!(!is_safe_relpath("trailing/"));
    }
}
