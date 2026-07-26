//! One-shot diagnostic: resolve bond `target_id`s to chara ids/names via the
//! `MasterSingleModeEvaluation` master table (read-only — `MasterDataManager`
//! singleton → `Get(id)` → record fields). Used to discover whether `target_id`
//! maps to a `CharaId` (e.g. scenario NPC 102 → 9002 → "Yayoi Akikawa").

use std::ffi::{c_void, CStr};
use std::os::raw::c_char;
use std::sync::OnceLock;

use crate::compat::Sdk;

use super::il2cpp::{call_obj, call_obj_with_i32, method_ptr, read_il2cpp_string, resolve_obj_method};

/// Leading layout of IL2CPP `FieldInfo` (name + type pointer) — matches class_dump.
#[repr(C)]
struct FieldInfoCompat {
    name: *const c_char,
    type_: *const c_void,
}

static MASTER_DATA_KLASS: OnceLock<usize> = OnceLock::new();

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

/// Resolve `MasterDataUtil.GetCharaNameByCharaId` (static, 1 arg), if present.
fn chara_name_method() -> *const c_void {
    let sdk = Sdk::get();
    sdk.get_assembly_image("umamusume.dll")
        .and_then(|img| sdk.get_class(img.cast(), "Gallop", "MasterDataUtil"))
        .and_then(|mdu| sdk.get_method(mdu, "GetCharaNameByCharaId", 1))
        .map_or(std::ptr::null(), |m| m.cast())
}

/// Resolve a chara id to its display name via the static util (empty on failure).
unsafe fn chara_name(m_name: *const c_void, chara_id: i32) -> String {
    if m_name.is_null() || chara_id == 0 {
        return String::new();
    }
    // SAFETY: IL2CPP static method call; resolved symbol valid for process lifetime.
    let str_obj = unsafe {
        let fp: extern "C" fn(i32, *const c_void) -> *mut c_void = std::mem::transmute(method_ptr(m_name));
        fp(chara_id, m_name)
    };
    // SAFETY: GetCharaNameByCharaId returns a managed String (or null).
    unsafe { read_il2cpp_string(str_obj) }.unwrap_or_default()
}

/// Probe `MasterSingleModeEvaluation.Get(target_id)` for each id and log the
/// record's int fields + resolved chara name. One-shot per career (gated by
/// caller). Pure read-only master-data access.
pub fn probe(target_ids: &[i32]) {
    // SAFETY: all reads are on resolved IL2CPP metadata / live master objects.
    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { inner(target_ids) })).is_err() {
        hlog_error!("eval_master::probe PANICKED");
    }
}

unsafe fn inner(target_ids: &[i32]) {
    let Some(mdm) = master_data_manager() else {
        hlog_warn!(target: "training-tracker", "eval_master: MasterDataManager singleton unavailable");
        return;
    };
    // SAFETY: 0-arg getter on the MasterDataManager singleton.
    let Some(m_tbl) = (unsafe { resolve_obj_method(mdm, "get_masterSingleModeEvaluation", 0) }) else {
        hlog_warn!(target: "training-tracker", "eval_master: get_masterSingleModeEvaluation not found");
        return;
    };
    // SAFETY: returns the master table object (or null).
    let table = unsafe { call_obj(mdm, m_tbl) };
    if table.is_null() {
        hlog_warn!(target: "training-tracker", "eval_master: masterSingleModeEvaluation table is null");
        return;
    }
    // SAFETY: Get(int id) on the table.
    let Some(m_get) = (unsafe { resolve_obj_method(table, "Get", 1) }) else {
        hlog_warn!(target: "training-tracker", "eval_master: Get(1) not found");
        return;
    };

    let m_name = chara_name_method();
    let get_fields: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void =
        match Sdk::get().resolve_symbol("il2cpp_class_get_fields") {
            // SAFETY: il2cpp_class_get_fields C export with this signature.
            Some(p) => unsafe { std::mem::transmute(p) },
            None => {
                hlog_warn!(target: "training-tracker", "eval_master: il2cpp_class_get_fields unavailable");
                return;
            }
        };

    let sdk = Sdk::get();
    hlog_info!(target: "training-tracker", "eval_master probe ({} target_ids):", target_ids.len());
    for &tid in target_ids {
        // SAFETY: Get(tid) returns a SingleModeEvaluation record (or null).
        let rec = unsafe { call_obj_with_i32(table, m_get, tid) };
        if rec.is_null() {
            hlog_info!(target: "training-tracker", "  target_id={} -> (no record)", tid);
            continue;
        }
        // SAFETY: IL2CPP object header — klass pointer at offset 0.
        let klass = unsafe { *(rec as *const *mut c_void) };

        // Enumerate the record's fields, reading each as i32 (master columns are ints).
        let mut parts: Vec<String> = Vec::new();
        let mut chara_id = 0i32;
        let mut iter: *mut c_void = std::ptr::null_mut();
        loop {
            // SAFETY: IL2CPP field iterator (void* iter = NULL convention).
            let field = unsafe { get_fields(klass, &mut iter) };
            if field.is_null() {
                break;
            }
            // SAFETY: FieldInfoCompat matches the leading IL2CPP FieldInfo layout.
            let name = unsafe {
                let fi = &*(field as *const FieldInfoCompat);
                if fi.name.is_null() {
                    String::from("?")
                } else {
                    CStr::from_ptr(fi.name).to_string_lossy().into_owned()
                }
            };
            let mut value = 0i32;
            // SAFETY: reading a field value into an i32 buffer.
            unsafe {
                sdk.get_field_value(rec.cast(), field.cast(), &mut value as *mut _ as *mut c_void);
            }
            if name.to_ascii_lowercase().contains("chara") && name.to_ascii_lowercase().contains("id") {
                chara_id = value;
            }
            parts.push(format!("{}={}", name, value));
        }
        // SAFETY: resolved static name method.
        let name = unsafe { chara_name(m_name, chara_id) };
        hlog_info!(
            target: "training-tracker",
            "  target_id={} fields=[{}] chara_id={} name={:?}",
            tid, parts.join(", "), chara_id, name
        );
    }
}
