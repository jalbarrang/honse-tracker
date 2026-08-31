//! Screen identity: `Gallop.SceneDefine.ViewId` observations, normalized.
//!
//! This module answers exactly one question — *which screen is this?* — and
//! deliberately says nothing about what that should mean. Read permission and
//! panel visibility are policy, they disagree with each other, and they live in
//! the tracker's `read_gate`.
//!
//! # Why identity is separate from policy
//!
//! The previous design mapped ids straight onto behavioural buckets
//! (`Concert`, `Race`, …). That threw the screen's identity away at the first
//! step, so a miscategorised id was invisible: view 1620 was labelled `Concert`
//! and hid the HUD for months while actually being the Techniques Shop, and
//! 1400 was labelled `Race` while actually being the Skills Shop. Neither could
//! be spotted from a log, because by the time anything was logged the identity
//! was gone.
//!
//! Here the id maps to *what it is*. A wrong policy is now an argument about
//! one match arm rather than a lie baked into the identity.
//!
//! # Adding a screen
//!
//! Add a variant and a [`VIEWS`] row. The policy match in `read_gate` is
//! exhaustive, so it will not compile until the new screen's behaviour is
//! decided — which is the point. Names come from manual in-game observation on
//! the Windows/Steam build.

use std::ffi::CStr;

/// A game screen, identified. One variant per catalogued view id.
///
/// `#[repr(u8)]` so [`View::label`] can find its own row without needing
/// `PartialEq` in const context.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum View {
    Launch,
    PressStart,
    Home,
    ScreenTransition,
    GachaPull,
    ConcertPlayback,
    TrackblazerPointsShop,
    RacePlayback,
    Intermission,
    CareerTraining,
    CareerPreComplete,
    CareerComplete,
    CareerIndependentTraining,
    RacePaddock,
    RaceList,
    CareerSkillsShop,
    CareerInspiration,
    GrandConcertTechniquesShop,
    GrandConcertConcertView,
    CareerStoryQuick,
    TeamTrialsHome,
    TeamTrialsEditTeam,
    TeamTrialsOpponentSelect,
    TeamTrialsScheduledBattle,
    TeamTrialsBattle,
    TeamTrialsResults,
    DailyLegendRaceHome,
    DailyLegendRaceSingle,
    DailyLegendRacePaddock,
    ConcertTheaterHome,
    /// An id with no row below. Never assume it is harmless — policy fails
    /// closed on it, and it should be catalogued rather than left here.
    #[default]
    Unknown,
}

/// The single id ↔ screen ↔ label table.
///
/// NUL-terminated labels so the host can hand a stable `'static` pointer
/// straight to plugins over FFI.
const VIEWS: &[(i32, View, &CStr)] = &[
    (1, View::Launch, c"General - Launch"),
    (2, View::PressStart, c"General - Press Start"),
    (35, View::TrackblazerPointsShop, c"Trackblazer - Points Shop"),
    (100, View::ScreenTransition, c"General - Screen Transition"),
    (101, View::Home, c"General - Home Screen"),
    (200, View::ConcertPlayback, c"General - Concert Playback"),
    (300, View::GachaPull, c"General - Gacha Pull"),
    (400, View::RacePlayback, c"Race - Playback"),
    (1100, View::Intermission, c"Intermission"),
    (1101, View::CareerTraining, c"Career - Training"),
    (1200, View::RacePaddock, c"Race - Paddock"),
    (1210, View::RaceList, c"Race - Race List"),
    (1300, View::CareerPreComplete, c"Career - Pre-Complete"),
    (1301, View::CareerComplete, c"Career - Complete"),
    (1400, View::CareerSkillsShop, c"Career - Skills Shop"),
    (1500, View::CareerInspiration, c"Career - Inspiration"),
    (
        1620,
        View::GrandConcertTechniquesShop,
        c"Grand Concert - Techniques Shop",
    ),
    (1621, View::GrandConcertConcertView, c"Grand Concert - Concert View"),
    (3000, View::CareerStoryQuick, c"Career - Story (Quick mode)"),
    (4000, View::TeamTrialsHome, c"Team Trials - Home"),
    (4020, View::TeamTrialsEditTeam, c"Team Trials - Edit Team"),
    (4040, View::TeamTrialsOpponentSelect, c"Team Trials - Opponent Select"),
    (4050, View::TeamTrialsScheduledBattle, c"Team Trials - Scheduled Battle"),
    (4060, View::TeamTrialsBattle, c"Team Trials - Battle"),
    (4080, View::TeamTrialsResults, c"Team Trials - Results"),
    (5620, View::DailyLegendRaceSingle, c"Daily Legend Races - Challenger"),
    (5630, View::DailyLegendRacePaddock, c"Daily Legend Races - Paddock"),
    (5650, View::DailyLegendRaceHome, c"Daily Legend Races - Home"),
    (5710, View::ConcertTheaterHome, c"Concert Theater - Home"),
    (6600, View::CareerIndependentTraining, c"Career - Independent Training"),
];

impl View {
    /// Identify a raw `SceneManager.GetCurrentViewId()` observation.
    ///
    /// Uncatalogued ids become [`View::Unknown`]; this function makes no
    /// judgement about what that should mean.
    #[must_use]
    pub const fn from_id(view_id: i32) -> Self {
        let mut i = 0;
        while i < VIEWS.len() {
            if VIEWS[i].0 == view_id {
                return VIEWS[i].1;
            }
            i += 1;
        }
        Self::Unknown
    }

    /// The view id this screen is observed as, or `None` for [`View::Unknown`].
    #[must_use]
    pub const fn id(self) -> Option<i32> {
        let mut i = 0;
        while i < VIEWS.len() {
            if VIEWS[i].1 as u8 == self as u8 {
                return Some(VIEWS[i].0);
            }
            i += 1;
        }
        None
    }

    /// Human-readable label, or `None` for [`View::Unknown`].
    #[must_use]
    pub fn label(self) -> Option<&'static str> {
        self.label_cstr().and_then(|c| c.to_str().ok())
    }

    /// Every catalogued screen, in table order. Excludes [`View::Unknown`],
    /// which is the absence of a row rather than a screen.
    ///
    /// Exists so tests can cover the whole table without keeping a second list
    /// in step with this one — the drift that produced the original bug.
    pub fn catalogued() -> impl Iterator<Item = Self> {
        VIEWS.iter().map(|&(_, view, _)| view)
    }

    /// Label as a C string, for handing over FFI.
    #[must_use]
    pub const fn label_cstr(self) -> Option<&'static CStr> {
        let mut i = 0;
        while i < VIEWS.len() {
            if VIEWS[i].1 as u8 == self as u8 {
                return Some(VIEWS[i].2);
            }
            i += 1;
        }
        None
    }
}

/// Label for a known view id as a C string, if catalogued.
#[must_use]
pub fn view_name_cstr(view_id: i32) -> Option<&'static CStr> {
    View::from_id(view_id).label_cstr()
}

/// Label for a known view id, if catalogued.
#[must_use]
pub fn view_name(view_id: i32) -> Option<&'static str> {
    View::from_id(view_id).label()
}

#[cfg(test)]
mod tests {
    use super::{view_name, view_name_cstr, View, VIEWS};

    #[test]
    fn known_ids_resolve() {
        assert_eq!(view_name(1), Some("General - Launch"));
        assert_eq!(view_name(400), Some("Race - Playback"));
        assert_eq!(view_name(1101), Some("Career - Training"));
    }

    #[test]
    fn unknown_ids_are_none() {
        assert!(view_name(0).is_none());
        assert!(view_name_cstr(424_242).is_none());
        assert_eq!(View::from_id(424_242), View::Unknown);
        assert!(View::Unknown.label().is_none());
        assert!(View::Unknown.id().is_none());
    }

    /// The two screens whose misidentification hid the HUD. Named here so a
    /// future edit that reverts them fails loudly.
    #[test]
    fn the_two_shops_are_shops() {
        assert_eq!(View::from_id(1620), View::GrandConcertTechniquesShop);
        assert_eq!(View::from_id(1400), View::CareerSkillsShop);
        assert_eq!(View::from_id(1621), View::GrandConcertConcertView);
    }

    #[test]
    fn every_row_round_trips_id_and_label() {
        for &(id, view, label) in VIEWS {
            assert_eq!(View::from_id(id), view, "id {id} did not resolve to {view:?}");
            assert_eq!(view.id(), Some(id), "{view:?} did not map back to {id}");
            assert_eq!(view.label_cstr(), Some(label), "{view:?} label mismatch");
        }
    }

    #[test]
    fn ids_are_unique() {
        for (i, a) in VIEWS.iter().enumerate() {
            for b in &VIEWS[i + 1..] {
                assert_ne!(a.0, b.0, "duplicate view id {}", a.0);
                assert_ne!(a.1 as u8, b.1 as u8, "duplicate variant {:?}", a.1);
            }
        }
    }
}
