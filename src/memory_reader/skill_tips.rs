//! Skill hints held by the current career — the discounts on the shop screen.
//!
//! Two facts that the code cannot show you, both from `il2cpp_classes.txt`
//! (Global build, 2026-08-30):
//!
//! - A hint names a skill *group* and rarity, never a skill id.
//!   `WorkSingleModeCharaData._skillTipsList` holds
//!   `SkillTips { GroupId, Rarity, Level }`; mapping that onto ids is
//!   [`crate::gametora_data::hinted_skills`].
//! - Those three are `ObscuredInt`. `get_GroupId()` hands back the struct,
//!   whose first word is the crypto key, so only [`read_obscured_int`] gets
//!   the number.
//!
//! Reads nothing but that list off the chara object, on the Unity main thread.

use std::sync::atomic::{AtomicBool, Ordering};

use super::chain::get_chara_ptr;
use super::il2cpp::{call_obj_with_i32, read_list_field, read_obscured_int};

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

/// The hint list on the career chara, as a logical name.
const TIPS_FIELD: &str = "skillTipsList";

/// Nothing bigger than this is a hint list; a bigger count means the field is
/// not what we think it is.
const MAX_TIPS: i32 = 512;

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
    // SAFETY: chara is a live active-career object; a missing field yields None.
    let Some((list, count, get_item)) = (unsafe { read_list_field(chara, TIPS_FIELD) }) else {
        warn_once();
        return Vec::new();
    };
    if count <= 0 || count > MAX_TIPS {
        return Vec::new();
    }

    let mut tips = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: i < count on a live List<T>; `get_Item` is a pure getter.
        let item = unsafe { call_obj_with_i32(list, get_item, i) };
        if item.is_null() {
            continue;
        }
        // SAFETY: three named ObscuredInt fields on a live SkillTips.
        let tip = unsafe {
            SkillTip {
                group_id: read_obscured_int(item, "groupId"),
                rarity: read_obscured_int(item, "rarity"),
                level: read_obscured_int(item, "level"),
            }
        };
        // A hint with no group is not a hint — most likely a field that did not
        // resolve, which must not reach the planner as skill id 0.
        if tip.group_id > 0 {
            tips.push(tip);
        }
    }
    log_once(&tips);
    tips
}

fn log_once(tips: &[SkillTip]) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        hlog_info!(target: "training-tracker", "Skill tips: read {} hint(s)", tips.len());
    }
}

/// Warn once, naming what to look for. A career can genuinely hold no hints, so
/// this is information rather than an error.
fn warn_once() {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if !LOGGED.swap(true, Ordering::Relaxed) {
        hlog_warn!(
            target: "training-tracker",
            "Skill tips: WorkSingleModeCharaData has no '{TIPS_FIELD}' list. \
             Dump IL2CPP classes and grep il2cpp_classes.txt for SkillTips."
        );
    }
}
