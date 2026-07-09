//! Skill purchase (write side).
//!
//! Calls the game's own `Gallop.SingleModeAPI.SendGainSkills(GainSkillInfo[],
//! System.Action)` **static** method on the Unity main thread. The method reads
//! its static `SingleModeAPIBase` field (set by the game at career start) and
//! dispatches to the active scenario impl (URA / TeamRace / Free), then sends a
//! server-authoritative `SingleModeGainSkillsRequest`. The server validates SP
//! and prerequisites, so impossible purchases cannot be forged — this is not
//! packet forging, it is the same code path the in-game "Decide" button runs.
//!
//! All IL2CPP object/array/delegate creation and the call itself must run on the
//! main thread. `Thread::schedule` takes a bare `fn()`, so pending purchases are
//! stashed in a static queue and drained on the scheduled callback (mirrors the
//! `STORY_GOTO_BLOCK_PARAMS` pattern in `core/ipc.rs`).

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use crate::compat::Sdk;
use crate::il2cpp::api::il2cpp_object_new;
use crate::il2cpp::symbols::{create_delegate, set_field_value, Array, Thread};
use crate::il2cpp::types::{Il2CppArray, Il2CppClass, Il2CppObject, MethodInfo};
use crate::overlay_cache;

// ---------------------------------------------------------------------------
// Resolved IL2CPP pointers (one-time)
// ---------------------------------------------------------------------------

struct Resolved {
    /// `SingleModeAPI.SendGainSkills(GainSkillInfo[], Action)` — static MethodInfo*.
    send_gain_skills_mi: *const MethodInfo,
    /// `Gallop.GainSkillInfo` class (reference type).
    gain_skill_info_klass: *mut Il2CppClass,
    f_skill_id: *mut c_void,
    f_level: *mut c_void,
    /// `System.Action` class for the onSuccess delegate.
    action_klass: *mut Il2CppClass,
}

// SAFETY: IL2CPP pointers are stable for the process lifetime.
unsafe impl Send for Resolved {}
// SAFETY: IL2CPP pointers are stable for the process lifetime.
unsafe impl Sync for Resolved {}

static RESOLVED: OnceLock<Resolved> = OnceLock::new();

fn try_resolve() -> Result<Resolved, &'static str> {
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        return Err("umamusume.dll not found");
    };

    let Some(sma) = sdk.get_class(img, "Gallop", "SingleModeAPI") else {
        return Err("SingleModeAPI not found");
    };
    let Some(send_gain_skills_mi) = sdk.get_method(sma, "SendGainSkills", 2) else {
        return Err("SendGainSkills method not found");
    };

    let Some(gsi) = sdk.get_class(img, "Gallop", "GainSkillInfo") else {
        return Err("GainSkillInfo not found");
    };
    let Some(f_skill_id) = sdk.get_field_from_name(gsi, "skill_id") else {
        return Err("GainSkillInfo.skill_id field not found");
    };
    let Some(f_level) = sdk.get_field_from_name(gsi, "level") else {
        return Err("GainSkillInfo.level field not found");
    };

    let Some(mscorlib) = sdk.get_assembly_image("mscorlib.dll") else {
        return Err("mscorlib.dll not found");
    };
    let Some(action_klass) = sdk.get_class(mscorlib, "System", "Action") else {
        return Err("System.Action not found");
    };

    hlog_info!("Skill purchase: SingleModeAPI.SendGainSkills chain resolved");
    Ok(Resolved {
        send_gain_skills_mi: send_gain_skills_mi.cast(),
        gain_skill_info_klass: gsi.cast(),
        f_skill_id: f_skill_id.cast(),
        f_level: f_level.cast(),
        action_klass: action_klass.cast(),
    })
}

fn ensure_resolved() -> Option<&'static Resolved> {
    if let Some(r) = RESOLVED.get() {
        return Some(r);
    }
    match try_resolve() {
        Ok(r) => {
            let _ = RESOLVED.set(r);
            RESOLVED.get()
        }
        Err(e) => {
            hlog_error!("Skill purchase resolution failed: {}", e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Main-thread dispatch
// ---------------------------------------------------------------------------

/// Pending `(skill_id, level)` purchases, drained on the main thread.
static PENDING: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());

/// Queue a purchase and schedule the commit on the Unity main thread.
///
/// Caller is responsible for any affordability / confirm gating; this only
/// performs the IL2CPP commit. Safe to call from any thread.
pub(crate) fn request_buy(skill_id: i32, level: i32) {
    PENDING.lock().expect("lock poisoned").push((skill_id, level));
    Thread::main_thread().schedule(drain_and_commit);
}

/// Main-thread callback: build the `GainSkillInfo[]` and call `SendGainSkills`.
fn drain_and_commit() {
    let items: Vec<(i32, i32)> = std::mem::take(&mut *PENDING.lock().expect("lock poisoned"));
    if items.is_empty() {
        return;
    }
    let Some(r) = ensure_resolved() else {
        return;
    };

    // Build GainSkillInfo[] (reference-type array).
    let arr = Array::<*mut Il2CppObject>::new(r.gain_skill_info_klass, items.len());
    if arr.this.is_null() {
        hlog_error!("Skill purchase: failed to allocate GainSkillInfo array");
        return;
    }
    for (i, &(skill_id, level)) in items.iter().enumerate() {
        let obj = il2cpp_object_new(r.gain_skill_info_klass);
        if obj.is_null() {
            hlog_error!("Skill purchase: failed to allocate GainSkillInfo");
            return;
        }
        set_field_value(obj, r.f_skill_id.cast(), &skill_id);
        set_field_value(obj, r.f_level.cast(), &level);
        // SAFETY: `i` is in range [0, len); the array holds object references.
        unsafe { *arr.data_ptr().add(i) = obj };
    }

    // onSuccess delegate (System.Action, 0 args) — refresh the panel + notify.
    let Some(delegate) = create_delegate(r.action_klass, 0, on_buy_success) else {
        hlog_error!("Skill purchase: failed to create onSuccess delegate");
        return;
    };

    // Static method: no `this`; signature is (args..., methodInfo).
    // SAFETY: `send_gain_skills_mi` is a valid MethodInfo*; its method_pointer is
    // the first pointer-sized field. Static call takes the two args + MethodInfo.
    unsafe {
        let method_ptr = *(r.send_gain_skills_mi as *const usize);
        let f: extern "C" fn(*mut Il2CppArray, *mut Il2CppObject, *const MethodInfo) = std::mem::transmute(method_ptr);
        f(arr.this, delegate.cast(), r.send_gain_skills_mi);
    }

    hlog_info!("Skill purchase: SendGainSkills dispatched for {} skill(s)", items.len());
}

/// Invoked by the game when the server confirms the purchase.
fn on_buy_success() {
    overlay_cache::request_refresh_immediate();
    Sdk::get().show_notification("Skill learned");
}
