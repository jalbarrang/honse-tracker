//! Grand Live / Our Grand Concert performance points reader.
//!
//! Path (all ObscuredInt backing fields — safe direct reads):
//! ```text
//! WorkSingleModeData.get_ScenarioLive() -> WorkSingleModeScenarioLive
//!   ._performance (PerformanceData value-type field, inline struct)
//!     fields: Dance, Passion, Vocal, Visual, Mental (each ObscuredInt)
//! ```
//!
//! PerformanceData is a value type (struct) stored inline in the parent object.
//! We read the ObscuredInt fields directly from the parent using field offsets.

use std::ffi::c_void;
use std::sync::Mutex;

use super::super::il2cpp::{call_obj, resolve_obj_method};

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
/// Returns `None` if the Grand Live scenario is not active.
///
/// # Safety
/// `wsmd` must be a valid non-null `WorkSingleModeData` IL2CPP object pointer.
pub(in crate::memory_reader) unsafe fn read_performance(wsmd: *mut c_void) -> Option<GrandLivePerformance> {
    unsafe {
        let m_get_live = resolve_obj_method(wsmd, "get_ScenarioLive", 0)?;
        let live = call_obj(wsmd, m_get_live);
        if live.is_null() {
            return None;
        }

        // PerformanceData is a value type. Use GetPerformance(enum) instead,
        // which takes a Performance enum (int-backed) and returns the decrypted i32.
        // Dance=0, Passion=1, Vocal=2, Visual=3, Mental=4
        let m_get = resolve_obj_method(live, "GetPerformance", 1)?;

        let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 =
            std::mem::transmute(super::super::il2cpp::method_ptr(m_get));

        let result = GrandLivePerformance {
            dance: fp(live, 0, m_get),
            passion: fp(live, 1, m_get),
            vocal: fp(live, 2, m_get),
            visual: fp(live, 3, m_get),
            mental: fp(live, 4, m_get),
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
