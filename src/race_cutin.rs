//! Skipping the race skill cut-ins.
//!
//! A unique skill firing stops the race, plays a cinematic, and hands the race
//! back. This makes it not play at all: the skill banner still appears and the
//! race carries on, which is the behaviour the mobile client gives you.
//!
//! # Stop it being queued, do not cut it short
//!
//! `RaceSkillCutInReserveCreator.AddCutInInfo` is where a cut-in joins the
//! reserve list, and `FindReserve` is what the race asks before entering the
//! cut-in state. An empty list means the state is never entered and every other
//! piece of race code runs exactly as it always did.
//!
//! Assets may still be loaded: `ReserveData` also carries `HasRareSkillCutIn`
//! and `HasSSRSkillCutIn`, and whether those are set inside `AddCutInInfo` or
//! alongside it is not visible from a class dump. Wasted loading, if so — not
//! a cut-in that plays.
//!
//! The alternative — letting it start and then cutting it short — leaves a
//! state machine mid-flight, with the race UI hidden and race time paused by
//! whoever started it. Not worth it when the list is right there.
//!
//! # The game's own setting is not this
//!
//! `RaceDefine.CutInPlayMode` offers `Long`, `LongOnceADay` and `Short`, and
//! the last one is a real improvement you can turn on in the options menu with
//! no mod at all. What it does not offer is Off, which is the only reason this
//! module exists.
//!
//! # Off by default
//!
//! Every other hook here reports what the game did and changes nothing. This
//! one changes what the game does, so it stays off until asked for, and the
//! hook is not even installed until the first time it is turned on.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compat::{Il2CppObject, Sdk};

static ENABLED: AtomicBool = AtomicBool::new(false);
static INSTALLED: AtomicBool = AtomicBool::new(false);

static mut ORIG_ADD: *mut c_void = std::ptr::null_mut();
static mut ORIG_ADD_NAMED: *mut c_void = std::ptr::null_mut();

/// Whether cut-ins are currently being dropped.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Turn skipping on or off.
///
/// Turning it on installs the hook if it is not installed yet, and refuses —
/// staying off — if that fails, so the flag never claims something the game is
/// not actually doing. Callers that persist the setting should read
/// [`is_enabled`] back rather than saving what they asked for.
pub fn set_enabled(enabled: bool) {
    if enabled && !install() {
        hlog_warn!(target: "training-tracker", "Race cut-in skip: hook unavailable; cut-ins will keep playing");
        return;
    }
    ENABLED.store(enabled, Ordering::Release);
    hlog_info!(target: "training-tracker", "Race cut-in skip: {}", if enabled { "on" } else { "off" });
}

/// Whether to drop this cut-in.
///
/// `RaceManager.CutInCategory` is `Null`, `Eye`, `Unique` and `UniqueRare`.
/// Every one of them interrupts the race to play something, so every one of
/// them goes; the category is logged in case that ever wants to be finer.
fn drop_cut_in(category: i32, skill_id: i32) -> bool {
    if !is_enabled() {
        return false;
    }
    hlog_info!(target: "training-tracker", "Race cut-in skipped: skill {skill_id}, category {category}");
    true
}

// ── Hook functions ──────────────────────────────────────────────────────────
//
// The trailing pointer is IL2CPP's `MethodInfo*`. It is forwarded rather than
// dropped: passing it on is correct whether or not the compiled method reads
// it, and reconstructing one would not be.

// AddCutInInfo(HorseData horseData, RaceManager.CutInCategory category, Int32 skillId, Single time)
type AddFn = extern "C" fn(*mut Il2CppObject, *mut c_void, i32, i32, f32, *mut c_void);

extern "C" fn add_cut_in_info(
    this: *mut Il2CppObject,
    horse: *mut c_void,
    category: i32,
    skill_id: i32,
    time: f32,
    method: *mut c_void,
) {
    if drop_cut_in(category, skill_id) {
        return;
    }
    // SAFETY: written once during install, before the hook can fire.
    let orig: AddFn = unsafe { std::mem::transmute(ORIG_ADD) };
    orig(this, horse, category, skill_id, time, method);
}

// AddCutInInfo(HorseData horseData, RaceManager.CutInCategory category, Int32 skillId,
//              String cutInName, Single time)
type AddNamedFn = extern "C" fn(*mut Il2CppObject, *mut c_void, i32, i32, *mut c_void, f32, *mut c_void);

extern "C" fn add_cut_in_info_named(
    this: *mut Il2CppObject,
    horse: *mut c_void,
    category: i32,
    skill_id: i32,
    cut_in_name: *mut c_void,
    time: f32,
    method: *mut c_void,
) {
    if drop_cut_in(category, skill_id) {
        return;
    }
    // SAFETY: as above.
    let orig: AddNamedFn = unsafe { std::mem::transmute(ORIG_ADD_NAMED) };
    orig(this, horse, category, skill_id, cut_in_name, time, method);
}

// ── Installation ────────────────────────────────────────────────────────────

struct HookSpec {
    /// Overloads share a name and are told apart by argument count.
    arity: i32,
    hook_fn: *mut c_void,
    trampoline: *mut *mut c_void,
}

/// Hook both `AddCutInInfo` overloads. Idempotent, and requires IL2CPP to be up.
///
/// Both must land: one overload left unhooked is a cut-in that still plays,
/// which reads as the feature being broken rather than partly applied.
fn install() -> bool {
    if INSTALLED.load(Ordering::Acquire) {
        return true;
    }
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        hlog_warn!(target: "training-tracker", "race_cutin: umamusume.dll not found");
        return false;
    };
    let Some(klass) = sdk.get_class(img, "Gallop", "RaceSkillCutInReserveCreator") else {
        hlog_warn!(target: "training-tracker", "race_cutin: RaceSkillCutInReserveCreator not found");
        return false;
    };

    // Raw pointers avoid references to mutable statics; each is written once
    // here, before any hook can fire.
    let specs: &mut [HookSpec] = &mut [
        HookSpec {
            arity: 4,
            hook_fn: add_cut_in_info as *mut c_void,
            trampoline: &raw mut ORIG_ADD,
        },
        HookSpec {
            arity: 5,
            hook_fn: add_cut_in_info_named as *mut c_void,
            trampoline: &raw mut ORIG_ADD_NAMED,
        },
    ];

    for spec in specs.iter_mut() {
        let Some(addr) = sdk.get_method_addr(klass, "AddCutInInfo", spec.arity) else {
            hlog_warn!(target: "training-tracker", "race_cutin: AddCutInInfo/{} not found", spec.arity);
            return false;
        };
        let Some(tramp) = sdk.hook(addr, spec.hook_fn) else {
            hlog_warn!(target: "training-tracker", "race_cutin: hook failed for AddCutInInfo/{}", spec.arity);
            return false;
        };
        // SAFETY: each pointer targets one trampoline static, written only here
        // during single-threaded installation.
        unsafe {
            *spec.trampoline = tramp;
        }
    }

    INSTALLED.store(true, Ordering::Release);
    hlog_info!(target: "training-tracker", "race_cutin: both AddCutInInfo overloads hooked");
    true
}
