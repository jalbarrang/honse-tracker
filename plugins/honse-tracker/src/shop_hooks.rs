//! Captures skill IDs rendered by the in-game skill shop list (`PartsSingleModeSkillListItem`).
//!
//! When the player opens the career skill shop, `UpdateItem` runs per visible row. We record
//! each `Info.get_Id()` so [`crate::skill_shop`] can merge full-price (no-hint) entries.

use std::collections::HashSet;
use std::ffi::c_void;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex, OnceLock};

use crate::compat::Sdk;

static RESOLVED: OnceLock<ShopHookResolved> = OnceLock::new();
static VISIBLE_SKILL_IDS: LazyLock<Mutex<HashSet<i32>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

static mut ORIG_UPDATE_ITEM: *mut c_void = std::ptr::null_mut();

/// Address of the hook function we installed (for unhooking on shutdown). 0 = none.
static INSTALLED_HOOK_FN: AtomicUsize = AtomicUsize::new(0);

struct ShopHookResolved {
    get_id: *const c_void,
}

// SAFETY: IL2CPP MethodInfo pointers are stable for process lifetime.
unsafe impl Send for ShopHookResolved {}
// SAFETY: IL2CPP MethodInfo pointers are stable for process lifetime.
unsafe impl Sync for ShopHookResolved {}

/// Skill IDs seen in the shop UI since the last [`clear_visible_skills`].
pub fn visible_skill_ids() -> Vec<i32> {
    VISIBLE_SKILL_IDS
        .lock()
        .ok()
        .map(|s| s.iter().copied().collect())
        .unwrap_or_default()
}

/// Clear captured IDs (e.g. when leaving the shop screen — optional; growth is bounded).
#[allow(dead_code)]
pub fn clear_visible_skills() {
    if let Ok(mut s) = VISIBLE_SKILL_IDS.lock() {
        s.clear();
    }
}

fn record_skill_id(skill_id: i32) {
    if skill_id <= 0 {
        return;
    }
    if let Ok(mut s) = VISIBLE_SKILL_IDS.lock() {
        s.insert(skill_id);
    }
}

#[inline]
unsafe fn mptr(mi: *const c_void) -> usize {
    // SAFETY: Reading MethodInfo method pointer.
    unsafe { *(mi as *const usize) }
}

#[inline]
unsafe fn call_i32(this: *mut c_void, mi: *const c_void) -> i32 {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let f: extern "C" fn(*mut c_void, *const c_void) -> i32 = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, mi)
}

unsafe fn skill_id_from_info(info: *mut c_void) -> Option<i32> {
    let r = RESOLVED.get()?;
    if info.is_null() {
        return None;
    }
    // SAFETY: Resolved get_Id on PartsSingleModeSkillListItem.Info.
    let id = unsafe { call_i32(info, r.get_id) };
    (id > 0).then_some(id)
}

// ---------------------------------------------------------------------------
// Hook variants for different UpdateItem signatures across game versions.
// We try 4-arg (JP), 3-arg (current Global), then 2-arg (old Global) in order.
// ---------------------------------------------------------------------------

extern "C" fn hook_update_item_4(
    this: *mut c_void,
    skill_info: *mut c_void,
    _is_plate: bool,
    _adjuster: *mut c_void,
    _hash: i32,
) {
    // SAFETY: skill_info from game; get_Id is a simple getter.
    if let Some(id) = unsafe { skill_id_from_info(skill_info) } {
        record_skill_id(id);
    }
    // SAFETY: Trampoline from interceptor.
    unsafe {
        if !ORIG_UPDATE_ITEM.is_null() {
            let orig: extern "C" fn(*mut c_void, *mut c_void, bool, *mut c_void, i32) =
                std::mem::transmute(ORIG_UPDATE_ITEM);
            orig(this, skill_info, _is_plate, _adjuster, _hash);
        }
    }
}

extern "C" fn hook_update_item_3(this: *mut c_void, skill_info: *mut c_void, _is_plate: bool, _adjuster: *mut c_void) {
    // SAFETY: skill_info from game; get_Id is a simple getter.
    if let Some(id) = unsafe { skill_id_from_info(skill_info) } {
        record_skill_id(id);
    }
    // SAFETY: Trampoline from interceptor.
    unsafe {
        if !ORIG_UPDATE_ITEM.is_null() {
            let orig: extern "C" fn(*mut c_void, *mut c_void, bool, *mut c_void) =
                std::mem::transmute(ORIG_UPDATE_ITEM);
            orig(this, skill_info, _is_plate, _adjuster);
        }
    }
}

extern "C" fn hook_update_item_2(this: *mut c_void, skill_info: *mut c_void, _is_plate: bool) {
    // SAFETY: skill_info from game; get_Id is a simple getter.
    if let Some(id) = unsafe { skill_id_from_info(skill_info) } {
        record_skill_id(id);
    }
    // SAFETY: Trampoline from interceptor.
    unsafe {
        if !ORIG_UPDATE_ITEM.is_null() {
            let orig: extern "C" fn(*mut c_void, *mut c_void, bool) = std::mem::transmute(ORIG_UPDATE_ITEM);
            orig(this, skill_info, _is_plate);
        }
    }
}

fn try_resolve() -> Result<ShopHookResolved, &'static str> {
    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        return Err("umamusume.dll not found");
    };
    let Some(list_item) = sdk.get_class(img, "Gallop", "PartsSingleModeSkillListItem") else {
        return Err("PartsSingleModeSkillListItem not found");
    };
    let Some(info) = sdk.find_nested_class(list_item, "Info") else {
        return Err("PartsSingleModeSkillListItem.Info not found");
    };
    let Some(get_id) = sdk.get_method(info, "get_Id", 0) else {
        return Err("Info.get_Id not found");
    };
    Ok(ShopHookResolved { get_id: get_id.cast() })
}

/// Candidate hook signatures ordered by preference (newest game version first).
const CANDIDATES: &[(i32, *const c_void)] = &[
    (4, hook_update_item_4 as *const c_void),
    (3, hook_update_item_3 as *const c_void),
    (2, hook_update_item_2 as *const c_void),
];

/// Install shop list hooks. Safe to call multiple times.
pub fn try_install_shop_hooks() -> bool {
    // SAFETY: Static trampoline written once during hook install.
    if RESOLVED.get().is_some() && unsafe { !ORIG_UPDATE_ITEM.is_null() } {
        return true;
    }

    let Ok(resolved) = try_resolve() else {
        hlog_warn!("Shop hooks: IL2CPP resolution failed");
        return false;
    };
    let _ = RESOLVED.set(resolved);

    let sdk = Sdk::get();
    let Some(img) = sdk.get_assembly_image("umamusume.dll") else {
        return false;
    };
    let Some(list_item) = sdk.get_class(img, "Gallop", "PartsSingleModeSkillListItem") else {
        return false;
    };

    for &(arg_count, hook_fn) in CANDIDATES {
        if let Some(addr) = sdk.get_method_addr(list_item, "UpdateItem", arg_count) {
            if let Some(tramp) = sdk.hook(addr, hook_fn as *mut c_void) {
                // SAFETY: Written once from init before hooks fire.
                unsafe {
                    ORIG_UPDATE_ITEM = tramp;
                }
                INSTALLED_HOOK_FN.store(hook_fn as usize, Ordering::Release);
                hlog_info!("Shop hooks: UpdateItem({}) installed", arg_count);
                return true;
            }
        }
    }

    hlog_warn!("Shop hooks: no UpdateItem variant found (tried 4, 3, 2 args)");
    false
}

/// Remove the shop hook so the plugin's DLL can be safely unloaded. Idempotent.
pub fn uninstall_shop_hooks() {
    let hook_fn = INSTALLED_HOOK_FN.swap(0, Ordering::AcqRel);
    if hook_fn == 0 {
        return;
    }
    Sdk::get().unhook(hook_fn as *mut c_void);
    // SAFETY: hooks no longer fire once unhooked; reset the trampoline slot.
    unsafe {
        ORIG_UPDATE_ITEM = std::ptr::null_mut();
    }
    hlog_info!("Shop hooks: uninstalled");
}
