//! Host lifecycle subscription.
//!
//! The plugin reads career state directly from game memory (see `memory_reader`).
//! Tracking is fully manual — only the user's Start/Stop control toggles it — so
//! there is no career start/end auto-lifecycle here. We subscribe to per-frame
//! (drive the throttled refresh), view-change (suspend reads during transitions),
//! and shutdown (tear down hooks).

use std::ffi::c_void;

use crate::compat::{capability, event, Sdk};

/// Fired once per rendered frame on the render thread (`data` is null). Drive the
/// overlay-cache refresh here so career snapshots are read/published even when the
/// tracker overlay (or any of its tabs) is not being drawn. The refresh itself is
/// throttled to [`crate::overlay_cache::AUTO_REFRESH_INTERVAL_MS`] and is a no-op
/// when tracking is off, so calling it every frame is cheap.
extern "C" fn on_frame(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::overlay_cache::maybe_request_refresh();
}

/// Fired when the game changes view/scene. Record the transition so the overlay
/// cache suspends its IL2CPP reads during the teardown/rebuild window: reading the
/// Single Mode `HomeInfo`/`TurnInfo` objects mid-transition (e.g. right after the
/// player clicks a training) races a use-after-free and crashes the game.
extern "C" fn on_view_change(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::overlay_cache::note_view_change();
}

extern "C" fn on_shutdown(_event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    crate::memory_reader::stop_tracking();
    crate::overlay_cache::shutdown();
    crate::shop_hooks::uninstall_shop_hooks();
    hachimi_telemetry::shutdown();
    hlog_info!("Shutdown: tracking stopped, hooks removed");
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
