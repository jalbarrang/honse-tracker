//! View-change signal via per-frame `SceneManager.GetCurrentViewId()` polling.
//!
//! # Why polling, not a hook
//!
//! The fork hooked `SceneManager.ChangeView` to learn the next view id. Edge
//! **already** hooks that same address, and MinHook refuses a second hook on one
//! target (`MH_ERROR_ALREADY_CREATED`) — a plugin `ChangeView` hook always fails.
//! Edge exposes no view-change callback to plugins either. So instead we read
//! `Gallop.SceneManager.GetCurrentViewId()` (0-arg → i32, confirmed in edge
//! `src/il2cpp/hook/umamusume/SceneManager.rs`) once per present tick and
//! dispatch [`dispatch_view_change`] when it changes.
//!
//! The read gate ([`crate::events`] consumers) arms a multi-second cooldown on
//! each change, so detecting the transition ~1 frame late is fine; the
//! command-suspend hooks cover the precise command-submit path separately.

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use edge_sdk::ffi::{Il2CppClass, Il2CppObject, MethodInfo};
use edge_sdk::Sdk;

use crate::events::dispatch_view_change;

/// Whether any consumer wants view-change events. Default off: the poll does a
/// (cheap but non-free) IL2CPP singleton resolve + getter each frame, so we only
/// pay it while something actually needs the gate (tracker while tracking, the
/// debug viewer always). See `set_view_poll_enabled`.
static POLL_ENABLED: AtomicBool = AtomicBool::new(false);
static POLL_READY: AtomicBool = AtomicBool::new(false);
static SCENE_MANAGER_KLASS: AtomicUsize = AtomicUsize::new(0);
static GET_CURRENT_VIEW_ID_ADDR: AtomicUsize = AtomicUsize::new(0);
static GET_CURRENT_VIEW_ID_MI: AtomicUsize = AtomicUsize::new(0);
/// Last observed view id; -1 = none yet (first observation does not dispatch).
static LAST_VIEW_ID: AtomicI32 = AtomicI32::new(-1);

/// Compiled 0-arg IL2CPP instance getter returning i32: `(this, MethodInfo*)`.
type GetI32Fn = extern "C" fn(this: *mut Il2CppObject, method: *const MethodInfo) -> i32;

/// Resolve `SceneManager.GetCurrentViewId` once the game runtime is up.
/// Called from the first-present bootstrap ([`crate::init::poll_bootstrap`]).
pub fn install_view_poll() {
    if POLL_READY.load(Ordering::Acquire) {
        return;
    }
    let Some(sdk) = Sdk::try_get() else {
        log::warn!("honse-services: install_view_poll before Sdk init");
        return;
    };
    let Some(image) = sdk.get_assembly_image("umamusume.dll") else {
        log::error!("honse-services: umamusume.dll image not found; view gate disabled");
        return;
    };
    let Some(class) = sdk.get_class(image, "Gallop", "SceneManager") else {
        log::error!("honse-services: Gallop.SceneManager not found; view gate disabled");
        return;
    };
    let Some(addr) = sdk.get_method_addr(class, "GetCurrentViewId", 0) else {
        log::error!("honse-services: SceneManager.GetCurrentViewId not found; view gate disabled");
        return;
    };
    let Some(mi) = sdk.get_method(class, "GetCurrentViewId", 0) else {
        log::error!("honse-services: SceneManager.GetCurrentViewId MethodInfo not found; view gate disabled");
        return;
    };
    SCENE_MANAGER_KLASS.store(class as usize, Ordering::Release);
    GET_CURRENT_VIEW_ID_ADDR.store(addr as usize, Ordering::Release);
    GET_CURRENT_VIEW_ID_MI.store(mi as usize, Ordering::Release);
    POLL_READY.store(true, Ordering::Release);
    log::info!("honse-services: view-id poll installed (SceneManager.GetCurrentViewId)");
}

/// Enable or disable per-frame view-change polling for THIS plugin's instance.
///
/// Off by default so an idle plugin costs nothing. Enable it while you actually
/// consume VIEW_CHANGE (e.g. the tracker turns it on in `start_tracking` and off
/// in `stop_tracking`; the debug viewer leaves it on). Disabling resets the
/// change baseline so re-enabling never dispatches a stale diff.
pub fn set_view_poll_enabled(enabled: bool) {
    POLL_ENABLED.store(enabled, Ordering::Release);
    if !enabled {
        LAST_VIEW_ID.store(-1, Ordering::Release);
    }
}

/// Read the current view id and dispatch VIEW_CHANGE if it changed. Called every
/// present tick by the frame source. No-op unless polling is enabled AND
/// [`install_view_poll`] has resolved the getter.
pub fn poll_view_change() {
    if !POLL_ENABLED.load(Ordering::Acquire) || !POLL_READY.load(Ordering::Acquire) {
        return;
    }
    let addr = GET_CURRENT_VIEW_ID_ADDR.load(Ordering::Acquire);
    if addr == 0 {
        return;
    }
    let Some(sdk) = Sdk::try_get() else {
        return;
    };
    let klass = SCENE_MANAGER_KLASS.load(Ordering::Acquire) as *mut Il2CppClass;
    let Some(singleton) = sdk.get_singleton(klass) else {
        return; // SceneManager instance not up (early boot / between scenes)
    };
    let mi = GET_CURRENT_VIEW_ID_MI.load(Ordering::Acquire) as *const MethodInfo;
    // SAFETY: addr is the compiled 0-arg GetCurrentViewId; singleton is the live
    // SceneManager; mi is its MethodInfo. Standard IL2CPP (this, MethodInfo*) call.
    let f: GetI32Fn = unsafe { std::mem::transmute::<usize, GetI32Fn>(addr) };
    let id = f(singleton, mi);

    let prev = LAST_VIEW_ID.swap(id, Ordering::AcqRel);
    if prev != id && prev != -1 {
        if let Some(name) = crate::scene_views::view_name(id) {
            log::debug!("honse-services: view {prev} -> {id} ({name})");
        } else {
            log::debug!("honse-services: view {prev} -> {id}");
        }
        dispatch_view_change(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Change detection is edge-triggered and skips the first observation.
    #[test]
    fn change_detection_logic() {
        // Model the swap+compare the poll uses, without IL2CPP.
        let last = AtomicI32::new(-1);
        let step = |id: i32| -> bool {
            let prev = last.swap(id, Ordering::AcqRel);
            prev != id && prev != -1
        };
        assert!(!step(101), "first observation never dispatches");
        assert!(!step(101), "same id does not dispatch");
        assert!(step(1101), "change dispatches");
        assert!(!step(1101), "settle");
        assert!(step(101), "change back dispatches");
    }
}
