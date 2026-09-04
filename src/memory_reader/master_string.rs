//! The game's localized text table, `Gallop.MasterString`.
//!
//! ```text
//! Singleton<MasterDataManager>
//!   ._<masterString>k__BackingField -> Gallop.MasterString
//!       .GetText(Category category, Int32 index) -> String
//! ```
//!
//! This is `master.mdb`'s `text_data` seen from inside the process: the same
//! `(category, index)` pair the table is keyed on, resolved against whatever
//! the client has loaded. Reading it live rather than shipping a copy means the
//! text follows the player's game — including a language the plugin has never
//! heard of — and cannot go stale on an update.
//!
//! # Categories are integers, and finding them is the hard part
//!
//! `MasterString.Category` carries no literal values in the metadata dump, so a
//! caller either knows the integer already (from querying `master.mdb`, which
//! stores the same number in its `category` column) or has to discover it — see
//! the probe in `scenario::master_shop`, which brute-forces the category that
//! resolves the most known ids.
//!
//! Caller contract: Unity main thread, like every other IL2CPP read here.

use std::ffi::c_void;
use std::sync::OnceLock;

use crate::compat::Sdk;

use super::il2cpp::{call_obj_with_2i32, read_il2cpp_string, resolve_obj_method};

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
pub(super) fn master_data_manager() -> Option<*mut c_void> {
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

/// One localized string, or `None` when the table or the row is missing.
///
/// An empty result counts as missing: the table answers a key it does not have
/// with an empty string, and no caller here wants to show one.
pub(super) fn text(category: i32, index: i32) -> Option<String> {
    let table = string_table()?;
    // SAFETY: `table.obj` is a valid MasterString; `get_text` is its GetText(2).
    let s = unsafe { call_obj_with_2i32(table.obj, table.get_text, category, index) };
    // SAFETY: GetText returns a managed String (or null).
    unsafe { read_il2cpp_string(s) }.filter(|t| !t.is_empty())
}
