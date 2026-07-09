//! Host event subscriptions: teardown on shutdown.
//!
//! The 100ms live cadence lives in the RaceManager hook (see `capture.rs`).
//! Resetting the overlay is manual (the Reset button); there is no auto-reset.

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};

use honse_services::SHUTDOWN;

extern "C" fn on_shutdown(event_id: u32, _data: *const c_void, _userdata: *mut c_void) {
    if event_id != SHUTDOWN {
        return;
    }
    let _ = panic::catch_unwind(AssertUnwindSafe(|| {
        crate::capture::uninstall();
        hachimi_telemetry::shutdown();
        hlog_info!(target: "race-hud", "Shutdown: hooks removed, state cleared");
    }));
}

/// Subscribe to the host events the plugin needs.
///
/// Capability checks deleted: single-version world — EVENTS always available.
pub fn subscribe_events() -> bool {
    honse_services::on(SHUTDOWN, on_shutdown, std::ptr::null_mut());
    true
}
