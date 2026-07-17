//! Per-frame job list driven by edge's present callback.
//!
//! `install_frame_source` registers one present callback that runs every job
//! then dispatches `FRAME`. Later tasks (surface watchdog, hotkey poll) add
//! jobs via [`register_frame_job`].

use std::ffi::c_void;

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::events::dispatch_frame;
use edge_sdk::Sdk;

/// A per-frame job. Called from the present callback before `FRAME` dispatch.
pub type FrameJob = Box<dyn FnMut() + Send>;

static FRAME_JOBS: Lazy<Mutex<Vec<FrameJob>>> = Lazy::new(|| Mutex::new(Vec::new()));
static FRAME_SOURCE_INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Append a job that runs every present tick (before `FRAME` event dispatch).
pub fn register_frame_job(job: FrameJob) {
    FRAME_JOBS.lock().push(job);
}

/// Present trampoline: run jobs, then dispatch FRAME.
///
/// # Safety
/// Called by the host on the render thread; `userdata` is unused (null).
unsafe extern "C" fn present_trampoline(_swapchain: *mut c_void, _userdata: *mut c_void) {
    // First-present bootstrap: fire on-game-ready listeners + install the view
    // poll once IL2CPP is up. No-op after the game-ready edge.
    crate::init::poll_bootstrap();
    {
        let mut jobs = FRAME_JOBS.lock();
        for job in jobs.iter_mut() {
            job();
        }
    }
    // View-change gate signal (SceneManager.GetCurrentViewId diff).
    crate::view_hook::poll_view_change();
    dispatch_frame();
}

/// Register the edge present callback that drives the frame job list + FRAME events.
///
/// Idempotent: subsequent calls are no-ops. Requires `Api::init` / `Sdk` to be live.
pub fn install_frame_source() {
    if FRAME_SOURCE_INSTALLED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let Some(sdk) = Sdk::try_get() else {
        log::warn!("honse-services: install_frame_source called before Sdk init");
        FRAME_SOURCE_INSTALLED.store(false, std::sync::atomic::Ordering::SeqCst);
        return;
    };
    if !sdk.register_present_callback(present_trampoline, std::ptr::null_mut()) {
        log::warn!("honse-services: hachimi_register_present_callback failed");
        FRAME_SOURCE_INSTALLED.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn frame_jobs_run_in_registration_order() {
        let order = std::sync::Arc::new(Mutex::new(Vec::new()));
        let a = order.clone();
        let b = order.clone();
        register_frame_job(Box::new(move || a.lock().push(1u32)));
        register_frame_job(Box::new(move || b.lock().push(2u32)));

        {
            let mut jobs = FRAME_JOBS.lock();
            for job in jobs.iter_mut() {
                job();
            }
        }
        assert_eq!(*order.lock(), vec![1, 2]);
        // Leave jobs in place would pollute other tests — clear.
        FRAME_JOBS.lock().clear();
    }

    #[test]
    fn present_trampoline_dispatches_frame() {
        use crate::{event::FRAME, events, TEST_LOCK};

        let _guard = TEST_LOCK.lock();
        FRAME_JOBS.lock().clear();
        events::dispatch_shutdown();
        static HITS: AtomicU32 = AtomicU32::new(0);
        extern "C" fn hit(event_id: u32, _d: *const c_void, _u: *mut c_void) {
            if event_id == FRAME {
                HITS.fetch_add(1, Ordering::Relaxed);
            }
        }
        HITS.store(0, Ordering::Relaxed);
        let _h = events::on(FRAME, hit, std::ptr::null_mut());

        // SAFETY: null userdata; trampoline only runs jobs + dispatch.
        unsafe { present_trampoline(std::ptr::null_mut(), std::ptr::null_mut()) };
        assert_eq!(HITS.load(Ordering::Relaxed), 1);
        events::dispatch_shutdown();
        FRAME_JOBS.lock().clear();
    }
}
