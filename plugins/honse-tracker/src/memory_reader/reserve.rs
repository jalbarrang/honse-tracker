//! Player-reserved races (the in-game "agenda") read from SingleMode memory.
//!
//! Walks the reserve chain off the active career:
//! ```text
//! WorkSingleModeData
//!   → get_RaceReserveContext() → SingleModeRaceReserve.Context
//!     → get_ReserveRepository() → SingleModeRaceReserveRepository
//!       → GetActiveDeckEntity() → SingleModeRaceReserveDeckEntity
//!         → get_DeckInfo() → SingleModeReservedRaceDeck { race_array }
//!           → SingleModeReservedRace[] { year, program_id }
//! ```
//!
//! Every step is a plain property/field read (no `MasterDataManager` lookups,
//! which crash on the render thread). Each accessor is resolved best-effort, so a
//! future game rename just yields an empty list instead of failing the tracker.
//! `program_id` resolves to the concrete race (name/grade/distance/turn) on the
//! dashboard side via master data.

use std::ffi::c_void;

use crate::compat::Sdk;

use super::chain::get_single_mode_data;
use super::il2cpp::{call_obj, read_i32_field, resolve_obj_method};

/// Field names on `Gallop.SingleModeReservedRace` (plain `System.Int32`).
const FIELD_YEAR: &str = "year";
const FIELD_PROGRAM_ID: &str = "program_id";

/// One reserved race: `(year, program_id)` exactly as stored by the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReservedRace {
    pub year: i32,
    pub program_id: i32,
}

/// Reserved races on the active deck, in stored order. Empty when no career is
/// active, nothing is reserved, or the accessors are unavailable.
pub fn read_reserved_races() -> Vec<ReservedRace> {
    // SAFETY: all reads are on resolved IL2CPP metadata / live objects; any bad
    // pointer is contained so it can't take down the render thread.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { inner() })) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_reserved_races PANICKED");
            Vec::new()
        }
    }
}

/// Walk one 0-arg object getter, returning null-safe `None` on a missing accessor
/// or null result.
unsafe fn step(obj: *mut c_void, getter: &str) -> Option<*mut c_void> {
    // SAFETY: caller passes a live IL2CPP object; resolve + call a 0-arg getter.
    let mi = unsafe { resolve_obj_method(obj, getter, 0) }?;
    // SAFETY: 0-arg getter on the live object.
    let next = unsafe { call_obj(obj, mi) };
    (!next.is_null()).then_some(next)
}

unsafe fn inner() -> Vec<ReservedRace> {
    // `get_single_mode_data` already gates on an active (is_playing) career.
    let Some(wsmd) = get_single_mode_data() else {
        return Vec::new();
    };

    // SAFETY: each step resolves + calls a 0-arg getter on the prior live object.
    let chain = unsafe {
        step(wsmd, "get_RaceReserveContext")
            .and_then(|ctx| step(ctx, "get_ReserveRepository"))
            .and_then(|repo| step(repo, "GetActiveDeckEntity"))
            .and_then(|deck| step(deck, "get_DeckInfo"))
    };
    let Some(deck_info) = chain else {
        return Vec::new();
    };

    // `SingleModeReservedRaceDeck.race_array` is a reference-type field holding a
    // `SingleModeReservedRace[]`. Read the field, then the SZ-array.
    let sdk = Sdk::get();
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let deck_klass = unsafe { *(deck_info as *const *mut c_void) };
    let Some(arr_field) = sdk.get_field_from_name(deck_klass.cast(), "race_array") else {
        hlog_warn!(target: "training-tracker", "reserve: race_array field not found");
        return Vec::new();
    };
    let mut array: *mut c_void = std::ptr::null_mut();
    // SAFETY: reference-type field on a live SingleModeReservedRaceDeck.
    unsafe {
        sdk.get_field_value(
            deck_info.cast(),
            arr_field.cast(),
            std::ptr::from_mut(&mut array).cast(),
        );
    }
    if array.is_null() {
        return Vec::new();
    }

    // SZ-array (64-bit): length (i32) at 0x18, element pointers from 0x20.
    // SAFETY: `array` is a live IL2CPP System.Array of references.
    let len = unsafe { *(array.byte_add(0x18) as *const i32) };
    if len <= 0 || len > 64 {
        return Vec::new();
    }

    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        // SAFETY: element `i` (pointer-sized) within the array bounds.
        let race = unsafe { *(array.byte_add(0x20 + (i as usize) * 8) as *const *mut c_void) };
        if race.is_null() {
            continue;
        }
        // SAFETY: plain i32 fields on a valid SingleModeReservedRace (0 if absent).
        let year = unsafe { read_i32_field(race, FIELD_YEAR) };
        // SAFETY: plain i32 field on a valid SingleModeReservedRace (0 if absent).
        let program_id = unsafe { read_i32_field(race, FIELD_PROGRAM_ID) };
        out.push(ReservedRace { year, program_id });
    }
    out
}
