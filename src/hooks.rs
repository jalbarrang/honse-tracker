//! Host lifecycle subscription.
//!
//! The plugin reads career state directly from game memory (see `memory_reader`),
//! driven entirely by passive game edges. Hooks advance one career lifecycle;
//! the per-frame pump schedules held captures only in `CommandSelectActive`.
//! We subscribe to frame (atomic pump), view-change (unsafe classification), and
//! shutdown (tear down hooks).

use std::ffi::c_void;

use crate::compat::{capability, event, Sdk};

/// Fired once per rendered frame on the render thread (`data` is null). Drives
/// the capture pump — atomic bookkeeping only, no IL2CPP access and no career
/// read; a no-op unless a capture request is held in `CommandSelectActive`.
/// The Independent Training watcher rides along under the same rule: it
/// compares a remembered deadline against the clock and schedules its own read
/// onto the main thread when it needs one.
extern "C" fn on_frame(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::career_poll::tick();
    crate::idle_training::tick();
}

/// Fired when the game changes view/scene. Records the transition (suspending
/// IL2CPP reads during the teardown/rebuild window — reading the Single Mode
/// `HomeInfo`/`TurnInfo` objects mid-transition races a use-after-free and
/// crashes the game) and holds a capture request for the settled post-transition
/// state (career entry/exit, race and story-event returns).
extern "C" fn on_view_change(_event_id: u32, data: *const c_void, _userdata: *mut c_void) {
    // t-001 diagnostic: log the view edge (fires on the render/present thread —
    // never read IL2CPP career objects here, so try_read_turn stays false).
    let view_id = if data.is_null() {
        None
    } else {
        // SAFETY: VIEW_CHANGE dispatches a `ViewChangeEvent` valid for the
        // callback duration (see honse-services event contract).
        Some(unsafe { (*data.cast::<crate::compat::ViewChangeEvent>()).view_id })
    };
    // A malformed event still fails closed as an unknown/cutscene view.
    crate::career_poll::note_view_change(view_id.unwrap_or(i32::MIN));
    crate::career_poll::diag_settle_edge("ViewChange", "event", "view_change", view_id, false);
}

extern "C" fn on_shutdown(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::career_poll::shutdown();
    crate::command_hooks::uninstall();
    crate::apply_hooks::uninstall();
    crate::idle_export::uninstall();
    hachimi_telemetry::shutdown();
    hlog_info!("Shutdown: capture stopped, hooks removed");
}

/// Subscribe to the host events we need. Returns `true` if the host advertises the
/// events capability (required for the shutdown teardown).
pub fn subscribe_events() -> bool {
    let sdk = Sdk::get();
    if !sdk.has_capability(capability::EVENTS) {
        hlog_warn!("Host does not advertise the EVENTS capability");
        return false;
    }
    sdk.on(event::SHUTDOWN, on_shutdown, std::ptr::null_mut());
    sdk.on(event::FRAME, on_frame, std::ptr::null_mut());
    sdk.on(event::VIEW_CHANGE, on_view_change, std::ptr::null_mut());
    true
}
