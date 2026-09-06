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

use super::chain::get_skills_chara_ptr;
use super::il2cpp::{read_list_items, read_obscured_int};

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
const MAX_TIPS: usize = 512;

/// Every hint the current career holds.
///
/// `None` means the list could not be read; `Some(vec![])` means it was read
/// and the career has no hints. The difference decides whether a plan built
/// from this is missing its discounts or genuinely has none, so it is not
/// flattened into an empty vector.
///
/// Must run on the Unity main thread.
pub fn read_skill_tips() -> Option<Vec<SkillTip>> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { read_inner() })) {
        Ok(tips) => tips,
        Err(_) => {
            hlog_error!("read_skill_tips PANICKED");
            None
        }
    }
}

unsafe fn read_inner() -> Option<Vec<SkillTip>> {
    let chara = get_skills_chara_ptr()?;
    // SAFETY: chara is a live WorkSingleModeCharaData.
    let items = unsafe { read_list_items(chara, &[TIPS_FIELD]) };
    if items.is_empty() {
        // Told apart from "no hints" by the field being there at all: a career
        // with none still has the list.
        // SAFETY: as above.
        if unsafe { super::il2cpp::read_ref_field(chara, &[TIPS_FIELD]) }.is_null() {
            warn_once();
            return None;
        }
        return Some(Vec::new());
    }
    if items.len() > MAX_TIPS {
        return None;
    }

    let mut tips = Vec::with_capacity(items.len());
    for item in items {
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
    Some(tips)
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
