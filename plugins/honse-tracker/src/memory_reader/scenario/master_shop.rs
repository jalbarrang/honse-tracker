//! Master-data enrichment for the Trackblazer shop: localized item names and
//! effect values, resolved on the Unity main thread.
//!
//! Access chains (all verified against `il2cpp_classes.txt`, build 2026-05-30):
//! ```text
//! Singleton<MasterDataManager>
//!   ._<masterString>k__BackingField -> Gallop.MasterString
//!       .GetText(Category category, Int32 index) -> String   (localized name)
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

use std::ffi::c_void;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::OnceLock;

use crate::compat::Sdk;

use super::super::il2cpp::{
    call_obj, call_obj_with_2i32, call_obj_with_i32, read_i32_field, read_il2cpp_string, resolve_obj_method,
};

/// Highest category int to probe when discovering the item-name category.
const CATEGORY_PROBE_MAX: i32 = 600;
/// Discovered `MasterString.Category` int for `SingleModeScenarioFreeItemName`.
/// `-2` = not yet attempted, `-1` = attempted but not found.
static NAME_CATEGORY: AtomicI32 = AtomicI32::new(-2);

/// Cached `MasterString` object pointer + its `GetText` MethodInfo.
struct StringTable {
    obj: *mut c_void,
    get_text: *const c_void,
}
// SAFETY: IL2CPP object/method pointers are stable for the process lifetime.
unsafe impl Send for StringTable {}
// SAFETY: IL2CPP object/method pointers are stable for the process lifetime.
unsafe impl Sync for StringTable {}

static MASTER_DATA_KLASS: OnceLock<usize> = OnceLock::new();

/// Resolve the `MasterDataManager` singleton object, or `None`.
fn master_data_manager() -> Option<*mut c_void> {
    let sdk = Sdk::get();
    let klass = *MASTER_DATA_KLASS.get_or_init(|| {
        sdk.get_assembly_image("umamusume.dll")
            .and_then(|img| sdk.get_class(img.cast(), "Gallop", "MasterDataManager"))
            .map_or(0usize, |k| k as usize)
    });
    if klass == 0 {
        return None;
    }
    sdk.get_singleton(klass as *mut c_void).map(|p| p.cast())
}

/// Resolve the cached `MasterString` table (object + `GetText`).
fn string_table() -> Option<&'static StringTable> {
    static TABLE: OnceLock<Option<StringTable>> = OnceLock::new();
    TABLE
        .get_or_init(|| {
            let mdm = master_data_manager()?;
            let sdk = Sdk::get();
            // SAFETY: IL2CPP object header — klass pointer at offset 0.
            let klass = unsafe { *(mdm as *const *mut c_void) };
            let field = sdk.get_field_from_name(klass.cast(), "<masterString>k__BackingField")?;
            let mut obj: *mut c_void = std::ptr::null_mut();
            // SAFETY: reading a managed-ref field from the singleton.
            unsafe {
                sdk.get_field_value(mdm.cast(), field, &mut obj as *mut _ as *mut c_void);
            }
            if obj.is_null() {
                return None;
            }
            // SAFETY: `obj` is a non-null MasterString object.
            let get_text = unsafe { resolve_obj_method(obj, "GetText", 2)? };
            Some(StringTable { obj, get_text })
        })
        .as_ref()
}

/// Call `MasterString.GetText(category, index)` and return a Rust string.
fn get_text(table: &StringTable, category: i32, index: i32) -> Option<String> {
    // SAFETY: `table.obj` is a valid MasterString; `get_text` is its GetText(2).
    let s = unsafe { call_obj_with_2i32(table.obj, table.get_text, category, index) };
    // SAFETY: GetText returns a managed String (or null).
    unsafe { read_il2cpp_string(s) }.filter(|t| !t.is_empty())
}

/// Discover (once) and return the `SingleModeScenarioFreeItemName` category int.
/// Picks the category that resolves the most `item_ids` to non-empty names.
fn name_category(table: &StringTable, item_ids: &[i32]) -> Option<i32> {
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
            .filter(|&&id| get_text(table, category, id).is_some())
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
    let table = string_table()?;
    let category = name_category(table, lineup_ids)?;
    get_text(table, category, item_id)
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
