//! Fired single-mode event history (`WorkSingleModeData.StoryInfoListDic`).
//!
//! Read-only: a `Dictionary<SingleModeEventPlayTiming, List<EventInfo>>` of events
//! that have already played this career. `EventInfo` exposes plain `Int32` getters
//! (`get_EventId` / `get_StoryId` / `get_CharaId`) — no ObscuredInt, no `Convert`,
//! no mutation. Used to count how many of a support card's chain events have fired.

use std::ffi::c_void;

use super::chain::get_single_mode_data;
use super::il2cpp::{call_i32, call_obj, call_obj_with_i32, dict_try_get_obj, resolve_obj_method};

/// One fired event. `event_id` / `story_id` are plain (decrypted) ints used to
/// match against a support card's catalogued chain keys.
#[derive(Debug, Clone, Copy)]
pub struct FiredEvent {
    pub event_id: i32,
    pub story_id: i32,
}

/// `SingleModeEventPlayTiming` is a small enum; probe this many key values.
const MAX_TIMING_KEYS: i32 = 32;

/// Read all events that have fired this career. Empty on failure / no career.
pub fn read_fired_events() -> Vec<FiredEvent> {
    // SAFETY: all reads are on resolved IL2CPP metadata / live objects.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { inner() })) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_fired_events PANICKED");
            Vec::new()
        }
    }
}

unsafe fn inner() -> Vec<FiredEvent> {
    let Some(wsmd) = get_single_mode_data() else {
        return Vec::new();
    };
    // SAFETY: 0-arg getter on a valid WorkSingleModeData.
    let Some(m_dic) = (unsafe { resolve_obj_method(wsmd, "get_StoryInfoListDic", 0) }) else {
        return Vec::new();
    };
    // SAFETY: returns the Dictionary (or null).
    let dict = unsafe { call_obj(wsmd, m_dic) };
    if dict.is_null() {
        return Vec::new();
    }
    // SAFETY: TryGetValue(enumKey, out List) on the dictionary.
    let Some(m_try) = (unsafe { resolve_obj_method(dict, "TryGetValue", 2) }) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut m_count: *const c_void = std::ptr::null();
    let mut m_item: *const c_void = std::ptr::null();
    let mut m_event: *const c_void = std::ptr::null();
    let mut m_story: *const c_void = std::ptr::null();

    // The dict is keyed by the (small) play-timing enum; probe each value.
    for key in 0..MAX_TIMING_KEYS {
        // SAFETY: TryGetValue with a value-type (enum) key; null when absent.
        let list = unsafe { dict_try_get_obj(dict, m_try, key) };
        if list.is_null() {
            continue;
        }
        if m_count.is_null() {
            // SAFETY: list methods resolved off the live List object.
            let c = unsafe { resolve_obj_method(list, "get_Count", 0) };
            // SAFETY: list methods resolved off the live List object.
            let it = unsafe { resolve_obj_method(list, "get_Item", 1) };
            let (Some(c), Some(it)) = (c, it) else {
                continue;
            };
            m_count = c;
            m_item = it;
        }
        // SAFETY: get_Count on a valid List.
        let count = unsafe { call_i32(list, m_count) };
        if !(0..=512).contains(&count) {
            continue;
        }
        for i in 0..count {
            // SAFETY: get_Item(i) within bounds returns an EventInfo reference.
            let info = unsafe { call_obj_with_i32(list, m_item, i) };
            if info.is_null() {
                continue;
            }
            if m_event.is_null() {
                // SAFETY: getter resolved off the live EventInfo object.
                let e = unsafe { resolve_obj_method(info, "get_EventId", 0) };
                // SAFETY: getter resolved off the live EventInfo object.
                let s = unsafe { resolve_obj_method(info, "get_StoryId", 0) };
                let (Some(e), Some(s)) = (e, s) else {
                    return out;
                };
                m_event = e;
                m_story = s;
            }
            // SAFETY: plain Int32 getter on a valid EventInfo.
            let event_id = unsafe { call_i32(info, m_event) };
            // SAFETY: plain Int32 getter on a valid EventInfo.
            let story_id = unsafe { call_i32(info, m_story) };
            out.push(FiredEvent { event_id, story_id });
        }
    }
    out
}
