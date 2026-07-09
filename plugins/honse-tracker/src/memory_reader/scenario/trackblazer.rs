//! Trackblazer (Make a New Track) RaceCoin shop, read live from the work object.
//!
//! Path (all getters return plain `Int32`/managed refs — directly callable):
//! ```text
//! WorkSingleModeCharaData.get_WorkScenarioFree() -> WorkSingleModeScenarioFree
//!   .get_CoinNum()             -> Int32  (player RaceCoins)
//!   .get_ShopId()              -> Int32  (current lineup id)
//!   .get_SaleValue()           -> Int32  (sale % / discount)
//!   .get_WinPoints()           -> Int32
//!   .get_PickUpItemInfoArray() -> SingleModeFreePickUpItem[]  (lineup)
//! ```
//! Each `SingleModeFreePickUpItem` has plain `Int32` fields:
//! `shop_item_id, item_id, coin_num, original_coin_num, item_buy_num,
//! limit_buy_count, limit_turn`.
//!
//! Item display names are localized via MasterString (`SingleModeScenarioFreeItemName`)
//! and are deferred — we surface `item_id` for now. Reads run on the main thread only.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

use super::super::il2cpp::{call_i32, call_obj, read_i32_field, read_obj_array, resolve_obj_method};
use super::items::Worth;
use super::{items, master_shop};

/// One shop lineup entry.
#[derive(Debug, Clone, Default)]
pub struct TrackblazerShopItem {
    pub item_id: i32,
    /// Localized display name, or empty if unresolved (UI falls back to `#id`).
    pub name: String,
    /// Human-readable effect (curated catalog, or raw master value fallback).
    pub effect: String,
    /// Editorial buy-priority tier (curated), or `None` when unknown.
    pub worth: Option<Worth>,
    /// Current price (after any sale).
    pub coin_num: i32,
    /// Pre-sale price (equals `coin_num` when not discounted).
    pub original_coin_num: i32,
    /// Times already bought this run.
    pub bought: i32,
    /// Purchase cap (`0` = unlimited).
    pub limit: i32,
    /// Turns this item remains available in the shop (`limit_turn`). `0` = unknown.
    pub turns_left: i32,
}

impl TrackblazerShopItem {
    /// Whether this entry is currently discounted.
    pub fn discounted(&self) -> bool {
        self.original_coin_num > 0 && self.coin_num < self.original_coin_num
    }

    /// Whether the purchase cap has been reached.
    pub fn sold_out(&self) -> bool {
        self.limit > 0 && self.bought >= self.limit
    }
}

/// One item the player currently holds in the Trackblazer inventory.
#[derive(Debug, Clone, Default)]
pub struct TrackblazerOwnedItem {
    pub item_id: i32,
    /// Localized display name, or empty if unresolved (UI falls back to `#id`).
    pub name: String,
    /// Human-readable effect (curated catalog), empty if unlisted.
    pub effect: String,
    /// How many of this item the player holds.
    pub count: i32,
}

/// Trackblazer shop snapshot: player coins + current lineup + owned inventory.
#[derive(Debug, Clone, Default)]
pub struct TrackblazerShop {
    pub coins: i32,
    pub sale_value: i32,
    pub win_points: i32,
    pub items: Vec<TrackblazerShopItem>,
    /// Items the player currently holds (sorted by `item_id`).
    pub owned: Vec<TrackblazerOwnedItem>,
}

/// Read the Trackblazer shop from the chara-data work object, or `None` if the
/// scenario is not Trackblazer (`get_WorkScenarioFree()` returns null).
pub(super) unsafe fn read_shop(chara: *mut c_void) -> Option<TrackblazerShop> {
    if chara.is_null() {
        return None;
    }
    // SAFETY: each step calls/reads on a non-null IL2CPP object verified below.
    unsafe {
        let m_free = resolve_obj_method(chara, "get_WorkScenarioFree", 0)?;
        let work = call_obj(chara, m_free);
        if work.is_null() {
            return None; // not the Trackblazer scenario
        }

        let read = |name: &str| {
            resolve_obj_method(work, name, 0)
                .map(|m| call_i32(work, m))
                .unwrap_or(0)
        };
        let coins = read("get_CoinNum");
        let sale_value = read("get_SaleValue");
        let win_points = read("get_WinPoints");

        let owned_map = read_owned_items(work);
        let mut items = read_lineup(work);

        // Name lookup needs the full id set (lineup + owned) for category discovery.
        let mut all_ids: Vec<i32> = items.iter().map(|it| it.item_id).collect();
        all_ids.extend(owned_map.keys().copied());
        enrich_items(&mut items, &all_ids);
        let owned = build_owned(&owned_map, &all_ids);

        log_shop_on_change(coins, sale_value, &items, &owned);
        Some(TrackblazerShop {
            coins,
            sale_value,
            win_points,
            items,
            owned,
        })
    }
}

/// Diagnostic: log the raw shop read whenever coins or the lineup CHANGE, so the
/// values can be cross-checked against the in-game Trackblazer shop. Deduped to
/// avoid spamming the ~2s refresh.
#[allow(clippy::type_complexity)]
fn log_shop_on_change(coins: i32, sale_value: i32, items: &[TrackblazerShopItem], owned: &[TrackblazerOwnedItem]) {
    static LAST: Mutex<
        Option<(
            i32,
            i32,
            Vec<(i32, String, String, i32, i32, i32)>,
            Vec<(i32, String, i32)>,
        )>,
    > = Mutex::new(None);
    let sig: Vec<(i32, String, String, i32, i32, i32)> = items
        .iter()
        .map(|it| {
            (
                it.item_id,
                it.name.clone(),
                it.effect.clone(),
                it.bought,
                it.limit,
                it.turns_left,
            )
        })
        .collect();
    let owned_sig: Vec<(i32, String, i32)> = owned.iter().map(|o| (o.item_id, o.name.clone(), o.count)).collect();
    let cur = (coins, sale_value, sig, owned_sig);
    if let Ok(mut guard) = LAST.lock() {
        if guard.as_ref() == Some(&cur) {
            return;
        }
        *guard = Some(cur.clone());
    }
    hlog_info!(
        "Trackblazer shop: coins={} sale={} items(item_id,name,effect,bought,limit,turns_left)={:?} owned(item_id,name,count)={:?}",
        cur.0,
        cur.1,
        cur.2,
        cur.3
    );
}

/// Read `get_UserItemInfoArray()` into an `item_id -> owned count` map.
unsafe fn read_owned_items(work: *mut c_void) -> HashMap<i32, i32> {
    let mut owned = HashMap::new();
    // SAFETY: `work` is a non-null WorkSingleModeScenarioFree object.
    unsafe {
        let Some(m_arr) = resolve_obj_method(work, "get_UserItemInfoArray", 0) else {
            return owned;
        };
        let array = call_obj(work, m_arr);
        let Some((base, len)) = read_obj_array(array) else {
            return owned;
        };
        for i in 0..len {
            let elem = *base.add(i);
            if elem.is_null() {
                continue;
            }
            owned.insert(read_i32_field(elem, "item_id"), read_i32_field(elem, "num"));
        }
    }
    owned
}

/// Fill in name / effect / worth for each lineup item.
/// Effect + worth come from the curated catalog; unlisted items fall back to the
/// raw master effect value (`EffectValue1`). `all_ids` is the lineup+owned id set
/// used to discover the name category.
fn enrich_items(items: &mut [TrackblazerShopItem], all_ids: &[i32]) {
    for item in items.iter_mut() {
        item.name = master_shop::item_name(item.item_id, all_ids).unwrap_or_default();
        match items::lookup(item.item_id) {
            Some(entry) => {
                item.effect = entry.effect.to_string();
                item.worth = Some(entry.worth);
            }
            None => {
                item.effect = master_shop::item_value(item.item_id)
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                item.worth = None;
            }
        }
    }
}

/// Build the owned-inventory list (sorted by `item_id`) from the owned map.
/// Names use the same lookup as the lineup; effects come from the curated catalog.
fn build_owned(owned_map: &HashMap<i32, i32>, all_ids: &[i32]) -> Vec<TrackblazerOwnedItem> {
    let mut owned: Vec<TrackblazerOwnedItem> = owned_map
        .iter()
        .filter(|(_, &count)| count > 0)
        .map(|(&item_id, &count)| TrackblazerOwnedItem {
            item_id,
            name: master_shop::item_name(item_id, all_ids).unwrap_or_default(),
            effect: items::lookup(item_id).map(|e| e.effect.to_string()).unwrap_or_default(),
            count,
        })
        .collect();
    owned.sort_by_key(|o| o.item_id);
    owned
}

/// Read the `SingleModeFreePickUpItem[]` lineup into shop items.
unsafe fn read_lineup(work: *mut c_void) -> Vec<TrackblazerShopItem> {
    // SAFETY: `work` is a non-null WorkSingleModeScenarioFree object.
    unsafe {
        let Some(m_arr) = resolve_obj_method(work, "get_PickUpItemInfoArray", 0) else {
            return Vec::new();
        };
        let array = call_obj(work, m_arr);
        let Some((base, len)) = read_obj_array(array) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            let elem = *base.add(i);
            if elem.is_null() {
                continue;
            }
            out.push(TrackblazerShopItem {
                item_id: read_i32_field(elem, "item_id"),
                coin_num: read_i32_field(elem, "coin_num"),
                original_coin_num: read_i32_field(elem, "original_coin_num"),
                bought: read_i32_field(elem, "item_buy_num"),
                limit: read_i32_field(elem, "limit_buy_count"),
                turns_left: read_i32_field(elem, "limit_turn"),
                ..Default::default()
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::TrackblazerShopItem;

    fn item(coin: i32, orig: i32, bought: i32, limit: i32) -> TrackblazerShopItem {
        TrackblazerShopItem {
            item_id: 1,
            coin_num: coin,
            original_coin_num: orig,
            bought,
            limit,
            ..Default::default()
        }
    }

    #[test]
    fn discounted_only_when_below_original() {
        assert!(item(80, 100, 0, 0).discounted());
        assert!(!item(100, 100, 0, 0).discounted());
        assert!(!item(100, 0, 0, 0).discounted()); // no original recorded
    }

    #[test]
    fn sold_out_respects_unlimited_and_cap() {
        assert!(item(10, 10, 3, 3).sold_out());
        assert!(!item(10, 10, 1, 3).sold_out());
        assert!(!item(10, 10, 99, 0).sold_out()); // limit 0 = unlimited
    }
}
