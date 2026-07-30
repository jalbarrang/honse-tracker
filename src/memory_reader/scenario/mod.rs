//! Per-scenario live state, dispatched off the active scenario.
//!
//! Each scenario keeps its own `WorkSingleModeScenario*` work object hanging off
//! `WorkSingleModeCharaData`. This module owns the readers for those objects and
//! a single [`ScenarioState`] enum surfaced through `CareerSnapshot`.
//!
//! Dispatch is keyed structurally: we simply try each scenario's work-object
//! accessor and read whichever is present (e.g. `get_WorkScenarioFree()` is null
//! unless the Trackblazer scenario is active). New scenarios add a variant + a
//! reader module mirroring [`trackblazer`].

use std::ffi::c_void;

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

/// Read scenario-specific state from the chara-data work object and the
/// single-mode data object.
/// `chara` is `WorkSingleModeCharaData`, `wsmd` is `WorkSingleModeData`.
/// `scenario_id` is the active scenario from `get_ScenarioId()`.
pub(super) fn read_scenario_state(chara: *mut c_void, wsmd: *mut c_void, scenario_id: i32) -> Option<ScenarioState> {
    // Dispatch on scenario_id to avoid false positives from work objects
    // that exist but belong to a different scenario.
    match scenario_id {
        // Grand Live / Our Grand Concert (ScenarioId.Live)
        3 => {
            // SAFETY: `wsmd` is a valid non-null IL2CPP object.
            unsafe { grand_live::read_performance(wsmd) }.map(ScenarioState::GrandLive)
        }
        // Trackblazer / Make a New Track (ScenarioId.Free)
        4 => {
            // SAFETY: `chara` is a valid non-null IL2CPP object.
            unsafe { trackblazer::read_shop(chara) }.map(ScenarioState::Trackblazer)
        }
        // URA (1), Aoharu (2), Venus (5), or unknown — no scenario-specific state.
        _ => None,
    }
}
