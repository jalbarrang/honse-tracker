//! Per-scenario live state, dispatched by the active scenario's raw master ID.
//!
//! `WorkSingleModeCharaData.get_ScenarioId()` returns the `single_mode_scenario.id`
//! value, **not** release order (`sort_id`). The current Global build maps:
//!
//! | Raw ID | IL2CPP `ScenarioId` | Global name |
//! |---:|---|---|
//! | 1 | `URA` | The Beginning: URA Finale |
//! | 2 | `TeamRace` | Unity Cup: Shine On, Team Spirit! (Aoharu) |
//! | 3 | `Live` | Brighter Together: Our Grand Concert (Grand Live) |
//! | 4 | `Free` | Trackblazer: Start of the Climax (Make a New Track) |
//! | 5 | `Venus` | Grandmasters: Legacies Immortal |
//!
//! IDs 3 and 4 are intentionally opposite their release `sort_id` values: Grand
//! Live has `(id=3, sort_id=4)` and Trackblazer has `(id=4, sort_id=3)`. Dispatch
//! must therefore never use release order, probe order, or work-object presence.
//! See `docs/scenario-ids.md` for the build evidence and reproducible SQL.

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};

pub mod grand_live;
mod items;
mod master_shop;
mod trackblazer;

pub use grand_live::GrandLivePerformance;
pub use items::Worth;
pub use trackblazer::{TrackblazerOwnedItem, TrackblazerShop, TrackblazerShopItem};

/// Live scenario-specific state for the active run, if it is a supported scenario.
#[derive(Debug, Clone)]
pub enum ScenarioState {
    /// Trackblazer / Make a New Track — RaceCoin shop readout.
    Trackblazer(TrackblazerShop),
    /// Grand Live / Our Grand Concert — performance points.
    GrandLive(GrandLivePerformance),
}

/// Scenario identity decoded from `single_mode_scenario.id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioKind {
    Ura,
    Aoharu,
    GrandLive,
    Trackblazer,
    GrandMasters,
    Unknown,
}

impl ScenarioKind {
    const fn from_raw(raw: i32) -> Self {
        match raw {
            1 => Self::Ura,
            2 => Self::Aoharu,
            3 => Self::GrandLive,
            4 => Self::Trackblazer,
            5 => Self::GrandMasters,
            _ => Self::Unknown,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Ura => "ura",
            Self::Aoharu => "aoharu",
            Self::GrandLive => "grand_live",
            Self::Trackblazer => "trackblazer",
            Self::GrandMasters => "grand_masters",
            Self::Unknown => "unknown",
        }
    }
}

/// Reader selected for a raw scenario ID. Unsupported scenarios fail closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScenarioReader {
    GrandLive,
    Trackblazer,
    Unsupported,
}

impl ScenarioReader {
    const fn name(self) -> &'static str {
        match self {
            Self::GrandLive => "grand_live",
            Self::Trackblazer => "trackblazer",
            Self::Unsupported => "unsupported",
        }
    }
}

const fn reader_for(scenario: ScenarioKind) -> ScenarioReader {
    match scenario {
        ScenarioKind::GrandLive => ScenarioReader::GrandLive,
        ScenarioKind::Trackblazer => ScenarioReader::Trackblazer,
        ScenarioKind::Ura | ScenarioKind::Aoharu | ScenarioKind::GrandMasters | ScenarioKind::Unknown => {
            ScenarioReader::Unsupported
        }
    }
}

fn log_dispatch(raw_scenario_id: i32, scenario: ScenarioKind, reader: ScenarioReader) {
    // Scenario identity is stable within a career. Log once per observed raw-ID
    // change so runtime evidence is visible without one line per capture.
    static LAST_LOGGED_ID: AtomicI32 = AtomicI32::new(i32::MIN);
    if LAST_LOGGED_ID.swap(raw_scenario_id, Ordering::Relaxed) != raw_scenario_id {
        hlog_info!(
            "Scenario dispatch: raw_scenario_id={} scenario={} selected_reader={}",
            raw_scenario_id,
            scenario.name(),
            reader.name()
        );
    }
}

/// Read scenario-specific state from `WorkSingleModeCharaData` (`chara`) and
/// `WorkSingleModeData` (`wsmd`). `raw_scenario_id` comes from the character's
/// `get_ScenarioId()` getter.
///
/// There is deliberately no reader fallback: a selected reader returning `None`
/// stays `None`, and an unknown/unsupported ID never probes either reader.
pub(super) fn read_scenario_state(
    chara: *mut c_void,
    wsmd: *mut c_void,
    raw_scenario_id: i32,
) -> Option<ScenarioState> {
    let scenario = ScenarioKind::from_raw(raw_scenario_id);
    let reader = reader_for(scenario);
    log_dispatch(raw_scenario_id, scenario, reader);

    match reader {
        ScenarioReader::GrandLive => {
            // SAFETY: `wsmd` is a valid non-null IL2CPP object.
            unsafe { grand_live::read_performance(wsmd) }.map(ScenarioState::GrandLive)
        }
        ScenarioReader::Trackblazer => {
            // SAFETY: `chara` is a valid non-null IL2CPP object.
            unsafe { trackblazer::read_shop(chara) }.map(ScenarioState::Trackblazer)
        }
        ScenarioReader::Unsupported => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn grand_live_selects_only_the_grand_live_reader() {
        let scenario = ScenarioKind::from_raw(3);
        assert_eq!(scenario, ScenarioKind::GrandLive);
        assert_eq!(reader_for(scenario), ScenarioReader::GrandLive);
        assert_ne!(reader_for(scenario), ScenarioReader::Trackblazer);
    }

    #[test]
    fn trackblazer_selects_only_the_trackblazer_reader() {
        let scenario = ScenarioKind::from_raw(4);
        assert_eq!(scenario, ScenarioKind::Trackblazer);
        assert_eq!(reader_for(scenario), ScenarioReader::Trackblazer);
        assert_ne!(reader_for(scenario), ScenarioReader::GrandLive);
    }

    #[test]
    fn known_but_unsupported_scenarios_do_not_select_a_reader() {
        for raw in [1, 2, 5] {
            assert_eq!(reader_for(ScenarioKind::from_raw(raw)), ScenarioReader::Unsupported);
        }
    }

    proptest! {
        #[test]
        fn every_other_raw_id_fails_closed(raw in any::<i32>().prop_filter(
            "supported reader IDs are covered by unit tests",
            |raw| !matches!(*raw, 3 | 4),
        )) {
            let scenario = ScenarioKind::from_raw(raw);
            prop_assert_eq!(reader_for(scenario), ScenarioReader::Unsupported);
            // Null pointers prove the unsupported path does not probe either IL2CPP reader.
            prop_assert!(read_scenario_state(std::ptr::null_mut(), std::ptr::null_mut(), raw).is_none());
        }
    }
}
