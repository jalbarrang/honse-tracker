//! Career command-flow hooks (`Gallop.SingleModeMainViewController`).
//!
//! Port of the fork host file
//! `apps/hachimi/src/il2cpp/hook/umamusume/SingleModeMainViewController.rs`.
//!
//! Two jobs:
//! 1. Move the lifecycle to `CommandInFlight` before command submission.
//! 2. Announce an actionable command view strictly after either
//!    `SetupCommandSelectStart*` or `OnCompletePlayInCommandView` returns. The
//!    latter covers career start/resume paths that skip command-select setup.
//!
//! ABI trap: both `*SendCommandAsync` return an `IEnumerator` (`*mut Il2CppObject`).
//! The hook MUST forward the trampoline's return value — a void hook leaves
//! garbage in the return register and crashes the game when the coroutine starts.
//!
//! Intentional deviation: fork also dispatched `event::TRAINING_COMMAND` from
//! `SendCommandAsync`. Nothing in the tracker or secondary plugins subscribes;
//! drop the dispatch (see PORT_NOTES.md).

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::compat::{Il2CppObject, Sdk};

static mut ORIG_SEND_COMMAND_ASYNC: *mut c_void = std::ptr::null_mut();
static mut ORIG_COMMON_SEND_COMMAND_ASYNC: *mut c_void = std::ptr::null_mut();
static mut ORIG_SETUP_COMMAND_SELECT_START: *mut c_void = std::ptr::null_mut();
static mut ORIG_SETUP_COMMAND_SELECT_START_STEP_TURN: *mut c_void = std::ptr::null_mut();
static mut ORIG_ON_COMPLETE_PLAY_IN_COMMAND_VIEW: *mut c_void = std::ptr::null_mut();

/// Bitmask of installed hook fn addresses (for uninstall).
static INSTALLED: AtomicUsize = AtomicUsize::new(0);

#[inline]
fn suspend_reads() {
    crate::suspend_reads_for_command();
}

#[inline]
fn announce_settled() {
    crate::resume_reads_on_command_select();
}

#[inline]
fn announce_play_in_completed() {
    crate::reads_on_command_view_play_in_completed();
}

type SendCommandAsyncFn = extern "C" fn(
    this: *mut Il2CppObject,
    command_type: usize,
    command_id: usize,
    command_group_id: usize,
    select_id: usize,
    on_success: usize,
    on_error: usize,
) -> *mut Il2CppObject;

extern "C" fn SendCommandAsync(
    this: *mut Il2CppObject,
    command_type: usize,
    command_id: usize,
    command_group_id: usize,
    select_id: usize,
    on_success: usize,
    on_error: usize,
) -> *mut Il2CppObject {
    // TRAINING_COMMAND event dispatch dropped — no subscribers (PORT_NOTES).
    suspend_reads();
    // Diagnostic after the lifecycle moved to CommandInFlight proves reads were
    // blocked before the original ran. No turn read on submit edges.
    crate::career_poll::diag_settle_edge("SendCommandAsync", "before_original", "command_submit", None, false);
    // SAFETY: trampoline written once during install; signature matches IL2CPP method.
    let orig: SendCommandAsyncFn = unsafe { std::mem::transmute(ORIG_SEND_COMMAND_ASYNC) };
    let ret = orig(
        this,
        command_type,
        command_id,
        command_group_id,
        select_id,
        on_success,
        on_error,
    );
    crate::career_poll::diag_settle_edge("SendCommandAsync", "after_original", "command_submit", None, false);
    ret
}

type CommonSendCommandAsyncFn =
    extern "C" fn(this: *mut Il2CppObject, command_type: usize, command_id: usize) -> *mut Il2CppObject;

extern "C" fn CommonSendCommandAsync(
    this: *mut Il2CppObject,
    command_type: usize,
    command_id: usize,
) -> *mut Il2CppObject {
    // Every command submit (rest / infirmary / outing / training) funnels here.
    suspend_reads();
    // t-001 diagnostic: see SendCommandAsync.
    crate::career_poll::diag_settle_edge(
        "CommonSendCommandAsync",
        "before_original",
        "command_submit",
        None,
        false,
    );
    // SAFETY: trampoline written once during install.
    let orig: CommonSendCommandAsyncFn = unsafe { std::mem::transmute(ORIG_COMMON_SEND_COMMAND_ASYNC) };
    let ret = orig(this, command_type, command_id);
    crate::career_poll::diag_settle_edge(
        "CommonSendCommandAsync",
        "after_original",
        "command_submit",
        None,
        false,
    );
    ret
}

type SetupCommandSelectStartFn = extern "C" fn(this: *mut Il2CppObject, play_voice: bool, to_top: bool);

extern "C" fn SetupCommandSelectStart(this: *mut Il2CppObject, play_voice: bool, to_top: bool) {
    // Hook-entry diagnostic before the original: the lifecycle must still be
    // unsafe until setup has completed.
    crate::career_poll::diag_settle_edge(
        "SetupCommandSelectStart",
        "before_original",
        "command_select_settled",
        None,
        false,
    );
    // SAFETY: trampoline written once during install.
    let orig: SetupCommandSelectStartFn = unsafe { std::mem::transmute(ORIG_SETUP_COMMAND_SELECT_START) };
    orig(this, play_voice, to_top);
    // Original-first ordering: only now are the Single Mode objects rebuilt.
    // Announce the settled turn and hold a capture request; the capture itself
    // runs deferred on a main-thread callback.
    announce_settled();
    // Post-original diagnostic: attempt a lifecycle-checked turn read to prove
    // this is the sole safe state.
    crate::career_poll::diag_settle_edge(
        "SetupCommandSelectStart",
        "after_original",
        "command_select_settled",
        None,
        true,
    );
}

type SetupCommandSelectStartStepTurnFn = extern "C" fn(this: *mut Il2CppObject, play_voice: bool);

extern "C" fn SetupCommandSelectStartStepTurn(this: *mut Il2CppObject, play_voice: bool) {
    // t-001 diagnostic: see SetupCommandSelectStart.
    crate::career_poll::diag_settle_edge(
        "SetupCommandSelectStartStepTurn",
        "before_original",
        "command_select_settled",
        None,
        false,
    );
    // SAFETY: trampoline written once during install.
    let orig: SetupCommandSelectStartStepTurnFn =
        unsafe { std::mem::transmute(ORIG_SETUP_COMMAND_SELECT_START_STEP_TURN) };
    orig(this, play_voice);
    // Original-first: see SetupCommandSelectStart.
    announce_settled();
    crate::career_poll::diag_settle_edge(
        "SetupCommandSelectStartStepTurn",
        "after_original",
        "command_select_settled",
        None,
        true,
    );
}

type OnCompletePlayInCommandViewFn = extern "C" fn(this: *mut Il2CppObject);

extern "C" fn OnCompletePlayInCommandView(this: *mut Il2CppObject) {
    crate::career_poll::diag_settle_edge(
        "OnCompletePlayInCommandView",
        "before_original",
        "command_view_play_in_completed",
        None,
        false,
    );
    // SAFETY: trampoline written once during install; zero-argument instance
    // signature confirmed in the current IL2CPP class dump.
    let orig: OnCompletePlayInCommandViewFn = unsafe { std::mem::transmute(ORIG_ON_COMPLETE_PLAY_IN_COMMAND_VIEW) };
    orig(this);
    announce_play_in_completed();
    crate::career_poll::diag_settle_edge(
        "OnCompletePlayInCommandView",
        "after_original",
        "command_view_play_in_completed",
        None,
        true,
    );
}

/// Install the five command-flow hooks. Idempotent. Returns `true` if all five
/// methods resolved and hooked.
pub fn install() -> bool {
    if INSTALLED.load(Ordering::Acquire) != 0 {
        return true;
    }
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        hlog_warn!(target: "training-tracker", "command_hooks: umamusume.dll not found");
        return false;
    };
    let Some(klass) = sdk.get_class(img, "Gallop", "SingleModeMainViewController") else {
        hlog_warn!(target: "training-tracker", "command_hooks: SingleModeMainViewController not found");
        return false;
    };

    let mut ok = 0usize;

    if let Some(addr) = sdk.get_method_addr(klass, "SendCommandAsync", 6) {
        if let Some(tramp) = sdk.hook(addr, SendCommandAsync as *mut c_void) {
            // SAFETY: written once before hooks fire; trampoline from edge interceptor.
            unsafe {
                ORIG_SEND_COMMAND_ASYNC = tramp;
            }
            ok |= 1;
        }
    }
    if let Some(addr) = sdk.get_method_addr(klass, "CommonSendCommandAsync", 2) {
        if let Some(tramp) = sdk.hook(addr, CommonSendCommandAsync as *mut c_void) {
            // SAFETY: written once before hooks fire; trampoline from edge interceptor.
            unsafe {
                ORIG_COMMON_SEND_COMMAND_ASYNC = tramp;
            }
            ok |= 2;
        }
    }
    if let Some(addr) = sdk.get_method_addr(klass, "SetupCommandSelectStart", 2) {
        if let Some(tramp) = sdk.hook(addr, SetupCommandSelectStart as *mut c_void) {
            // SAFETY: written once before hooks fire; trampoline from edge interceptor.
            unsafe {
                ORIG_SETUP_COMMAND_SELECT_START = tramp;
            }
            ok |= 4;
        }
    }
    if let Some(addr) = sdk.get_method_addr(klass, "SetupCommandSelectStartStepTurn", 1) {
        if let Some(tramp) = sdk.hook(addr, SetupCommandSelectStartStepTurn as *mut c_void) {
            // SAFETY: written once before hooks fire; trampoline from edge interceptor.
            unsafe {
                ORIG_SETUP_COMMAND_SELECT_START_STEP_TURN = tramp;
            }
            ok |= 8;
        }
    }
    if let Some(addr) = sdk.get_method_addr(klass, "OnCompletePlayInCommandView", 0) {
        if let Some(tramp) = sdk.hook(addr, OnCompletePlayInCommandView as *mut c_void) {
            // SAFETY: written once before hooks fire; trampoline from edge interceptor.
            unsafe {
                ORIG_ON_COMPLETE_PLAY_IN_COMMAND_VIEW = tramp;
            }
            ok |= 16;
        }
    }

    if ok == 0b1_1111 {
        INSTALLED.store(ok, Ordering::Release);
        hlog_info!(target: "training-tracker", "command_hooks: all five hooks installed");
        true
    } else {
        hlog_warn!(target: "training-tracker", "command_hooks: partial install mask={ok:#07b}");
        // Best-effort uninstall of whatever landed.
        uninstall();
        false
    }
}

/// Remove command-flow hooks. Idempotent.
pub fn uninstall() {
    let mask = INSTALLED.swap(0, Ordering::AcqRel);
    let sdk = Sdk::get();
    if mask & 1 != 0 {
        sdk.unhook(SendCommandAsync as *mut c_void);
    }
    if mask & 2 != 0 {
        sdk.unhook(CommonSendCommandAsync as *mut c_void);
    }
    if mask & 4 != 0 {
        sdk.unhook(SetupCommandSelectStart as *mut c_void);
    }
    if mask & 8 != 0 {
        sdk.unhook(SetupCommandSelectStartStepTurn as *mut c_void);
    }
    if mask & 16 != 0 {
        sdk.unhook(OnCompletePlayInCommandView as *mut c_void);
    }
    // SAFETY: hooks no longer fire once unhooked.
    unsafe {
        ORIG_SEND_COMMAND_ASYNC = std::ptr::null_mut();
        ORIG_COMMON_SEND_COMMAND_ASYNC = std::ptr::null_mut();
        ORIG_SETUP_COMMAND_SELECT_START = std::ptr::null_mut();
        ORIG_SETUP_COMMAND_SELECT_START_STEP_TURN = std::ptr::null_mut();
        ORIG_ON_COMPLETE_PLAY_IN_COMMAND_VIEW = std::ptr::null_mut();
    }
}
