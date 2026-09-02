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
//! The `song_plan` section of `honse-tracker.json`. It used to be its own file,
//! because `PluginConfig` round-trips a whole document and two owners sharing
//! one path would erase each other on save. `crate::config` is that single
//! owner now, so the split is no longer needed.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::song_catalog::{self, TokenVector};

/// On-disk shape. Only explicit choices are stored.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SongPlanFile {
    /// `song id -> planned`. A song absent here follows the guide default.
    #[serde(default)]
    choices: BTreeMap<String, bool>,
    /// Songs marked bought by hand, for when the game's own list cannot be
    /// read. Unioned with what the reader detects, never subtracted from it —
    /// so detection improving later can only add, and a stale mark costs a
    /// keypress rather than a wrong total.
    #[serde(default)]
    bought: BTreeMap<String, bool>,
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

    /// Whether a song was marked bought by hand.
    #[must_use]
    pub fn is_marked_bought(&self, song_id: &str) -> bool {
        self.bought.get(song_id).copied().unwrap_or(false)
    }

    /// Flip a song's hand-marked bought state.
    pub fn toggle_bought(&mut self, song_id: &str) {
        let next = !self.is_marked_bought(song_id);
        self.bought.insert(song_id.to_owned(), next);
    }

    /// Forget every choice in one window, returning it to guide defaults.
    /// Hand-marked purchases survive: they record what happened in the run,
    /// not what you intend, and a plan reset should not un-buy anything.
    pub fn reset_window(&mut self, window: u8) {
        for song in song_catalog::songs_in_window(window) {
            self.choices.remove(song.id);
        }
    }

    /// Songs still to buy in `scope`: planned, not yet owned.
    ///
    /// Everything else here is derived from this list, so a total can never
    /// disagree with the songs shown beside it.
    #[must_use]
    pub fn outstanding(&self, scope: Scope, owned: &Owned) -> Vec<&'static song_catalog::Song> {
        scope
            .songs()
            .filter(|s| self.is_planned(s.id) && !owned.has(s.id))
            .collect()
    }

    /// What the plan in `scope` **still** costs.
    ///
    /// This is the ledger. A song you have bought contributes nothing further,
    /// so the shortfall counts down as you spend rather than standing still.
    #[must_use]
    pub fn remaining_cost(&self, scope: Scope, owned: &Owned) -> TokenVector {
        sum_costs(&self.outstanding(scope, owned))
    }

    /// Combined cost of everything planned in `scope`, ignoring ownership.
    #[must_use]
    pub fn planned_cost(&self, scope: Scope) -> TokenVector {
        self.remaining_cost(scope, &Owned::default())
    }

    /// How many songs are planned in `scope`, owned or not.
    #[must_use]
    pub fn planned_count(&self, scope: Scope) -> usize {
        scope.songs().filter(|s| self.is_planned(s.id)).count()
    }

    /// How many planned songs in `scope` are already bought.
    #[must_use]
    pub fn owned_count(&self, scope: Scope, owned: &Owned) -> usize {
        scope
            .songs()
            .filter(|s| self.is_planned(s.id) && owned.has(s.id))
            .count()
    }
}

/// Which concerts a readout covers.
///
/// Songs planned but never bought are assumed to stay purchasable, so anything
/// reporting what you still owe uses [`Scope::Through`] — dropping last
/// concert's debt the moment the cap rises would under-report, which is the
/// worse direction for a panel whose job is saying what to save for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// One concert, in isolation. What the planner edits.
    Concert(u8),
    /// This concert and every one before it. What you actually still owe.
    Through(u8),
}

impl Scope {
    fn songs(self) -> impl Iterator<Item = &'static song_catalog::Song> {
        let windows = match self {
            Self::Concert(w) => w..=w,
            Self::Through(w) => 1..=w,
        };
        windows.flat_map(song_catalog::songs_in_window)
    }
}

fn sum_costs(songs: &[&'static song_catalog::Song]) -> TokenVector {
    let mut total = [0; 5];
    for song in songs {
        for (slot, cost) in total.iter_mut().zip(song.cost) {
            *slot += cost;
        }
    }
    total
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

    /// Add songs marked bought by hand.
    #[must_use]
    fn plus(mut self, ids: impl IntoIterator<Item = &'static str>) -> Self {
        self.ids.extend(ids);
        self.ids.sort_unstable();
        self.ids.dedup();
        self
    }

    #[must_use]
    pub fn has(&self, song_id: &str) -> bool {
        self.ids.contains(&song_id)
    }
}

/// Report what the plan starts the session with. The plan itself lives in
/// [`crate::config`]; this runs once that has loaded.
pub fn log_loaded() {
    let counts: Vec<String> = (1..=4)
        .map(|w| with_plan(|p| p.planned_count(Scope::Concert(w))).to_string())
        .collect();
    hlog_info!(
        target: "training-tracker",
        "Song plan — planned per concert: {}",
        counts.join("/")
    );
}

/// Run `f` against the plan, falling back to guide defaults when unloaded.
fn with_plan<R>(f: impl FnOnce(&SongPlanFile) -> R) -> R {
    crate::config::read(|file| f(&file.song_plan))
}

/// Mutate the plan and write it straight back to disk.
///
/// Saving on every edit rather than at shutdown: a crash mid-career must not
/// cost you the plan, and the file is a few hundred bytes.
fn edit(f: impl FnOnce(&mut SongPlanFile)) {
    crate::config::edit(|file| f(&mut file.song_plan));
}

#[must_use]
pub fn is_planned(song_id: &str) -> bool {
    with_plan(|p| p.is_planned(song_id))
}

#[must_use]
pub fn is_marked_bought(song_id: &str) -> bool {
    with_plan(|p| p.is_marked_bought(song_id))
}

pub fn toggle_bought(song_id: &str) {
    edit(|p| p.toggle_bought(song_id));
}

/// Everything the run owns: what the reader detected, plus your hand marks.
///
/// The single place ownership is decided, so no panel can disagree with
/// another about whether a song is bought.
#[must_use]
pub fn owned_from<'a>(detected: impl IntoIterator<Item = &'a str>) -> Owned {
    let marked: Vec<&'static str> = with_plan(|p| {
        song_catalog::SONGS
            .iter()
            .filter(|s| p.is_marked_bought(s.id))
            .map(|s| s.id)
            .collect()
    });
    Owned::from_names(detected).plus(marked)
}

#[must_use]
pub fn remaining_cost(scope: Scope, owned: &Owned) -> TokenVector {
    with_plan(|p| p.remaining_cost(scope, owned))
}

#[must_use]
pub fn planned_count(scope: Scope) -> usize {
    with_plan(|p| p.planned_count(scope))
}

#[must_use]
pub fn owned_count(scope: Scope, owned: &Owned) -> usize {
    with_plan(|p| p.owned_count(scope, owned))
}

#[must_use]
pub fn outstanding(scope: Scope, owned: &Owned) -> Vec<&'static song_catalog::Song> {
    with_plan(|p| p.outstanding(scope, owned))
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
        assert_eq!(plan.planned_count(Scope::Concert(1)), song_catalog::planned_count(1));
        assert_eq!(plan.planned_cost(Scope::Concert(1)), song_catalog::planned_cost(1));
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
        let before = plan.planned_cost(Scope::Concert(1));
        plan.toggle("go-this-way"); // Vo21 Co21
        let after = plan.planned_cost(Scope::Concert(1));
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
        let full = plan.planned_cost(Scope::Concert(1));
        // Here Comes Our Time is Vo32 Co12 and planned by default.
        let owned = Owned::from_names(["Here Comes Our Time"]);
        let remaining = plan.remaining_cost(Scope::Concert(1), &owned);
        assert_eq!(remaining[2], full[2] - 32);
        assert_eq!(remaining[4], full[4] - 12);
        assert_eq!(plan.owned_count(Scope::Concert(1), &owned), 1);
    }

    #[test]
    fn outstanding_lists_exactly_what_remaining_cost_sums() {
        let plan = SongPlanFile::default();
        let owned = Owned::from_names(["Here Comes Our Time"]);
        let outstanding = plan.outstanding(Scope::Concert(1), &owned);

        assert!(
            !outstanding.iter().any(|s| s.id == "here-comes-our-time"),
            "a bought song is not outstanding"
        );
        assert!(
            !outstanding.iter().any(|s| s.id == "go-this-way"),
            "a skipped song is not outstanding"
        );

        let mut summed = [0; 5];
        for song in &outstanding {
            for (slot, cost) in summed.iter_mut().zip(song.cost) {
                *slot += cost;
            }
        }
        assert_eq!(
            summed,
            plan.remaining_cost(Scope::Concert(1), &owned),
            "list and total must agree"
        );
    }

    /// A song planned in concert 1 and never bought is still owed in concert 2.
    /// Scoping the footer to the current window alone would drop that debt the
    /// moment the cap rose, and quietly under-report what is left to save for.
    #[test]
    fn unbought_songs_carry_over_into_later_concerts() {
        let plan = SongPlanFile::default();
        let nothing_owned = Owned::default();

        let concert_2_only = plan.remaining_cost(Scope::Concert(2), &nothing_owned);
        let through_2 = plan.remaining_cost(Scope::Through(2), &nothing_owned);
        let concert_1 = plan.remaining_cost(Scope::Concert(1), &nothing_owned);

        assert_eq!(
            through_2,
            std::array::from_fn(|i| concert_1[i] + concert_2_only[i]),
            "through 2 must be concerts 1 and 2 combined"
        );
        assert!(
            plan.outstanding(Scope::Through(2), &nothing_owned)
                .iter()
                .any(|s| s.window == 1),
            "a concert-1 song is still outstanding during concert 2"
        );
    }

    #[test]
    fn buying_a_carried_over_song_clears_it() {
        let plan = SongPlanFile::default();
        let owned = Owned::from_names(["Here Comes Our Time"]); // concert 1, Vo32 Co12
        let before = plan.remaining_cost(Scope::Through(3), &Owned::default());
        let after = plan.remaining_cost(Scope::Through(3), &owned);
        assert_eq!(after[2], before[2] - 32);
        assert_eq!(after[4], before[4] - 12);
    }

    #[test]
    fn the_first_concert_is_the_same_either_way() {
        let plan = SongPlanFile::default();
        let owned = Owned::default();
        assert_eq!(
            plan.remaining_cost(Scope::Concert(1), &owned),
            plan.remaining_cost(Scope::Through(1), &owned)
        );
    }

    #[test]
    fn owning_a_song_you_skipped_changes_nothing() {
        let plan = SongPlanFile::default();
        let owned = Owned::from_names(["Go This Way"]); // guide skips it
        assert_eq!(
            plan.remaining_cost(Scope::Concert(1), &owned),
            plan.planned_cost(Scope::Concert(1))
        );
        assert_eq!(plan.owned_count(Scope::Concert(1), &owned), 0);
    }

    #[test]
    fn owning_everything_leaves_nothing_to_save_for() {
        let plan = SongPlanFile::default();
        let names: Vec<&str> = song_catalog::songs_in_window(1).map(|s| s.name).collect();
        let owned = Owned::from_names(names);
        assert_eq!(plan.remaining_cost(Scope::Concert(1), &owned), [0; 5]);
    }

    #[test]
    fn unrecognised_song_names_are_dropped_not_guessed() {
        let with_junk = Owned::from_names(["Here Comes Our Time", "Some Unreleased Song"]);
        assert_eq!(with_junk, Owned::from_names(["Here Comes Our Time"]));
        assert!(with_junk.has("here-comes-our-time"));
    }

    #[test]
    fn a_hand_marked_song_counts_as_bought() {
        let mut plan = SongPlanFile::default();
        assert!(!plan.is_marked_bought("here-comes-our-time"));
        plan.toggle_bought("here-comes-our-time");
        assert!(plan.is_marked_bought("here-comes-our-time"));
        plan.toggle_bought("here-comes-our-time");
        assert!(!plan.is_marked_bought("here-comes-our-time"), "toggles back off");
    }

    /// Resetting a concert's plan is about intent. What you already bought is
    /// history, and un-buying it would silently inflate the shortfall.
    #[test]
    fn resetting_a_window_does_not_un_buy_anything() {
        let mut plan = SongPlanFile::default();
        plan.toggle_bought("go-this-way");
        plan.toggle("go-this-way");
        plan.reset_window(1);
        assert!(!plan.is_planned("go-this-way"), "plan is back to the guide default");
        assert!(plan.is_marked_bought("go-this-way"), "the purchase survives");
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
