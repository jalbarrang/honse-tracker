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
pub(super) fn read_scenario_state(chara: *mut c_void, wsmd: *mut c_void) -> Option<ScenarioState> {
    // Try Trackblazer first.
    // SAFETY: `chara` is a valid non-null IL2CPP object from the resolved chain.
    if let Some(shop) = unsafe { trackblazer::read_shop(chara) } {
        return Some(ScenarioState::Trackblazer(shop));
    }
    // Try Grand Live.
    // SAFETY: `wsmd` is a valid non-null IL2CPP object from the resolved chain.
    if let Some(perf) = unsafe { grand_live::read_performance(wsmd) } {
        return Some(ScenarioState::GrandLive(perf));
    }
    None
}
