//! Skill shop feature: memory reconstruction, pure logic, and crypto.
//!
//! Submodules are re-exported flatly so existing `skill_shop::*` call sites
//! keep working:
//! - [`access`]: IL2CPP resolution + memory reading (unsafe)
//! - [`il2cpp`]: low-level read/call primitives
//! - [`logic`]: pure, unit-tested shop logic (discounts, sorting, filtering)
//! - [`crypto`]: ObscuredInt decryption

mod access;
mod crypto;
mod il2cpp;
mod logic;
mod purchase;

pub(crate) use access::{read_skill_points, read_skill_shop};
pub use logic::{discount_pct, discounted_cost, prepare_entries_for_display, rarity_label};

/// Buy a skill by id, gated on affordability against the live shop snapshot.
///
/// Looks up the skill's discounted cost from the cached shop entries and the
/// current SP, refuses if unaffordable / unknown, otherwise schedules the commit
/// on the Unity main thread (server-validated). `level` is the target skill level
/// (use 1 for normal skills). Returns the SP cost on success.
///
/// Callers (panel Buy button, IPC) MUST present a confirm prompt first — this
/// performs the purchase immediately.
pub(crate) fn buy_skill(skill_id: i32, level: i32) -> Result<i32, String> {
    use crate::overlay_cache;

    let entry = overlay_cache::skill_shop()
        .into_iter()
        .find(|e| e.skill_id == skill_id)
        .ok_or_else(|| format!("skill {skill_id} not available in the current shop"))?;
    if entry.is_learned {
        return Err(format!("skill {skill_id} is already learned"));
    }
    if entry.base_cost <= 0 {
        return Err(format!("skill {skill_id} has no known cost"));
    }
    let cost = discounted_cost(entry.base_cost, entry.hint_level, false);

    let sp = read_skill_points().ok_or("could not read current skill points")?;
    if sp < cost {
        return Err(format!("not enough skill points (need {cost}, have {sp})"));
    }

    purchase::request_buy(skill_id, level);
    Ok(cost)
}

/// A skill available in the shop, resolved from tips + master data.
#[derive(Debug, Clone)]
pub struct SkillShopEntry {
    pub skill_id: i32,
    pub group_id: i32,
    pub rarity: i32,
    pub hint_level: i32,
    pub name: String,
    pub base_cost: i32,
    pub is_learned: bool,
    /// `true` when derived from `_skillTipsList`; `false` for full-price (no hint) rows.
    pub has_hint: bool,
    /// Tag IDs from `MasterSkillData.SkillData.GetTagIds()` (distance/style/etc.).
    pub tags: Vec<i32>,
    /// `FilterSwitch` field — shop UI filter bitmask when tags are empty.
    pub filter_switch: i32,
}
