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

mod items;
mod master_shop;
mod trackblazer;

pub use items::Worth;
pub use trackblazer::{TrackblazerOwnedItem, TrackblazerShop, TrackblazerShopItem};

/// Live scenario-specific state for the active run, if it is a supported scenario.
#[derive(Debug, Clone)]
pub enum ScenarioState {
    /// Trackblazer / Make a New Track — RaceCoin shop readout.
    Trackblazer(TrackblazerShop),
}

/// Read scenario-specific state from the chara-data work object.
/// Returns `None` for unsupported scenarios (e.g. URA Finale base scenario).
/// `chara` is the `WorkSingleModeCharaData` object pointer.
pub(super) fn read_scenario_state(chara: *mut c_void) -> Option<ScenarioState> {
    // SAFETY: `chara` is a valid non-null IL2CPP object from the resolved chain.
    unsafe { trackblazer::read_shop(chara) }.map(ScenarioState::Trackblazer)
}
