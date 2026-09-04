//! Master-data enrichment for the Trackblazer shop: localized item names and
//! effect values, resolved on the Unity main thread.
//!
//! Access chains (all verified against `il2cpp_classes.txt`, build 2026-05-30).
//! Localized names come from `super::super::master_string`; the rest is here:
//! ```text
//! Singleton<MasterDataManager>
//!   .get_masterSingleModeFreeShopItem()   -> MasterSingleModeFreeShopItem
//!       .GetWithItemId(Int32 itemId)       -> SingleModeFreeShopItem  (EffectGroupId)
//!   .get_masterSingleModeFreeShopEffect() -> MasterSingleModeFreeShopEffect
//!       .GetWithEffectGroupId(Int32 grpId) -> SingleModeFreeShopEffect (EffectValue1, EffectType)
//! ```
//!
//! The `MasterString.Category` enum has no literal values in the metadata dump,
//! so the integer for `SingleModeScenarioFreeItemName` is **discovered at
//! runtime**: we probe category ints and pick the one that resolves the most
//! lineup item ids to non-empty names. The winner is cached and logged.

use std::sync::atomic::{AtomicI32, Ordering};

use super::super::il2cpp::{call_obj, call_obj_with_i32, read_i32_field, resolve_obj_method};
use super::super::master_string::{self, master_data_manager};

/// Highest category int to probe when discovering the item-name category.
const CATEGORY_PROBE_MAX: i32 = 600;
/// Discovered `MasterString.Category` int for `SingleModeScenarioFreeItemName`.
/// `-2` = not yet attempted, `-1` = attempted but not found.
static NAME_CATEGORY: AtomicI32 = AtomicI32::new(-2);

/// Discover (once) and return the `SingleModeScenarioFreeItemName` category int.
/// Picks the category that resolves the most `item_ids` to non-empty names.
fn name_category(item_ids: &[i32]) -> Option<i32> {
    let cached = NAME_CATEGORY.load(Ordering::Relaxed);
    if cached >= 0 {
        return Some(cached);
    }
    if cached == -1 || item_ids.is_empty() {
        return None;
    }

    let mut best: Option<(i32, usize)> = None; // (category, hit count)
    for category in 0..CATEGORY_PROBE_MAX {
        let hits = item_ids
            .iter()
            .filter(|&&id| master_string::text(category, id).is_some())
            .count();
        if hits == item_ids.len() {
            best = Some((category, hits));
            break; // full coverage — almost certainly the right category
        }
        if hits > 0 && best.is_none_or(|(_, b)| hits > b) {
            best = Some((category, hits));
        }
    }

    match best {
        Some((category, hits)) => {
            NAME_CATEGORY.store(category, Ordering::Relaxed);
            hlog_info!(
                "Trackblazer name category discovered: {} ({}/{} items resolved)",
                category,
                hits,
                item_ids.len()
            );
            Some(category)
        }
        None => {
            NAME_CATEGORY.store(-1, Ordering::Relaxed);
            hlog_warn!("Trackblazer name category not found (probed 0..{})", CATEGORY_PROBE_MAX);
            None
        }
    }
}

/// Localized display name for `item_id`, or `None` if unresolved.
pub(super) fn item_name(item_id: i32, lineup_ids: &[i32]) -> Option<String> {
    let category = name_category(lineup_ids)?;
    master_string::text(category, item_id)
}

/// Effect value (`EffectValue1`) advertised by the shop item, or `None`.
/// Chain: `get_masterSingleModeFreeShopItem().GetWithItemId(item_id).EffectGroupId`
/// → `get_masterSingleModeFreeShopEffect().GetWithEffectGroupId(grp).EffectValue1`.
pub(super) fn item_value(item_id: i32) -> Option<i32> {
    let mdm = master_data_manager()?;
    // SAFETY: each step calls/reads on a non-null IL2CPP object verified below.
    unsafe {
        let m_item_tbl = resolve_obj_method(mdm, "get_masterSingleModeFreeShopItem", 0)?;
        let item_tbl = call_obj(mdm, m_item_tbl);
        let m_get_item = resolve_obj_method(item_tbl, "GetWithItemId", 1)?;
        let shop_item = call_obj_with_i32(item_tbl, m_get_item, item_id);
        if shop_item.is_null() {
            return None;
        }
        let group_id = read_i32_field(shop_item, "EffectGroupId");
        if group_id == 0 {
            return None;
        }

        let m_eff_tbl = resolve_obj_method(mdm, "get_masterSingleModeFreeShopEffect", 0)?;
        let eff_tbl = call_obj(mdm, m_eff_tbl);
        let m_get_eff = resolve_obj_method(eff_tbl, "GetWithEffectGroupId", 1)?;
        let effect = call_obj_with_i32(eff_tbl, m_get_eff, group_id);
        if effect.is_null() {
            return None;
        }
        Some(read_i32_field(effect, "EffectValue1"))
    }
}
