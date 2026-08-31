//! The Grand Live song catalogue and its concert-window arithmetic.
//!
//! Static data, no IL2CPP. Ported from the sister project's
//! `apps/web/src/lib/grand-live/calculator.ts`, itself transcribed from
//! uma.guide's Grand Live calculator.
//!
//! # Why static data rather than a live read
//!
//! The tree only ever offers a handful of squares at a time, so the game can
//! tell you what is available *now* but not what exists. Planning is the
//! opposite problem — you want to know in Junior year what a window-4 song will
//! cost. That only works from a catalogue known ahead of time.
//!
//! The trade is that this can drift from the game. Costs for songs actually on
//! the tree are read live in `memory_reader::scenario::grand_live`; when the
//! planner grows the ability to mark songs owned, cross-checking the two is the
//! natural way to catch drift.
//!
//! Each song's mastery effect and concert bonus were transcribed too — they are
//! not carried here because nothing shows them yet. They are in the sister
//! project if and when the full planner needs them.

/// Token amounts in `[Dance, Passion, Vocal, Visual, Composure]` order — the
/// same order as `PerformanceTokens::labelled`.
pub type TokenVector = [i32; 5];

/// Per-token ceiling for each concert, in order. The cap rises between them,
/// which is why nothing hardcodes 200.
///
/// The fifth tier is the closing Grand Concert. It raises the ceiling again but
/// offers **no new songs** — the catalogue has nothing in window 5 — so it is
/// purely a window in which to finish buying what earlier concerts offered.
/// Leaving it out made the concert readout vanish entirely at `cap 400`,
/// because the cap matched no known concert.
pub const CONCERT_CAPS: [i32; 5] = [200, 250, 300, 350, 400];

/// The last concert that offers songs of its own.
pub const LAST_SONG_WINDOW: u8 = 4;

/// Whether a concert offers any songs. False for the closing Grand Concert.
#[must_use]
pub fn has_songs(window: u8) -> bool {
    songs_in_window(window).next().is_some()
}

/// One purchasable song.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Song {
    /// Stable slug, shared with the sister project so the two agree.
    pub id: &'static str,
    pub name: &'static str,
    /// Concert this song belongs to, 1-4.
    pub window: u8,
    pub cost: TokenVector,
    /// Whether uma.guide's default plan takes this song. The guide skips a few
    /// on cost grounds; that judgement is theirs, and it is only a default.
    pub planned_by_default: bool,
}

const fn song(id: &'static str, name: &'static str, window: u8, cost: TokenVector, planned_by_default: bool) -> Song {
    Song {
        id,
        name,
        window,
        cost,
        planned_by_default,
    }
}

/// Every song, grouped by window in the order the guide lists them.
pub const SONGS: &[Song] = &[
    // Concert 1
    song("here-comes-our-time", "Here Comes Our Time", 1, [0, 0, 32, 0, 12], true),
    song(
        "full-speed-ahead-umadol-power",
        "Full Speed Ahead! Umadol Power\u{2606}",
        1,
        [32, 0, 0, 12, 0],
        true,
    ),
    song("run-x-run", "Run n' Run!", 1, [14, 0, 0, 16, 14], true),
    song(
        "believe-in-miracles",
        "Believe in Miracles!",
        1,
        [0, 21, 0, 0, 21],
        true,
    ),
    song(
        "zero-center-stands",
        "Zero Is Where the Center Stands!",
        1,
        [21, 0, 0, 21, 0],
        true,
    ),
    song("go-this-way", "Go This Way", 1, [0, 0, 21, 0, 21], false),
    song(
        "run-away-fallin-love",
        "Getaway! Fallin' Love",
        1,
        [21, 0, 0, 21, 0],
        false,
    ),
    song("ring-ring-diary", "Ring Ring Diary", 1, [0, 21, 0, 21, 0], false),
    // Concert 2
    song("run-for-our-dream", "Run for Our Dream!", 2, [0, 21, 0, 21, 0], true),
    song("our-blue-bird-days", "Our Blue Bird Days", 2, [21, 0, 0, 42, 0], true),
    song("a-no-ne", "Hey, Guess What!", 2, [42, 0, 0, 21, 0], true),
    // Concert 3
    song("grow-up-and-shine", "Grow Up and Shine!", 3, [21, 0, 21, 0, 21], true),
    song(
        "seven-colors-scenery",
        "Seven Colors Scenery",
        3,
        [0, 0, 21, 0, 42],
        true,
    ),
    song("sunbeam-cheer", "Sunbeam Cheer", 3, [0, 42, 0, 0, 21], true),
    song(
        "hoppity-sunny-days",
        "Hoppity Sunny Days\u{266a}",
        3,
        [0, 42, 21, 0, 0],
        false,
    ),
    // Concert 4
    song(
        "precious-treasure-box",
        "Precious Treasure Box",
        4,
        [42, 0, 0, 26, 0],
        true,
    ),
    song(
        "fanfare-for-the-future",
        "Fanfare for the Future!",
        4,
        [26, 0, 0, 42, 0],
        true,
    ),
    song("dream-sky", "Dream Sky", 4, [0, 22, 0, 0, 22], true),
    song("present-march", "Present March\u{266a}", 4, [0, 0, 22, 0, 22], true),
    song(
        "worlds-at-our-whim",
        "The World's at Our Whim",
        4,
        [0, 32, 12, 0, 0],
        true,
    ),
    song("sky-blue-spring", "Sky-Blue Spring", 4, [12, 0, 0, 32, 0], true),
];

/// Which concert window a live per-token cap corresponds to, or `None` if the
/// cap is unknown (`0`) or not one we recognise.
///
/// The cap is read from `GetPerformanceMax` every capture, so the window is
/// derived from the game rather than from a turn-number table that would need
/// maintaining.
#[must_use]
pub fn window_for_cap(cap: i32) -> Option<u8> {
    CONCERT_CAPS
        .iter()
        .position(|&c| c == cap)
        .map(|i| u8::try_from(i + 1).unwrap_or(1))
}

/// Reduce a song name to something two sources can be compared on.
///
/// The catalogue's names were transcribed from uma.guide; the game's come from
/// `get_MusicName()`. They agree on words but not reliably on punctuation —
/// apostrophes may be `'` or `\u{2019}`, and decorations like `\u{2606}` or
/// `\u{266a}` are easy to lose in transcription. Comparing only lowercase
/// alphanumerics survives all of that while still telling the 21 songs apart.
#[must_use]
pub fn normalize_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Find the catalogue song matching a name from the game, if any.
#[must_use]
pub fn song_by_name(name: &str) -> Option<&'static Song> {
    let needle = normalize_name(name);
    SONGS.iter().find(|s| normalize_name(s.name) == needle)
}

/// Songs belonging to one concert window.
pub fn songs_in_window(window: u8) -> impl Iterator<Item = &'static Song> {
    SONGS.iter().filter(move |s| s.window == window)
}

/// Combined cost of the default-planned songs in one window.
#[must_use]
pub fn planned_cost(window: u8) -> TokenVector {
    let mut total = [0; 5];
    for s in songs_in_window(window).filter(|s| s.planned_by_default) {
        for (slot, cost) in total.iter_mut().zip(s.cost) {
            *slot += cost;
        }
    }
    total
}

/// How many default-planned songs a window has.
#[must_use]
pub fn planned_count(window: u8) -> usize {
    songs_in_window(window).filter(|s| s.planned_by_default).count()
}

/// Per-token shortfall of `available` against `required`, never negative.
#[must_use]
pub fn shortfall(required: TokenVector, available: TokenVector) -> TokenVector {
    std::array::from_fn(|i| (required[i] - available[i]).max(0))
}

/// Whether any single token's requirement exceeds the window's ceiling — the
/// plan cannot be met in that window no matter how you train.
#[must_use]
pub fn exceeds_cap(required: TokenVector, cap: i32) -> bool {
    cap > 0 && required.iter().any(|&v| v > cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_the_researched_twenty_one() {
        assert_eq!(SONGS.len(), 21);
        for window in 1..=4 {
            assert!(songs_in_window(window).count() > 0, "window {window} is empty");
        }
        assert_eq!(songs_in_window(1).count(), 8);
        assert_eq!(songs_in_window(2).count(), 3);
        assert_eq!(songs_in_window(3).count(), 4);
        assert_eq!(songs_in_window(4).count(), 6);
    }

    #[test]
    fn song_ids_are_unique() {
        for (i, a) in SONGS.iter().enumerate() {
            for b in &SONGS[i + 1..] {
                assert_ne!(a.id, b.id, "duplicate song id {}", a.id);
            }
        }
    }

    #[test]
    fn every_song_costs_something_in_at_least_one_token() {
        for s in SONGS {
            assert!(s.cost.iter().any(|&v| v > 0), "{} is free", s.id);
        }
    }

    #[test]
    fn window_one_guide_plan_matches_the_transcribed_costs() {
        // Here Comes Our Time + Umadol Power + Run n' Run! + Believe in
        // Miracles! + Zero Is Where the Center Stands!, summed per token.
        assert_eq!(planned_count(1), 5);
        assert_eq!(planned_cost(1), [67, 21, 32, 49, 47]);
    }

    #[test]
    fn the_guide_skips_three_of_window_one() {
        let skipped: Vec<&str> = songs_in_window(1)
            .filter(|s| !s.planned_by_default)
            .map(|s| s.id)
            .collect();
        assert_eq!(skipped, ["go-this-way", "run-away-fallin-love", "ring-ring-diary"]);
    }

    #[test]
    fn names_match_across_punctuation_differences() {
        // Whatever the game hands back, these must land on the same song.
        for variant in [
            "Full Speed Ahead! Umadol Power\u{2606}",
            "Full Speed Ahead! Umadol Power",
            "full speed ahead umadol power",
        ] {
            assert_eq!(
                song_by_name(variant).map(|s| s.id),
                Some("full-speed-ahead-umadol-power"),
                "{variant:?} did not match"
            );
        }
        // Apostrophe style must not matter.
        assert_eq!(song_by_name("Run n\u{2019} Run!").map(|s| s.id), Some("run-x-run"));
        assert_eq!(song_by_name("Run n' Run!").map(|s| s.id), Some("run-x-run"));
    }

    #[test]
    fn normalizing_still_tells_every_song_apart() {
        let mut seen: Vec<String> = SONGS.iter().map(|s| normalize_name(s.name)).collect();
        seen.sort();
        let before = seen.len();
        seen.dedup();
        assert_eq!(seen.len(), before, "two songs normalize to the same name");
    }

    #[test]
    fn an_unknown_name_matches_nothing() {
        assert!(song_by_name("Never Gonna Give You Up").is_none());
        assert!(song_by_name("").is_none());
    }

    #[test]
    fn caps_identify_their_window() {
        assert_eq!(window_for_cap(200), Some(1));
        assert_eq!(window_for_cap(250), Some(2));
        assert_eq!(window_for_cap(300), Some(3));
        assert_eq!(window_for_cap(350), Some(4));
        assert_eq!(window_for_cap(400), Some(5), "the closing Grand Concert");
    }

    /// The finale raises the ceiling but adds no songs. If that ever changes,
    /// the planner needs to page to it and this test is the tripwire.
    #[test]
    fn only_the_closing_concert_has_no_songs_of_its_own() {
        for window in 1..=LAST_SONG_WINDOW {
            assert!(has_songs(window), "concert {window} should offer songs");
        }
        assert!(!has_songs(5), "the closing Grand Concert offers none");
    }

    #[test]
    fn an_unknown_cap_identifies_no_window() {
        assert_eq!(window_for_cap(0), None);
        assert_eq!(window_for_cap(275), None);
    }

    #[test]
    fn shortfall_counts_only_what_is_missing() {
        assert_eq!(shortfall([67, 21, 32, 49, 47], [17, 4, 10, 8, 7]), [50, 17, 22, 41, 40]);
        assert_eq!(shortfall([10, 10, 10, 10, 10], [99, 99, 99, 99, 99]), [0; 5]);
    }

    #[test]
    fn a_plan_within_the_ceiling_does_not_flag() {
        assert!(!exceeds_cap(planned_cost(1), CONCERT_CAPS[0]));
        assert!(exceeds_cap([201, 0, 0, 0, 0], 200));
        // An unknown ceiling can never be exceeded — it is not a ceiling of 0.
        assert!(!exceeds_cap([999; 5], 0));
    }
}
