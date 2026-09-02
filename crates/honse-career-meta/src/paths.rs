//! Where the plugin puts things, as pure functions of a home directory.
//!
//! The plugin writes exports and the viewer reads them; if the two ever
//! disagreed about the default location, the viewer would open to an empty
//! list with no hint why. So the join lives here, once, and both take a home
//! directory in rather than reading the environment themselves — which keeps
//! this crate free of I/O and lets the rule be tested.

use std::path::{Path, PathBuf};

/// `<home>\Documents\SavedIdleCareers` — the default export directory.
#[must_use]
pub fn saved_careers_dir(home: &Path) -> PathBuf {
    home.join("Documents").join("SavedIdleCareers")
}

#[cfg(test)]
mod tests {
    use super::saved_careers_dir;
    use std::path::Path;

    #[test]
    fn exports_land_under_documents() {
        let dir = saved_careers_dir(Path::new(r"C:\Users\juan"));
        assert!(dir.ends_with(Path::new("Documents").join("SavedIdleCareers")));
        assert!(dir.starts_with(r"C:\Users\juan"));
    }
}
