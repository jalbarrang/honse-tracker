//! Skill hints held by the current career — the discounts on the shop screen.
//!
//! # A hint is a group, not a skill
//!
//! `Gallop.SkillTips` is `{ group_id, rarity, level }`: hints are stored per
//! skill *group* and rarity, never per skill id. Turning that into the skill
//! ids a planner wants is [`crate::gametora_data::candidates_for_tips`]'s job;
//! this module only reads what the game holds.
//!
//! The shape is confirmed twice over: umadump reads the same three fields off
//! `SingleModeChara` (`game_structs.py`), and a real capture of ours has them
//! (`docs/idle-career-format.example.json`).
//!
//! # Why the field is searched for rather than named
//!
//! The list hangs off `WorkSingleModeCharaData`, next to `_acquiredSkillList`,
//! but its exact name is not something we can pin from outside the process, and
//! the game renames private fields between builds. So both the field and the
//! accessors are resolved from a short list of spellings, once, and what
//! actually took is logged — a rename then costs a line here rather than a
//! silent empty list. When nothing resolves the log says which class to grep
//! for in `il2cpp_classes.txt` (see [`crate::class_dump`]).
//!
//! Read tier: career-lifetime work data on the chara object, the same tier the
//! shop-screen light refresh already reads. No per-screen UI objects.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use super::chain::get_chara_ptr;
use super::il2cpp::{call_i32, call_obj_with_i32, read_i32_field_any, read_list_field, read_obj_array, read_ref_field};

/// One hint the career is holding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SkillTip {
    /// `skill_id / 10` for every skill in the group.
    pub group_id: i32,
    /// `1` white, `2` gold. Narrows the group to one tier.
    pub rarity: i32,
    /// Hint level `1`–`5`; higher is a bigger discount.
    pub level: i32,
}

/// Spellings of the tips list on `WorkSingleModeCharaData`, most likely first.
/// `_acquiredSkillList` is the confirmed sibling, so `_skillTipsList` leads.
const FIELD_NAMES: &[&str] = &[
    "_skillTipsList",
    "_skillTipsArray",
    "_skillTips",
    "<SkillTipsList>k__BackingField",
    "<SkillTips>k__BackingField",
    "skillTipsArray",
];

/// Accessor spellings per field of a `SkillTips`, getters resolved separately.
const GROUP_ID_FIELDS: &[&str] = &["<GroupId>k__BackingField", "_groupId", "groupId"];
const RARITY_FIELDS: &[&str] = &["<Rarity>k__BackingField", "_rarity", "rarity"];
const LEVEL_FIELDS: &[&str] = &["<Level>k__BackingField", "_level", "level"];

/// Nothing bigger than this is a hint list; a bigger count means the field is
/// not what we think it is.
const MAX_TIPS: usize = 512;

/// Read every hint the current career holds. Empty outside a career, and empty
/// (with a warning, once) when the list cannot be found.
///
/// Must run on the Unity main thread.
pub fn read_skill_tips() -> Vec<SkillTip> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { read_inner() })) {
        Ok(tips) => tips,
        Err(_) => {
            hlog_error!("read_skill_tips PANICKED");
            Vec::new()
        }
    }
}

unsafe fn read_inner() -> Vec<SkillTip> {
    let Some(chara) = get_chara_ptr() else {
        return Vec::new();
    };

    // SAFETY: chara is a live active-career object; both readers resolve their
    // own metadata and return nothing when the field is absent.
    let items = unsafe { tip_objects(chara) };
    if items.is_empty() {
        warn_once();
        return Vec::new();
    }

    let mut tips = Vec::with_capacity(items.len());
    for item in items {
        // SAFETY: element of a list read from a live career object.
        if let Some(tip) = unsafe { read_tip(item) } {
            tips.push(tip);
        }
    }
    log_once(&tips);
    tips
}

/// The list elements, whether the game holds them as a `List<SkillTips>` or a
/// `SkillTips[]`. Both shapes have been seen for career-side collections, and
/// which one this is does not change what it means.
unsafe fn tip_objects(chara: *mut c_void) -> Vec<*mut c_void> {
    for name in FIELD_NAMES {
        let field = std::ffi::CString::new(*name).unwrap_or_default();
        // SAFETY: chara is a live IL2CPP object; a missing field yields None.
        if let Some((list, count, get_item)) = unsafe { read_list_field(chara, &field) } {
            if count <= 0 || count as usize > MAX_TIPS {
                continue;
            }
            let items = (0..count)
                // SAFETY: get_Item resolved from this list's own klass.
                .map(|i| unsafe { call_obj_with_i32(list, get_item, i) })
                .filter(|item| !item.is_null())
                .collect::<Vec<_>>();
            if !items.is_empty() {
                resolved_as(name, "List");
                return items;
            }
        }

        // SAFETY: reads a reference field by name; null when it is not there.
        let array = unsafe { read_ref_field(chara, &[name]) };
        // SAFETY: read_obj_array validates the header before dereferencing.
        if let Some((base, len)) = unsafe { read_obj_array(array) } {
            if len == 0 || len > MAX_TIPS {
                continue;
            }
            // SAFETY: base points at `len` object slots inside the array body.
            let items = (0..len)
                .map(|i| unsafe { *base.add(i) })
                .filter(|item| !item.is_null())
                .collect::<Vec<_>>();
            if !items.is_empty() {
                resolved_as(name, "array");
                return items;
            }
        }
    }
    Vec::new()
}

/// One `SkillTips`, by getter where there is one and by field otherwise.
/// `None` when the object yields no group id, which means it is not a tip.
unsafe fn read_tip(item: *mut c_void) -> Option<SkillTip> {
    // SAFETY: item is a live element; each accessor is resolved from its klass.
    let group_id = unsafe { read_int(item, "get_GroupId", GROUP_ID_FIELDS) };
    if group_id <= 0 {
        return None;
    }
    // SAFETY: as above.
    let rarity = unsafe { read_int(item, "get_Rarity", RARITY_FIELDS) };
    // SAFETY: as above.
    let level = unsafe { read_int(item, "get_Level", LEVEL_FIELDS) };
    Some(SkillTip {
        group_id,
        rarity,
        level,
    })
}

/// A property's value: the getter if the class has one, else the backing field.
unsafe fn read_int(item: *mut c_void, getter: &str, fields: &[&str]) -> i32 {
    // SAFETY: item is a live IL2CPP object; a missing method yields None.
    if let Some(method) = unsafe { super::il2cpp::resolve_obj_method(item, getter, 0) } {
        // SAFETY: 0-arg i32 getter resolved from this object's klass.
        return unsafe { call_i32(item, method) };
    }
    // SAFETY: reads by field name; 0 when none of the names are there.
    unsafe { read_i32_field_any(item, fields) }
}

/// Say once which spelling took, so a rename is visible in the log rather than
/// only in an empty planner export.
fn resolved_as(field: &str, shape: &str) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        hlog_info!(target: "training-tracker", "Skill tips: resolved WorkSingleModeCharaData.{field} ({shape})");
    }
}

fn log_once(tips: &[SkillTip]) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        hlog_info!(target: "training-tracker", "Skill tips: read {} hint(s)", tips.len());
    }
}

/// Warn once, naming what to look for. A career with no hints at all is
/// possible early on, so this is information rather than an error.
fn warn_once() {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        hlog_warn!(
            target: "training-tracker",
            "Skill tips: no hint list on WorkSingleModeCharaData (tried {}). \
             If the career has hints, dump IL2CPP classes and grep il2cpp_classes.txt for SkillTips.",
            FIELD_NAMES.join(", ")
        );
    }
}
