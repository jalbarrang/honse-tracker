//! Host→plugin event dispatch (ported from fork `core/plugin/events.rs`).
//!
//! Callbacks are snapshotted under a short lock and invoked with the lock
//! released so a callback may safely (un)subscribe. Each invoke is wrapped in
//! `catch_unwind` so a misbehaving subscriber cannot take down the render thread.

use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::{
    event::{EventFn, ViewChangeEvent, FRAME, SHUTDOWN, VIEW_CHANGE},
    next_handle,
};

struct Subscription {
    handle: u64,
    event_id: u32,
    callback: EventFn,
    /// Stored as `usize` so `Subscription` is `Send + Sync` (raw `*mut c_void` is not).
    userdata: usize,
}

static SUBSCRIPTIONS: Lazy<Mutex<Vec<Subscription>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Register an event callback. Returns a non-zero handle.
pub fn on(event_id: u32, callback: EventFn, userdata: *mut c_void) -> u64 {
    let handle = next_handle();
    SUBSCRIPTIONS.lock().push(Subscription {
        handle,
        event_id,
        callback,
        userdata: userdata as usize,
    });
    handle
}

/// Remove a subscription by handle.
pub fn off(handle: u64) {
    SUBSCRIPTIONS.lock().retain(|s| s.handle != handle);
}

/// Invoke every callback registered for `event_id`. `data` is event-specific.
pub fn dispatch(event_id: u32, data: *const c_void) {
    // Snapshot matching callbacks, then release the lock before invoking so a
    // callback may safely (un)subscribe (fork events.rs ~64-82).
    let targets: Vec<(EventFn, usize)> = {
        let subs = SUBSCRIPTIONS.lock();
        if subs.is_empty() {
            return;
        }
        subs.iter()
            .filter(|s| s.event_id == event_id)
            .map(|s| (s.callback, s.userdata))
            .collect()
    };

    for (callback, userdata) in targets {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            callback(event_id, data, userdata as *mut c_void);
        }));
    }
}

/// Fired once per rendered frame.
pub fn dispatch_frame() {
    dispatch(FRAME, std::ptr::null());
}

/// Fired when the game changes view/scene.
pub fn dispatch_view_change(view_id: i32) {
    let payload = ViewChangeEvent { view_id };
    dispatch(VIEW_CHANGE, std::ptr::from_ref(&payload).cast());
}

/// Dispatch `SHUTDOWN` to every subscription (any event_id), then drop all.
///
/// Call from `DllMain` `DLL_PROCESS_DETACH` (best-effort). Matches fork
/// `shutdown_and_remove_owner` (~87-104): invoke every owned callback with
/// `SHUTDOWN`, then retain-away — so plugins that only subscribed to FRAME
/// still get a chance to unhook. Process-wide variant clears everyone.
pub fn dispatch_shutdown() {
    let targets: Vec<(EventFn, usize)> = {
        let subs = SUBSCRIPTIONS.lock();
        subs.iter().map(|s| (s.callback, s.userdata)).collect()
    };
    for (callback, userdata) in targets {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            callback(SHUTDOWN, std::ptr::null(), userdata as *mut c_void);
        }));
    }
    SUBSCRIPTIONS.lock().clear();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    use crate::TEST_LOCK;

    static HITS: AtomicU32 = AtomicU32::new(0);
    static SHUTDOWN_HITS: AtomicU32 = AtomicU32::new(0);
    static UNSUB_HANDLE: AtomicUsize = AtomicUsize::new(0);

    extern "C" fn count_frame(event_id: u32, _d: *const c_void, _u: *mut c_void) {
        if event_id == FRAME {
            HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    extern "C" fn count_shutdown(event_id: u32, _d: *const c_void, _u: *mut c_void) {
        if event_id == SHUTDOWN {
            SHUTDOWN_HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    extern "C" fn unsubscribe_self(event_id: u32, _d: *const c_void, _u: *mut c_void) {
        if event_id == FRAME {
            let h = UNSUB_HANDLE.load(Ordering::Relaxed) as u64;
            off(h);
            HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    extern "C" fn count_after_unsub(event_id: u32, _d: *const c_void, _u: *mut c_void) {
        if event_id == FRAME {
            HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn clear() {
        SUBSCRIPTIONS.lock().clear();
    }

    #[test]
    fn on_dispatch_off_round_trip() {
        let _guard = TEST_LOCK.lock();
        clear();
        HITS.store(0, Ordering::Relaxed);

        let h = on(FRAME, count_frame, std::ptr::null_mut());
        assert_ne!(h, 0);
        dispatch_frame();
        assert_eq!(HITS.load(Ordering::Relaxed), 1);

        off(h);
        dispatch_frame();
        assert_eq!(HITS.load(Ordering::Relaxed), 1);
        clear();
    }

    #[test]
    fn unsubscribe_during_dispatch_does_not_skip_or_deadlock() {
        let _guard = TEST_LOCK.lock();
        clear();
        HITS.store(0, Ordering::Relaxed);

        // First sub unsubscribes itself mid-dispatch; second must still fire
        // because dispatch snapshots before invoking (fork semantics).
        let h1 = on(FRAME, unsubscribe_self, std::ptr::null_mut());
        UNSUB_HANDLE.store(h1 as usize, Ordering::Relaxed);
        let _h2 = on(FRAME, count_after_unsub, std::ptr::null_mut());

        dispatch_frame();
        assert_eq!(HITS.load(Ordering::Relaxed), 2);

        // First is gone; second remains and fires once more.
        dispatch_frame();
        assert_eq!(HITS.load(Ordering::Relaxed), 3);
        assert_eq!(SUBSCRIPTIONS.lock().len(), 1);
        clear();
    }

    #[test]
    fn shutdown_dispatch_drops_all_subscriptions() {
        let _guard = TEST_LOCK.lock();
        clear();
        SHUTDOWN_HITS.store(0, Ordering::Relaxed);

        on(FRAME, count_shutdown, std::ptr::null_mut());
        on(VIEW_CHANGE, count_shutdown, std::ptr::null_mut());
        assert_eq!(SUBSCRIPTIONS.lock().len(), 2);

        dispatch_shutdown();

        assert_eq!(SHUTDOWN_HITS.load(Ordering::Relaxed), 2);
        assert!(SUBSCRIPTIONS.lock().is_empty());
        clear();
    }
}
