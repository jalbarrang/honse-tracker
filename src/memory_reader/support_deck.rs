//! Equipped support-card identity per deck position.
//!
//! Reads `EquipSupportCard.<Position>` + `<SupportCardId>` as **ObscuredInt fields**
//! directly (decrypted), never calling `ConvertWorkSupportCardData` — that API
//! mutates live equip/evaluation state and corrupts the career profile when called
//! on a refresh cadence (see docs/reverse-engineering/support-card-event-chains.md).
//! Field reads are pure and side-effect free.

use std::ffi::c_void;

use crate::compat::Sdk;

use super::chain::get_chara_ptr;
use super::il2cpp::{call_obj, read_obscured_int_field, resolve_obj_method};

/// `(position, support_card_id)` for each equipped card. `position` matches the
/// `Evaluation.target_id` deck slot (1..6). Empty when unavailable.
pub fn read_equipped_support_ids() -> Vec<(i32, i32)> {
    // SAFETY: all reads are on resolved IL2CPP metadata / live objects.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { inner() })) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_equipped_support_ids PANICKED");
            Vec::new()
        }
    }
}

unsafe fn inner() -> Vec<(i32, i32)> {
    let Some(chara) = get_chara_ptr() else {
        return Vec::new();
    };
    // SAFETY: resolve + call a 0-arg getter on the chara klass.
    let Some(m_get_array) = (unsafe { resolve_obj_method(chara, "get_EquipSupportCardArray", 0) }) else {
        return Vec::new();
    };
    // SAFETY: get_EquipSupportCardArray returns a System.Array (or null).
    let array = unsafe { call_obj(chara, m_get_array) };
    if array.is_null() {
        return Vec::new();
    }
    // IL2CPP array: length at 0x18, pointer-sized elements from 0x20.
    // SAFETY: array is a valid IL2CPP System.Array.
    let len = unsafe { *(array.byte_add(0x18) as *const i32) };
    if len <= 0 || len > 8 {
        return Vec::new();
    }

    let sdk = Sdk::get();
    let mut f_position: *mut c_void = std::ptr::null_mut();
    let mut f_card_id: *mut c_void = std::ptr::null_mut();
    let mut out = Vec::with_capacity(len as usize);

    for i in 0..len {
        // SAFETY: element i (pointer-sized) within the array bounds.
        let equip = unsafe { *(array.byte_add(0x20 + (i as usize) * 8) as *const *mut c_void) };
        if equip.is_null() {
            continue;
        }
        if f_card_id.is_null() {
            // SAFETY: IL2CPP object header — klass pointer at offset 0.
            let klass = unsafe { *(equip as *const *mut c_void) };
            f_position = sdk
                .get_field_from_name(klass.cast(), "<Position>k__BackingField")
                .map(|f| f.cast())
                .unwrap_or(std::ptr::null_mut());
            for name in ["<SupportCardId>k__BackingField", "_supportCardId"] {
                if let Some(f) = sdk.get_field_from_name(klass.cast(), name) {
                    f_card_id = f.cast();
                    break;
                }
            }
            if f_card_id.is_null() {
                hlog_warn!("support_deck: SupportCardId field not found on EquipSupportCard");
                return Vec::new();
            }
        }
        // Fall back to array index + 1 if the position field is missing.
        let position = if f_position.is_null() {
            i + 1
        } else {
            // SAFETY: ObscuredInt field on a valid EquipSupportCard.
            unsafe { read_obscured_int_field(equip, f_position) }
        };
        // SAFETY: ObscuredInt field on a valid EquipSupportCard.
        let card_id = unsafe { read_obscured_int_field(equip, f_card_id) };
        out.push((position, card_id));
    }
    out
}
