//! SceneManager `ChangeView` hook — arg-count probe (7 then 5).
//!
//! Fork reference: `hachimi-redux/.../SceneManager.rs` hooks ONE of TWO overloads
//! keyed on region (`Region::Japan` → args_count 7 / `ChangeViewJpfn`; else → 5 /
//! `ChangeViewOtherfn`). Edge exposes no region API, so we probe args_count 7
//! first; if absent, fall back to 5.
//!
//! # Coexistence with edge's own SceneManager hook
//!
//! Edge also hooks `ChangeView` (`hachimi-edge/src/il2cpp/hook/umamusume/SceneManager.rs`).
//! Plugin hooks go through the same process-wide `Interceptor` (`plugin_api.rs`
//! `interceptor_hook` → `Interceptor::hook` → `MinHook::create_hook` in
//! `windows/interceptor_impl.rs`). **MinHook does not chain two hooks on the
//! same target address** (`MH_ERROR_ALREADY_CREATED`); contrary to an earlier
//! plan assumption, a second `create_hook` on the address edge already hooked
//! will fail and `Sdk::hook` returns `None`. We log and continue without the
//! view hook rather than crash. Runtime coexistence may require a future
//! trampoline-chain strategy (out of scope here — do not invent).

use std::{
    ffi::c_void,
    sync::atomic::{AtomicPtr, AtomicUsize, Ordering},
};

use edge_sdk::{ffi::Il2CppObject, Sdk};

use crate::events::dispatch_view_change;

/// Transcribed from fork `SceneManager.rs` `ChangeViewJpfn` (il2cpp args_count 7
/// excludes `this`; extern includes it → 8 params).
type ChangeViewJpfn = extern "C" fn(
    this: *mut Il2CppObject,
    next_view_id: i32,
    view_info: *mut Il2CppObject,
    callback_on_change_view_cancel: *mut Il2CppObject,
    callback_on_change_view_accept: *mut Il2CppObject,
    force_change: bool,
    is_fast_destroy: bool,
    fade_in_duration: f32,
);

/// Transcribed from fork `SceneManager.rs` `ChangeViewOtherfn` (il2cpp args_count 5
/// excludes `this`; extern includes it → 6 params).
type ChangeViewOtherfn = extern "C" fn(
    this: *mut Il2CppObject,
    next_view_id: i32,
    view_info: *mut Il2CppObject,
    callback_on_change_view_cancel: *mut Il2CppObject,
    callback_on_change_view_accept: *mut Il2CppObject,
    force_change: bool,
);

static ORIG_JP: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static ORIG_OTHER: AtomicPtr<c_void> = AtomicPtr::new(std::ptr::null_mut());
static INSTALLED: AtomicUsize = AtomicUsize::new(0);

fn on_change_view(next_view_id: i32) {
    if let Some(name) = crate::scene_views::view_name(next_view_id) {
        log::debug!("ChangeView -> {next_view_id} ({name})");
    } else {
        log::debug!("ChangeView -> {next_view_id} (uncatalogued)");
    }
    dispatch_view_change(next_view_id);
}

extern "C" fn change_view_jp(
    this: *mut Il2CppObject,
    next_view_id: i32,
    view_info: *mut Il2CppObject,
    callback_on_change_view_cancel: *mut Il2CppObject,
    callback_on_change_view_accept: *mut Il2CppObject,
    force_change: bool,
    is_fast_destroy: bool,
    fade_in_duration: f32,
) {
    let orig = ORIG_JP.load(Ordering::Acquire);
    if !orig.is_null() {
        // SAFETY: trampoline from Sdk::hook; signature matches ChangeViewJpfn.
        let f: ChangeViewJpfn = unsafe { std::mem::transmute(orig) };
        f(
            this,
            next_view_id,
            view_info,
            callback_on_change_view_cancel,
            callback_on_change_view_accept,
            force_change,
            is_fast_destroy,
            fade_in_duration,
        );
    }
    on_change_view(next_view_id);
}

extern "C" fn change_view_other(
    this: *mut Il2CppObject,
    next_view_id: i32,
    view_info: *mut Il2CppObject,
    callback_on_change_view_cancel: *mut Il2CppObject,
    callback_on_change_view_accept: *mut Il2CppObject,
    force_change: bool,
) {
    let orig = ORIG_OTHER.load(Ordering::Acquire);
    if !orig.is_null() {
        // SAFETY: trampoline from Sdk::hook; signature matches ChangeViewOtherfn.
        let f: ChangeViewOtherfn = unsafe { std::mem::transmute(orig) };
        f(
            this,
            next_view_id,
            view_info,
            callback_on_change_view_cancel,
            callback_on_change_view_accept,
            force_change,
        );
    }
    on_change_view(next_view_id);
}

/// Probe args_count 7 then 5 and install the matching hook.
///
/// Probe order replaces the fork's region branch (`SceneManager.rs` `init`:
/// `Region::Japan` → 7 / else → 5). If both resolve (unexpected), hook the
/// 7-arg overload and log a warning. If neither resolves, log an error and
/// continue without the hook.
pub fn install_view_hook() {
    if INSTALLED.swap(1, Ordering::SeqCst) != 0 {
        return;
    }
    let Some(sdk) = Sdk::try_get() else {
        log::warn!("honse-services: install_view_hook before Sdk init");
        INSTALLED.store(0, Ordering::SeqCst);
        return;
    };

    let Some(image) = sdk.get_assembly_image("umamusume.dll") else {
        log::error!("honse-services: umamusume.dll image not found; skipping ChangeView hook");
        return;
    };
    let Some(class) = sdk.get_class(image, "Gallop", "SceneManager") else {
        log::error!("honse-services: Gallop.SceneManager not found; skipping ChangeView hook");
        return;
    };

    let addr7 = sdk.get_method_addr(class, "ChangeView", 7);
    let addr5 = sdk.get_method_addr(class, "ChangeView", 5);

    match (addr7, addr5) {
        (Some(a7), Some(_)) => {
            log::warn!("honse-services: both ChangeView overloads (7 and 5) resolved; hooking 7-arg (JP) form");
            install_jp(sdk, a7);
        }
        (Some(a7), None) => install_jp(sdk, a7),
        (None, Some(a5)) => install_other(sdk, a5),
        (None, None) => {
            log::error!("honse-services: neither ChangeView overload (7 nor 5) resolved; continuing without view hook");
        }
    }
}

fn install_jp(sdk: &Sdk, addr: *mut c_void) {
    let hook_ptr = change_view_jp as *mut c_void;
    match sdk.hook(addr, hook_ptr) {
        Some(tramp) => {
            ORIG_JP.store(tramp, Ordering::Release);
            log::info!("honse-services: hooked ChangeView (args_count 7 / JP signature)");
        }
        None => {
            log::error!(
                "honse-services: Sdk::hook failed for ChangeView args_count 7 \
                 (edge may already own this address via MinHook; see module docs)"
            );
        }
    }
}

fn install_other(sdk: &Sdk, addr: *mut c_void) {
    let hook_ptr = change_view_other as *mut c_void;
    match sdk.hook(addr, hook_ptr) {
        Some(tramp) => {
            ORIG_OTHER.store(tramp, Ordering::Release);
            log::info!("honse-services: hooked ChangeView (args_count 5 / non-JP signature)");
        }
        None => {
            log::error!(
                "honse-services: Sdk::hook failed for ChangeView args_count 5 \
                 (edge may already own this address via MinHook; see module docs)"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    /// Documents the probe decision table (pure; no IL2CPP).
    #[test]
    fn probe_prefers_seven_when_both_present() {
        let choose = |a7: bool, a5: bool| -> &'static str {
            match (a7, a5) {
                (true, true) | (true, false) => "jp7",
                (false, true) => "other5",
                (false, false) => "none",
            }
        };
        assert_eq!(choose(true, true), "jp7");
        assert_eq!(choose(true, false), "jp7");
        assert_eq!(choose(false, true), "other5");
        assert_eq!(choose(false, false), "none");
    }
}
