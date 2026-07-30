//! Server-response hooks (`Gallop.WorkSingleModeData.Apply*`).
//!
//! These fire on the main thread immediately after the game writes fresh state
//! from the server response into the working data objects. Hooking here gives
//! the earliest safe capture point — objects are stable, fully written, and on
//! the correct thread. Every hook calls the original first, then requests a
//! capture with the view-settle gate cleared (the response itself proves the
//! game is in a stable state).
//!
//! All hooked methods return `void` — no ABI return-value traps.

use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::compat::{Il2CppObject, Sdk};

// ── Trampolines ─────────────────────────────────────────────────────────────

static mut ORIG_APPLY_EXEC_COMMAND: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_RACE_ENTRY: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_RACE_END: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_RACE_OUT: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_CHECK_EVENT: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_CONTINUE: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_START: *mut c_void = std::ptr::null_mut();
static mut ORIG_APPLY_LOAD: *mut c_void = std::ptr::null_mut();

static INSTALLED: AtomicUsize = AtomicUsize::new(0);

/// After any Apply hook: clear the settle gate and request a capture.
/// The server response just wrote fresh state — safe to read immediately.
fn on_applied(label: &str) {
    hlog_info!(target: "settle-diag", "Apply hook fired: {label} — requesting capture");
    crate::career_poll::clear_settle_gate();
    crate::career_poll::request_capture();
}

// ── Hook functions ──────────────────────────────────────────────────────────

// ApplyExecCommand(SingleModeChara, RaceCondition[], SingleModeHomeInfo, SingleModeEventInfo[])
type ApplyExecCommandFn = extern "C" fn(*mut Il2CppObject, usize, usize, usize, usize);

extern "C" fn ApplyExecCommand(this: *mut Il2CppObject, a: usize, b: usize, c: usize, d: usize) {
    let orig: ApplyExecCommandFn = unsafe { std::mem::transmute(ORIG_APPLY_EXEC_COMMAND) };
    orig(this, a, b, c, d);
    on_applied("ApplyExecCommand");
}

// ApplyRaceEntry(SingleModeChara, SingleModeHomeInfo, SingleRaceStartInfo, SingleModeEventInfo[])
type ApplyRaceEntryFn = extern "C" fn(*mut Il2CppObject, usize, usize, usize, usize);

extern "C" fn ApplyRaceEntry(this: *mut Il2CppObject, a: usize, b: usize, c: usize, d: usize) {
    let orig: ApplyRaceEntryFn = unsafe { std::mem::transmute(ORIG_APPLY_RACE_ENTRY) };
    orig(this, a, b, c, d);
    on_applied("ApplyRaceEntry");
}

// ApplyRaceEnd(CharaRaceReward, SingleModeChara, SingleModeHomeInfo, UserMusic,
//              SingleRaceHistory[], Int32[], SingleModeRaceCondition[], SingleModeRaceAddRewardInfo[])
type ApplyRaceEndFn = extern "C" fn(*mut Il2CppObject, usize, usize, usize, usize, usize, usize, usize, usize);

extern "C" fn ApplyRaceEnd(
    this: *mut Il2CppObject, a: usize, b: usize, c: usize, d: usize,
    e: usize, f: usize, g: usize, h: usize,
) {
    let orig: ApplyRaceEndFn = unsafe { std::mem::transmute(ORIG_APPLY_RACE_END) };
    orig(this, a, b, c, d, e, f, g, h);
    on_applied("ApplyRaceEnd");
}

// ApplyRaceOut(SingleModeChara, SingleModeHomeInfo, SingleModeEventInfo[])
type ApplyRaceOutFn = extern "C" fn(*mut Il2CppObject, usize, usize, usize);

extern "C" fn ApplyRaceOut(this: *mut Il2CppObject, a: usize, b: usize, c: usize) {
    let orig: ApplyRaceOutFn = unsafe { std::mem::transmute(ORIG_APPLY_RACE_OUT) };
    orig(this, a, b, c);
    on_applied("ApplyRaceOut");
}

// ApplyCheckEvent(SingleModeChara, SingleModeHomeInfo, SingleModeEventInfo[],
//                 SuccessionEffectedFactor[], SingleModeRaceCondition[], SingleRaceStartInfo)
type ApplyCheckEventFn = extern "C" fn(*mut Il2CppObject, usize, usize, usize, usize, usize, usize);

extern "C" fn ApplyCheckEvent(
    this: *mut Il2CppObject, a: usize, b: usize, c: usize,
    d: usize, e: usize, f: usize,
) {
    let orig: ApplyCheckEventFn = unsafe { std::mem::transmute(ORIG_APPLY_CHECK_EVENT) };
    orig(this, a, b, c, d, e, f);
    on_applied("ApplyCheckEvent");
}

// ApplyContinue(SingleModeChara, SingleModeHomeInfo, SingleRaceStartInfo, SingleModeEventInfo[])
type ApplyContinueFn = extern "C" fn(*mut Il2CppObject, usize, usize, usize, usize);

extern "C" fn ApplyContinue(this: *mut Il2CppObject, a: usize, b: usize, c: usize, d: usize) {
    let orig: ApplyContinueFn = unsafe { std::mem::transmute(ORIG_APPLY_CONTINUE) };
    orig(this, a, b, c, d);
    on_applied("ApplyContinue");
}

// ApplySingleModeStartResponse(SingleModeStartCommon)
type ApplyStartFn = extern "C" fn(*mut Il2CppObject, usize);

extern "C" fn ApplyStart(this: *mut Il2CppObject, a: usize) {
    let orig: ApplyStartFn = unsafe { std::mem::transmute(ORIG_APPLY_START) };
    orig(this, a);
    on_applied("ApplySingleModeStartResponse");
}

// ApplySingleModeLoadResponse(SingleModeLoadCommon)
type ApplyLoadFn = extern "C" fn(*mut Il2CppObject, usize);

extern "C" fn ApplyLoad(this: *mut Il2CppObject, a: usize) {
    let orig: ApplyLoadFn = unsafe { std::mem::transmute(ORIG_APPLY_LOAD) };
    orig(this, a);
    on_applied("ApplySingleModeLoadResponse");
}

// ── Installation ────────────────────────────────────────────────────────────

struct HookSpec {
    name: &'static str,
    arity: i32,
    hook_fn: *mut c_void,
    trampoline: &'static mut *mut c_void,
    bit: usize,
}

/// Install Apply hooks on `WorkSingleModeData`. Best-effort: each hook is
/// independent, partial success is fine (log which ones landed).
pub fn install() -> bool {
    if INSTALLED.load(Ordering::Acquire) != 0 {
        return true;
    }
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        hlog_warn!(target: "training-tracker", "apply_hooks: umamusume.dll not found");
        return false;
    };
    let Some(klass) = sdk.get_class(img, "Gallop", "WorkSingleModeData") else {
        hlog_warn!(target: "training-tracker", "apply_hooks: WorkSingleModeData not found");
        return false;
    };

    // SAFETY: static mut trampolines are written once here before hooks fire.
    let specs: &mut [HookSpec] = unsafe {
        &mut [
            HookSpec { name: "ApplyExecCommand", arity: 4, hook_fn: ApplyExecCommand as *mut c_void, trampoline: &mut ORIG_APPLY_EXEC_COMMAND, bit: 1 },
            HookSpec { name: "ApplyRaceEntry", arity: 4, hook_fn: ApplyRaceEntry as *mut c_void, trampoline: &mut ORIG_APPLY_RACE_ENTRY, bit: 2 },
            HookSpec { name: "ApplyRaceEnd", arity: 8, hook_fn: ApplyRaceEnd as *mut c_void, trampoline: &mut ORIG_APPLY_RACE_END, bit: 4 },
            HookSpec { name: "ApplyRaceOut", arity: 3, hook_fn: ApplyRaceOut as *mut c_void, trampoline: &mut ORIG_APPLY_RACE_OUT, bit: 8 },
            HookSpec { name: "ApplyCheckEvent", arity: 6, hook_fn: ApplyCheckEvent as *mut c_void, trampoline: &mut ORIG_APPLY_CHECK_EVENT, bit: 16 },
            HookSpec { name: "ApplyContinue", arity: 4, hook_fn: ApplyContinue as *mut c_void, trampoline: &mut ORIG_APPLY_CONTINUE, bit: 32 },
            HookSpec { name: "ApplySingleModeStartResponse", arity: 1, hook_fn: ApplyStart as *mut c_void, trampoline: &mut ORIG_APPLY_START, bit: 64 },
            HookSpec { name: "ApplySingleModeLoadResponse", arity: 1, hook_fn: ApplyLoad as *mut c_void, trampoline: &mut ORIG_APPLY_LOAD, bit: 128 },
        ]
    };

    let mut mask = 0usize;
    for spec in specs.iter_mut() {
        if let Some(addr) = sdk.get_method_addr(klass, spec.name, spec.arity) {
            if let Some(tramp) = sdk.hook(addr, spec.hook_fn) {
                *spec.trampoline = tramp;
                mask |= spec.bit;
                hlog_info!(target: "training-tracker", "apply_hooks: hooked {}", spec.name);
            } else {
                hlog_warn!(target: "training-tracker", "apply_hooks: hook failed for {}", spec.name);
            }
        } else {
            hlog_warn!(target: "training-tracker", "apply_hooks: method not found: {} (arity {})", spec.name, spec.arity);
        }
    }

    if mask != 0 {
        INSTALLED.store(mask, Ordering::Release);
        hlog_info!(target: "training-tracker", "apply_hooks: installed mask={mask:#010b} ({}/8)", mask.count_ones());
        true
    } else {
        hlog_warn!(target: "training-tracker", "apply_hooks: no hooks installed");
        false
    }
}

/// Remove Apply hooks. Idempotent.
pub fn uninstall() {
    let mask = INSTALLED.swap(0, Ordering::AcqRel);
    if mask == 0 {
        return;
    }
    let sdk = Sdk::get();
    let hooks: &[(*mut c_void, usize)] = &[
        (ApplyExecCommand as *mut c_void, 1),
        (ApplyRaceEntry as *mut c_void, 2),
        (ApplyRaceEnd as *mut c_void, 4),
        (ApplyRaceOut as *mut c_void, 8),
        (ApplyCheckEvent as *mut c_void, 16),
        (ApplyContinue as *mut c_void, 32),
        (ApplyStart as *mut c_void, 64),
        (ApplyLoad as *mut c_void, 128),
    ];
    for &(fn_ptr, bit) in hooks {
        if mask & bit != 0 {
            sdk.unhook(fn_ptr);
        }
    }
    // SAFETY: hooks no longer fire once unhooked.
    unsafe {
        ORIG_APPLY_EXEC_COMMAND = std::ptr::null_mut();
        ORIG_APPLY_RACE_ENTRY = std::ptr::null_mut();
        ORIG_APPLY_RACE_END = std::ptr::null_mut();
        ORIG_APPLY_RACE_OUT = std::ptr::null_mut();
        ORIG_APPLY_CHECK_EVENT = std::ptr::null_mut();
        ORIG_APPLY_CONTINUE = std::ptr::null_mut();
        ORIG_APPLY_START = std::ptr::null_mut();
        ORIG_APPLY_LOAD = std::ptr::null_mut();
    }
}
