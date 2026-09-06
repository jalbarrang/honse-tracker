//! Where the plugin puts things, as pure functions of a home directory.
//!
//! The plugin writes exports and the viewer reads them; if the two ever
//! disagreed about the default location, the viewer would open to an empty
//! list with no hint why. So the join lives here, once, and both take a home
//! directory in rather than reading the environment themselves — which keeps
//! this crate free of I/O and lets the rule be tested.
//!
//! # The layout
//!
//! ```text
//! <home>\Documents\honse-tracker\
//!     idle-careers\<career files>.json
//!     veterans.json
//! ```
//!
//! One folder with the plugin's name on it, because the plugin writes more than
//! one kind of file now and scattering them across `Documents` makes each one
//! harder to find than the last.

use std::path::{Path, PathBuf};

/// `<home>\Documents\honse-tracker` — everything the plugin writes lives here.
#[must_use]
pub fn honse_tracker_dir(home: &Path) -> PathBuf {
    home.join("Documents").join("honse-tracker")
}

/// `<home>\Documents\honse-tracker\idle-careers` — one file per finished run.
#[must_use]
pub fn idle_careers_dir(home: &Path) -> PathBuf {
    honse_tracker_dir(home).join("idle-careers")
}

/// `<home>\Documents\honse-tracker\veterans.json` — the whole trained-chara
/// list, rewritten on each export.
#[must_use]
pub fn veterans_file(home: &Path) -> PathBuf {
    honse_tracker_dir(home).join("veterans.json")
}

/// `<home>\Documents\SavedIdleCareers` — where careers landed before the move.
///
/// Nothing writes here any more and nothing moves what is in it: the viewer
/// reads it alongside the new folder so runs saved by an older build stay
/// visible, and deleting someone's exports to tidy a rename would be a poor
/// trade.
#[must_use]
pub fn legacy_saved_careers_dir(home: &Path) -> PathBuf {
    home.join("Documents").join("SavedIdleCareers")
}

#[cfg(test)]
mod tests {
    use super::{honse_tracker_dir, idle_careers_dir, legacy_saved_careers_dir, veterans_file};
    use std::path::Path;

    #[test]
    fn everything_lands_in_one_named_folder() {
        let home = Path::new(r"C:\Users\juan");
        let root = honse_tracker_dir(home);
        assert!(root.ends_with(Path::new("Documents").join("honse-tracker")));
        assert!(root.starts_with(home));
        assert_eq!(idle_careers_dir(home), root.join("idle-careers"));
        assert_eq!(veterans_file(home), root.join("veterans.json"));
    }

    #[test]
    fn the_old_folder_is_still_nameable() {
        let dir = legacy_saved_careers_dir(Path::new(r"C:\Users\juan"));
        assert!(dir.ends_with(Path::new("Documents").join("SavedIdleCareers")));
    }
}
