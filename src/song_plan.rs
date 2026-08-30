//! Which songs *you* intend to buy, persisted across sessions.
//!
//! [`crate::song_catalog`] knows what every song costs and what uma.guide would
//! take. This module knows what you decided, which is only ever a set of
//! overrides on top of that.
//!
//! # Why overrides rather than a stored set
//!
//! Storing "the planned songs" would mean a song added to the catalogue later
//! is silently absent from every existing plan — skipped by omission, with no
//! way to tell that apart from a deliberate skip. Storing only the choices you
//! actually made lets a new song arrive with the guide's default, which is the
//! behaviour you would expect.
//!
//! # File
//!
//! `songPlan.json` in edge's base dir, beside `honseTrackerConfig.json`. It is
//! deliberately its own file: `PluginConfig` round-trips a whole document, so
//! two configs sharing one file would drop each other's fields on save.

use std::collections::BTreeMap;
use std::sync::Mutex;

use honse_services::PluginConfig;
use serde::{Deserialize, Serialize};

use crate::song_catalog::{self, TokenVector};

/// On-disk shape. Only explicit choices are stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SongPlanFile {
    /// `song id -> planned`. A song absent here follows the guide default.
    #[serde(default)]
    choices: BTreeMap<String, bool>,
}

impl SongPlanFile {
    /// Whether a song is planned: your choice if you made one, the guide's
    /// otherwise. An id not in the catalogue is never planned.
    #[must_use]
    pub fn is_planned(&self, song_id: &str) -> bool {
        if let Some(&chosen) = self.choices.get(song_id) {
            return chosen;
        }
        song_catalog::SONGS
            .iter()
            .find(|s| s.id == song_id)
            .is_some_and(|s| s.planned_by_default)
    }

    /// Flip one song, recording it as an explicit choice.
    pub fn toggle(&mut self, song_id: &str) {
        let next = !self.is_planned(song_id);
        self.choices.insert(song_id.to_owned(), next);
    }

    /// Forget every choice in one window, returning it to guide defaults.
    pub fn reset_window(&mut self, window: u8) {
        for song in song_catalog::songs_in_window(window) {
            self.choices.remove(song.id);
        }
    }

    /// Combined cost of the songs planned in one window, ignoring ownership.
    #[must_use]
    pub fn planned_cost(&self, window: u8) -> TokenVector {
        self.remaining_cost(window, &Owned::default())
    }

    /// What the plan for one window **still** costs: planned songs you do not
    /// already own.
    ///
    /// This is the ledger. A song you have bought contributes nothing further,
    /// so the shortfall counts down as you spend rather than standing still.
    #[must_use]
    pub fn remaining_cost(&self, window: u8, owned: &Owned) -> TokenVector {
        let mut total = [0; 5];
        for song in song_catalog::songs_in_window(window)
            .filter(|s| self.is_planned(s.id) && !owned.has(s.id))
        {
            for (slot, cost) in total.iter_mut().zip(song.cost) {
                *slot += cost;
            }
        }
        total
    }

    /// How many songs are planned in one window, owned or not.
    #[must_use]
    pub fn planned_count(&self, window: u8) -> usize {
        song_catalog::songs_in_window(window)
            .filter(|s| self.is_planned(s.id))
            .count()
    }

    /// How many planned songs in one window are already bought.
    #[must_use]
    pub fn owned_count(&self, window: u8, owned: &Owned) -> usize {
        song_catalog::songs_in_window(window)
            .filter(|s| self.is_planned(s.id) && owned.has(s.id))
            .count()
    }
}

/// Catalogue songs the run has already learned.
///
/// Built by matching the game's `get_MusicName()` against catalogue names. A
/// name the catalogue does not know is dropped rather than guessed at — it is
/// logged once by the reader so the mismatch is fixable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Owned {
    ids: Vec<&'static str>,
}

impl Owned {
    /// Resolve game-reported song names to catalogue ids.
    #[must_use]
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut ids: Vec<&'static str> = names
            .into_iter()
            .filter_map(|n| song_catalog::song_by_name(n).map(|s| s.id))
            .collect();
        ids.sort_unstable();
        ids.dedup();
        Self { ids }
    }

    #[must_use]
    pub fn has(&self, song_id: &str) -> bool {
        self.ids.contains(&song_id)
    }
}

/// The loaded plan. `None` until [`load`] runs, or if edge has no base dir —
/// in which case choices still work for the session but are not persisted.
static PLAN: Mutex<Option<PluginConfig<SongPlanFile>>> = Mutex::new(None);

/// Load the plan from disk. Called once from plugin init.
pub fn load() {
    let Some(config) = PluginConfig::<SongPlanFile>::load("songPlan.json") else {
        hlog_warn!(target: "training-tracker", "Song plan: no base dir; choices will not persist");
        return;
    };
    let counts: Vec<String> = (1..=4).map(|w| config.value.planned_count(w).to_string()).collect();
    hlog_info!(
        target: "training-tracker",
        "Song plan loaded from {} — planned per concert: {}",
        config.path().display(),
        counts.join("/")
    );
    *PLAN.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(config);
}

/// Run `f` against the plan, falling back to guide defaults when unloaded.
fn with_plan<R>(f: impl FnOnce(&SongPlanFile) -> R) -> R {
    let guard = PLAN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.as_ref() {
        Some(config) => f(&config.value),
        None => f(&SongPlanFile::default()),
    }
}

/// Mutate the plan and write it straight back to disk.
///
/// Saving on every edit rather than at shutdown: a crash mid-career must not
/// cost you the plan, and the file is a few hundred bytes.
fn edit(f: impl FnOnce(&mut SongPlanFile)) {
    let mut guard = PLAN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(config) = guard.as_mut() else {
        return;
    };
    f(&mut config.value);
    if let Err(e) = config.save() {
        hlog_warn!(target: "training-tracker", "Song plan: save failed: {e}");
    }
}

#[must_use]
pub fn is_planned(song_id: &str) -> bool {
    with_plan(|p| p.is_planned(song_id))
}

#[must_use]
pub fn remaining_cost(window: u8, owned: &Owned) -> TokenVector {
    with_plan(|p| p.remaining_cost(window, owned))
}

#[must_use]
pub fn planned_count(window: u8) -> usize {
    with_plan(|p| p.planned_count(window))
}

#[must_use]
pub fn owned_count(window: u8, owned: &Owned) -> usize {
    with_plan(|p| p.owned_count(window, owned))
}

pub fn toggle(song_id: &str) {
    edit(|p| p.toggle(song_id));
}

pub fn reset_window(window: u8) {
    edit(|p| p.reset_window(window));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_plan_is_the_guide_plan() {
        let plan = SongPlanFile::default();
        assert_eq!(plan.planned_count(1), song_catalog::planned_count(1));
        assert_eq!(plan.planned_cost(1), song_catalog::planned_cost(1));
        assert!(plan.is_planned("here-comes-our-time"));
        assert!(!plan.is_planned("go-this-way"));
    }

    #[test]
    fn toggling_overrides_the_default_in_both_directions() {
        let mut plan = SongPlanFile::default();
        plan.toggle("go-this-way"); // guide skips it
        assert!(plan.is_planned("go-this-way"));
        plan.toggle("here-comes-our-time"); // guide plans it
        assert!(!plan.is_planned("here-comes-our-time"));
    }

    #[test]
    fn planning_a_skipped_song_adds_its_cost() {
        let mut plan = SongPlanFile::default();
        let before = plan.planned_cost(1);
        plan.toggle("go-this-way"); // Vo21 Co21
        let after = plan.planned_cost(1);
        assert_eq!(after[2], before[2] + 21);
        assert_eq!(after[4], before[4] + 21);
    }

    #[test]
    fn reset_forgets_choices_in_that_window_only() {
        let mut plan = SongPlanFile::default();
        plan.toggle("go-this-way"); // window 1
        plan.toggle("dream-sky"); // window 4
        plan.reset_window(1);
        assert!(!plan.is_planned("go-this-way"), "window 1 back to guide default");
        assert!(!plan.is_planned("dream-sky"), "window 4 choice survives");
    }

    #[test]
    fn owning_a_planned_song_removes_its_cost() {
        let plan = SongPlanFile::default();
        let full = plan.planned_cost(1);
        // Here Comes Our Time is Vo32 Co12 and planned by default.
        let owned = Owned::from_names(["Here Comes Our Time"]);
        let remaining = plan.remaining_cost(1, &owned);
        assert_eq!(remaining[2], full[2] - 32);
        assert_eq!(remaining[4], full[4] - 12);
        assert_eq!(plan.owned_count(1, &owned), 1);
    }

    #[test]
    fn owning_a_song_you_skipped_changes_nothing() {
        let plan = SongPlanFile::default();
        let owned = Owned::from_names(["Go This Way"]); // guide skips it
        assert_eq!(plan.remaining_cost(1, &owned), plan.planned_cost(1));
        assert_eq!(plan.owned_count(1, &owned), 0);
    }

    #[test]
    fn owning_everything_leaves_nothing_to_save_for() {
        let plan = SongPlanFile::default();
        let names: Vec<&str> = song_catalog::songs_in_window(1).map(|s| s.name).collect();
        let owned = Owned::from_names(names);
        assert_eq!(plan.remaining_cost(1, &owned), [0; 5]);
    }

    #[test]
    fn unrecognised_song_names_are_dropped_not_guessed() {
        let with_junk = Owned::from_names(["Here Comes Our Time", "Some Unreleased Song"]);
        assert_eq!(with_junk, Owned::from_names(["Here Comes Our Time"]));
        assert!(with_junk.has("here-comes-our-time"));
    }

    #[test]
    fn an_unknown_song_is_never_planned() {
        assert!(!SongPlanFile::default().is_planned("not-a-song"));
    }

    /// A song added to the catalogue later must arrive with the guide's
    /// default, not silently skipped because an old file does not mention it.
    #[test]
    fn songs_missing_from_the_file_follow_the_guide() {
        let plan: SongPlanFile = serde_json::from_str(r#"{"choices":{"go-this-way":true}}"#).expect("valid");
        assert!(plan.is_planned("go-this-way"), "stored choice honoured");
        assert!(plan.is_planned("here-comes-our-time"), "unmentioned song uses default");
        assert!(!plan.is_planned("ring-ring-diary"), "unmentioned skip uses default");
    }

    #[test]
    fn choices_round_trip_through_json() {
        let mut plan = SongPlanFile::default();
        plan.toggle("go-this-way");
        let text = serde_json::to_string(&plan).expect("serialize");
        let back: SongPlanFile = serde_json::from_str(&text).expect("deserialize");
        assert!(back.is_planned("go-this-way"));
    }
}
