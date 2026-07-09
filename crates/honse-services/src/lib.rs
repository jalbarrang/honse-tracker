//! Self-hosted services that upstream hachimi-edge does not expose to plugins.
//!
//! Layering: this crate depends on `edge-sdk`. `edge-sdk` must never depend on
//! this crate (hiker law `sdk_depends_on_services`).

pub mod event;
pub mod events;
pub mod frame;

pub use event::{EventFn, ViewChangeEvent, FRAME, SHUTDOWN, VIEW_CHANGE};
pub use events::{dispatch, dispatch_shutdown, dispatch_view_change, off, on};
pub use frame::{install_frame_source, register_frame_job, FrameJob};

use std::sync::atomic::{AtomicU64, Ordering};

/// Shared monotonic handle allocator for event subscriptions and (later) GUI/hotkey regs.
/// `0` is reserved for failure / unused.
static HANDLE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Allocate a fresh non-zero registration handle.
#[must_use]
pub fn next_handle() -> u64 {
    HANDLE_COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) static TEST_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
