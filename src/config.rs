//! `honse-tracker.json` — everything this plugin remembers, in one file.
//!
//! Settings, panel positions and the song plan used to be three files. They are
//! one document now, with one owner, because three files in a folder you have
//! to go looking for is worse than one you can open and read top to bottom.
//!
//! # Why there has to be a single owner
//!
//! [`PluginConfig`] round-trips a whole document: it deserialises the file into
//! `T` and writes `T` back out. Two owners sharing one path would therefore
//! erase each other's sections on every save — which is the reason the split
//! existed in the first place. Merging the file only works if the *ownership*
//! merges too, so every writer goes through [`edit`] here and no module holds
//! its own handle to the file.
//!
//! # Writes are human-paced, not per-frame
//!
//! The worry with one document is that a frequent writer drags the rest along.
//! Nothing here writes per frame: a panel is saved when a drag ends or an arrow
//! key lands, a song when you press a key, a setting when you pick a menu item.
//! The whole document is a couple of kilobytes, and [`PluginConfig::save`] is a
//! temp-file-plus-rename, so the cost is a rounding error against the actions
//! that trigger it.
//!
//! # Migration
//!
//! The first run with no `honse-tracker.json` folds in whatever the three old
//! files held. They are left on disk rather than deleted — reverting to an
//! older build should find its settings where it left them, and nothing here
//! is worth removing someone's file over.

use std::sync::Mutex;

use honse_services::{HostedDataUrls, PluginConfig};
use serde::{Deserialize, Serialize};

use crate::song_plan::SongPlanFile;
use crate::ui::layout::LayoutSection;

/// The file name, and the three it replaced.
const FILE: &str = "honse-tracker.json";
const LEGACY_SETTINGS: &str = "honseTrackerConfig.json";
const LEGACY_LAYOUT: &str = "overlayLayout.json";
const LEGACY_SONG_PLAN: &str = "songPlan.json";

/// The old flat `honseTrackerConfig.json`, whose fields land in two sections of
/// the new document.
///
/// Kept as its own type rather than parsed loosely so the migration is a
/// compile-checked mapping and can be tested against a real old file — losing
/// someone's settings to a silent rename is the one failure this whole module
/// has to avoid.
#[derive(Debug, Default, Deserialize)]
struct LegacySettings {
    #[serde(default)]
    hosted_data: HostedDataUrls,
    #[serde(flatten)]
    settings: Settings,
}

/// The whole document.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HonseTrackerFile {
    /// Things you decide once.
    #[serde(default)]
    pub settings: Settings,
    /// Hosted-data URL overrides.
    #[serde(default)]
    pub hosted_data: HostedDataUrls,
    /// Where each panel sits, keyed by panel id.
    #[serde(default)]
    pub layout: LayoutSection,
    /// Which songs you are saving for.
    #[serde(default)]
    pub song_plan: SongPlanFile,
}

/// Settings proper: the things a menu item toggles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Drop race skill cut-ins so the race never stops for them. Off unless
    /// asked for: it is the one setting here that changes the game rather than
    /// reporting on it.
    #[serde(default)]
    pub skip_race_skill_cutins: bool,
    /// Write each finished Independent Training's server response to disk. On
    /// by default: it only reports, and the data is gone once the game has
    /// shown you its summary screen.
    #[serde(default = "yes")]
    pub save_idle_careers: bool,
    /// Where those files go. Empty means
    /// `%USERPROFILE%\Documents\SavedIdleCareers`; a relative path resolves
    /// under the user profile, never under the game folder.
    #[serde(default)]
    pub idle_career_dir: String,
}

/// serde needs a function for a default that is not `false`.
const fn yes() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            skip_race_skill_cutins: false,
            save_idle_careers: true,
            idle_career_dir: String::new(),
        }
    }
}

/// The one handle to the file. `None` until [`load`] runs, or when edge exposes
/// no base dir — in which case everything still works for the session and
/// nothing is persisted.
static CONFIG: Mutex<Option<PluginConfig<HonseTrackerFile>>> = Mutex::new(None);

/// Load the document, folding in the legacy files on a first run. Called once
/// from plugin init, before anything reads a setting.
pub fn load() {
    // Ask before loading: `PluginConfig::load` creates the file with defaults
    // when it is missing, which would erase the evidence that this is a first
    // run and silently drop the old files' contents.
    let fresh = !edge_sdk::Sdk::try_get()
        .and_then(|sdk| sdk.base_dir())
        .is_some_and(|base| base.join(FILE).exists());

    let Some(mut config) = PluginConfig::<HonseTrackerFile>::load(FILE) else {
        hlog_warn!(target: "training-tracker", "Config: no base dir; nothing will persist this session");
        return;
    };

    if fresh {
        if let Some(migrated) = migrate() {
            config.value = migrated;
            if let Err(e) = config.save() {
                hlog_warn!(target: "training-tracker", "Config: could not write the merged {FILE}: {e}");
            } else {
                hlog_info!(
                    target: "training-tracker",
                    "Config: merged the previous settings, layout and song plan into {}",
                    config.path().display()
                );
            }
        }
    }

    hlog_info!(target: "training-tracker", "Config loaded from {}", config.path().display());
    *CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(config);
}

/// Read the three old files into one document, or `None` if none of them exist.
///
/// Each section is taken independently: a corrupt layout file must not cost you
/// your song plan, so a section that will not parse falls back to its default
/// and says so rather than failing the whole migration.
fn migrate() -> Option<HonseTrackerFile> {
    let base = edge_sdk::Sdk::try_get()?.base_dir()?;
    let mut found = Vec::new();
    let mut merged = HonseTrackerFile::default();

    /// Parse one legacy file, or default. Absent is silent; unreadable is not.
    fn section<T: Default + serde::de::DeserializeOwned>(
        path: &std::path::Path,
        found: &mut Vec<&'static str>,
        label: &'static str,
    ) -> T {
        if !path.exists() {
            return T::default();
        }
        found.push(label);
        match std::fs::read_to_string(path).map(|text| serde_json::from_str::<T>(&text)) {
            Ok(Ok(value)) => value,
            _ => {
                hlog_warn!(
                    target: "training-tracker",
                    "Config: could not read {} during migration; that section starts fresh",
                    path.display()
                );
                T::default()
            }
        }
    }

    let legacy: LegacySettings = section(&base.join(LEGACY_SETTINGS), &mut found, LEGACY_SETTINGS);
    merged.hosted_data = legacy.hosted_data;
    merged.settings = legacy.settings;
    merged.layout = section(&base.join(LEGACY_LAYOUT), &mut found, LEGACY_LAYOUT);
    merged.song_plan = section(&base.join(LEGACY_SONG_PLAN), &mut found, LEGACY_SONG_PLAN);

    if found.is_empty() {
        return None; // genuinely a new install
    }
    hlog_info!(target: "training-tracker", "Config: migrating {}", found.join(", "));
    Some(merged)
}

/// Run `f` against the document, falling back to defaults when it is not
/// loaded. Never blocks on I/O.
pub fn read<R>(f: impl FnOnce(&HonseTrackerFile) -> R) -> R {
    let guard = CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.as_ref() {
        Some(config) => f(&config.value),
        None => f(&HonseTrackerFile::default()),
    }
}

/// Mutate the document and write it straight back.
///
/// Saving on every edit rather than at shutdown: a crash mid-career must not
/// cost you a plan or a layout, and the file is small enough that the choice
/// costs nothing.
pub fn edit(f: impl FnOnce(&mut HonseTrackerFile)) {
    let mut guard = CONFIG.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(config) = guard.as_mut() else {
        return; // no base dir: the change applies this session and is not stored
    };
    f(&mut config.value);
    if let Err(e) = config.save() {
        hlog_warn!(target: "training-tracker", "Config: save failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::{HonseTrackerFile, Settings};

    /// The one setting that changes the game stays off by default; the one that
    /// only records stays on. Getting these backwards would either patch the
    /// game uninvited or quietly drop data the player cannot get back.
    #[test]
    fn defaults_are_report_on_modify_off() {
        let settings = Settings::default();
        assert!(!settings.skip_race_skill_cutins);
        assert!(settings.save_idle_careers);
        assert!(settings.idle_career_dir.is_empty());
    }

    /// Every section is `#[serde(default)]`, so a hand-edited file that drops
    /// one — or an older file written before it existed — still loads.
    #[test]
    fn a_partial_document_loads() {
        let file: HonseTrackerFile =
            serde_json::from_str(r#"{"settings":{"skip_race_skill_cutins":true}}"#).expect("partial document");
        assert!(file.settings.skip_race_skill_cutins);
        assert!(file.settings.save_idle_careers, "an absent field takes its default");
        assert!(file.layout.panels.is_empty());
    }

    #[test]
    fn an_empty_document_is_all_defaults() {
        let file: HonseTrackerFile = serde_json::from_str("{}").expect("empty document");
        assert!(!file.settings.skip_race_skill_cutins);
        assert!(file.settings.save_idle_careers);
    }

    /// The real `honseTrackerConfig.json` this replaced. A rename that quietly
    /// dropped someone's choices would be the worst outcome here, so the exact
    /// old shape is pinned.
    #[test]
    fn the_old_settings_file_migrates() {
        let legacy: super::LegacySettings =
            serde_json::from_str(r#"{"hosted_data":{},"skip_race_skill_cutins":true}"#).expect("old file");
        assert!(legacy.settings.skip_race_skill_cutins, "the choice survives");
        assert!(
            legacy.settings.save_idle_careers,
            "a field it predates takes its default"
        );
        assert!(legacy.settings.idle_career_dir.is_empty());
    }

    /// An old file from before either idle field existed still loads.
    #[test]
    fn the_oldest_settings_file_migrates() {
        let legacy: super::LegacySettings = serde_json::from_str("{}").expect("empty old file");
        assert!(!legacy.settings.skip_race_skill_cutins);
        assert!(legacy.settings.save_idle_careers);
    }

    /// The document round-trips: what `edit` writes is what the next launch
    /// reads, sections it does not touch included.
    #[test]
    fn the_document_round_trips() {
        let mut file = HonseTrackerFile::default();
        file.settings.skip_race_skill_cutins = true;
        file.settings.idle_career_dir = "D:\\runs".to_string();
        let json = serde_json::to_string(&file).expect("serialise");
        let back: HonseTrackerFile = serde_json::from_str(&json).expect("deserialise");
        assert!(back.settings.skip_race_skill_cutins);
        assert_eq!(back.settings.idle_career_dir, "D:\\runs");
    }
}
