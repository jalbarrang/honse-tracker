//! Host lifecycle subscription.
//!
//! The plugin reads career state directly from game memory (see `memory_reader`),
//! driven entirely by passive game edges: command-settle hooks and view changes
//! request captures, and the per-frame pump schedules them once both crash-safety
//! gates are open. We subscribe to per-frame (drive the atomic pump), view-change
//! (suspend reads during transitions + request the post-transition capture), and
//! shutdown (tear down hooks).

use std::ffi::c_void;

use crate::compat::{capability, event, Sdk};

/// Fired once per rendered frame on the render thread (`data` is null). Drives
/// the capture pump — atomic bookkeeping only, no IL2CPP access and no career
/// read; a no-op unless a capture request is held and both gates are open.
extern "C" fn on_frame(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::career_poll::tick();
}

/// Fired when the game changes view/scene. Records the transition (suspending
/// IL2CPP reads during the teardown/rebuild window — reading the Single Mode
/// `HomeInfo`/`TurnInfo` objects mid-transition races a use-after-free and
/// crashes the game) and holds a capture request for the settled post-transition
/// state (career entry/exit, race and story-event returns).
extern "C" fn on_view_change(_event_id: u32, data: *const c_void, _userdata: *mut c_void) {
    crate::career_poll::note_view_change();
    // t-001 diagnostic: log the view edge (fires on the render/present thread —
    // never read IL2CPP career objects here, so try_read_turn stays false).
    let view_id = if data.is_null() {
        None
    } else {
        // SAFETY: VIEW_CHANGE dispatches a `ViewChangeEvent` valid for the
        // callback duration (see honse-services event contract).
        Some(unsafe { (*data.cast::<crate::compat::ViewChangeEvent>()).view_id })
    };
    crate::career_poll::diag_settle_edge("ViewChange", "event", "view_change", view_id, false);
}

extern "C" fn on_shutdown(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::career_poll::shutdown();
    crate::command_hooks::uninstall();
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
    // t-001 diagnostic (TEMPORARY): arm the per-frame view-id poll from startup
    // so VIEW_CHANGE edges (career entry, first-session rows of the coverage
    // matrix) are observable before the first capture. Production arming is
    // career-scoped in `career_poll` — armed after the first active-career
    // capture, disarmed on career end/shutdown — so once the runtime matrix is
    // recorded this startup arm is removed and the career lifecycle owns the
    // poll alone. The poll itself is a cheap singleton+getter read.
    honse_services::set_view_poll_enabled(true);
    true
}
