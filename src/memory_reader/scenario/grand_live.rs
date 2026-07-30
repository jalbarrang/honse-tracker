//! Grand Live / Our Grand Concert performance points reader.
//!
//! Path (all ObscuredInt backing fields — safe direct reads):
//! ```text
//! WorkSingleModeData.get_ScenarioLive() -> WorkSingleModeScenarioLive
//!   ._performance -> PerformanceData (nested struct)
//!     .<Dance>k__BackingField      -> ObscuredInt
//!     .<Passion>k__BackingField    -> ObscuredInt
//!     .<Vocal>k__BackingField      -> ObscuredInt
//!     .<Visual>k__BackingField     -> ObscuredInt
//!     .<Mental>k__BackingField     -> ObscuredInt  (displayed as "Comedy" in EN)
//! ```

use std::ffi::c_void;
use std::sync::Mutex;

use super::super::il2cpp::{call_obj, read_obscured_int_field, resolve_obj_method};

/// Live performance points for the Grand Live scenario.
#[derive(Debug, Clone, Default)]
pub struct GrandLivePerformance {
    pub dance: i32,
    pub passion: i32,
    pub vocal: i32,
    pub visual: i32,
    /// Called "Comedy" in EN, "Mental" internally.
    pub mental: i32,
}

/// Read Grand Live performance points from `WorkSingleModeData`.
/// Returns `None` if the Grand Live scenario is not active (`get_ScenarioLive` returns null).
///
/// # Safety
/// `wsmd` must be a valid non-null `WorkSingleModeData` IL2CPP object pointer.
pub(in crate::memory_reader) unsafe fn read_performance(wsmd: *mut c_void) -> Option<GrandLivePerformance> {
    // SAFETY: all calls operate on non-null IL2CPP objects verified by null checks.
    unsafe {
        let m_get_live = resolve_obj_method(wsmd, "get_ScenarioLive", 0)?;
        let live = call_obj(wsmd, m_get_live);
        if live.is_null() {
            return None;
        }

        let m_get_perf = resolve_obj_method(live, "get_Performance", 0)?;
        let perf = call_obj(live, m_get_perf);
        if perf.is_null() {
            return None;
        }

        let sdk = crate::compat::Sdk::get();
        let perf_klass = *(perf as *const *mut c_void);

        let f_dance = sdk.get_field_from_name(perf_klass.cast(), "<Dance>k__BackingField")?;
        let f_passion = sdk.get_field_from_name(perf_klass.cast(), "<Passion>k__BackingField")?;
        let f_vocal = sdk.get_field_from_name(perf_klass.cast(), "<Vocal>k__BackingField")?;
        let f_visual = sdk.get_field_from_name(perf_klass.cast(), "<Visual>k__BackingField")?;
        let f_mental = sdk.get_field_from_name(perf_klass.cast(), "<Mental>k__BackingField")?;

        let result = GrandLivePerformance {
            dance: read_obscured_int_field(perf, f_dance.cast()),
            passion: read_obscured_int_field(perf, f_passion.cast()),
            vocal: read_obscured_int_field(perf, f_vocal.cast()),
            visual: read_obscured_int_field(perf, f_visual.cast()),
            mental: read_obscured_int_field(perf, f_mental.cast()),
        };
        log_on_change(&result);
        Some(result)
    }
}

/// Log performance points when they change (deduped).
fn log_on_change(p: &GrandLivePerformance) {
    static LAST: Mutex<Option<(i32, i32, i32, i32, i32)>> = Mutex::new(None);
    let cur = (p.dance, p.passion, p.vocal, p.visual, p.mental);
    if let Ok(mut guard) = LAST.lock() {
        if guard.as_ref() == Some(&cur) {
            return;
        }
        *guard = Some(cur);
    }
    hlog_info!(
        "Grand Live performance: Da={} Pa={} Vo={} Vi={} Co={}",
        p.dance, p.passion, p.vocal, p.visual, p.mental
    );
}
