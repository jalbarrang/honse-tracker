//! Support-card friendship/evaluation list reading.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compat::Sdk;

use super::chain::get_chara_ptr;
use super::il2cpp::{call_bool, call_i32, call_obj_with_i32, method_ptr, read_il2cpp_string, read_list_field};

/// A single support card's friendship/bond value.
#[derive(Debug, Clone)]
pub struct EvaluationInfo {
    pub target_id: i32,  // support card chara ID
    pub value: i32,      // friendship/bond value (0-100+)
    pub is_appear: bool, // whether the character is present in this career
    pub name: String,    // resolved character name
    /// Support-card story/outing progress step (`get_StoryStep`). `0` when none.
    pub story_step: i32,
    /// Guest character id (`get_GuestCharaId`); `0`/`-1` for non-guest. Diagnostic only.
    pub guest_chara_id: i32,
}

/// Read the evaluation (friendship) list from the chara object.
pub fn read_evaluations() -> Vec<EvaluationInfo> {
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { read_evaluations_inner() })) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_evaluations PANICKED");
            Vec::new()
        }
    }
}

unsafe fn read_evaluations_inner() -> Vec<EvaluationInfo> {
    let chara = match get_chara_ptr() {
        Some(c) => c,
        None => return Vec::new(),
    };

    // Try known field names for the evaluation list
    let field_names = [c"_evaluationList", c"_evaluationInfoList", c"_evaluations"];

    let mut list_data = None;
    for name in &field_names {
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        if let Some(data) = unsafe { read_list_field(chara, name) } {
            list_data = Some(data);
            break;
        }
    }

    let (list_ptr, count, m_get_item) = match list_data {
        Some(v) => v,
        None => return Vec::new(),
    };

    if count <= 0 || count > 50 {
        return Vec::new();
    }

    let sdk = Sdk::get();
    let mut evals = Vec::with_capacity(count as usize);
    let mut m_target_id: *const c_void = std::ptr::null();
    let mut m_value: *const c_void = std::ptr::null();
    let mut m_is_appear: *const c_void = std::ptr::null();
    let mut m_story_step: *const c_void = std::ptr::null();
    let mut m_guest_chara_id: *const c_void = std::ptr::null();
    let mut m_get_chara_name: *const c_void = std::ptr::null();
    let mut methods_resolved = false;

    for i in 0..count {
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let item = unsafe { call_obj_with_i32(list_ptr, m_get_item, i) };
        if item.is_null() {
            continue;
        }

        if !methods_resolved {
            methods_resolved = true;
            // SAFETY: IL2CPP object header — klass pointer at offset 0.
            let klass = unsafe { *(item as *const *mut c_void) };

            let (Some(tid), Some(val)) = (
                sdk.get_method(klass.cast(), "get_TargetId", 0),
                sdk.get_method(klass.cast(), "get_Value", 0),
            ) else {
                hlog_warn!("Evaluation methods not found (get_TargetId/get_Value)");
                return Vec::new();
            };
            m_target_id = tid.cast();
            m_value = val.cast();
            m_is_appear = sdk
                .get_method(klass.cast(), "get_IsAppear", 0)
                .map(|m| m.cast())
                .unwrap_or(std::ptr::null());
            m_story_step = sdk
                .get_method(klass.cast(), "get_StoryStep", 0)
                .map(|m| m.cast())
                .unwrap_or(std::ptr::null());
            m_guest_chara_id = sdk
                .get_method(klass.cast(), "get_GuestCharaId", 0)
                .map(|m| m.cast())
                .unwrap_or(std::ptr::null());

            if let Some(image) = sdk.get_assembly_image("umamusume.dll") {
                if let Some(mdu) = sdk.get_class(image, "Gallop", "MasterDataUtil") {
                    m_get_chara_name = sdk
                        .get_method(mdu, "GetCharaNameByCharaId", 1)
                        .map(|m| m.cast())
                        .unwrap_or(std::ptr::null());
                }
            }

            static LOGGED: AtomicBool = AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                hlog_info!(
                    "Evaluation: resolved get_TargetId + get_Value + get_IsAppear={} + get_StoryStep={} + get_GuestCharaId={} + GetCharaName={}",
                    !m_is_appear.is_null(),
                    !m_story_step.is_null(),
                    !m_guest_chara_id.is_null(),
                    !m_get_chara_name.is_null()
                );
            }
        }

        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let target_id = unsafe { call_i32(item, m_target_id) };
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let value = unsafe { call_i32(item, m_value) };

        let is_appear = if !m_is_appear.is_null() {
            // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
            unsafe { call_bool(item, m_is_appear) }
        } else {
            true // assume present if we can't check
        };

        let story_step = if !m_story_step.is_null() {
            // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
            unsafe { call_i32(item, m_story_step) }
        } else {
            0
        };
        let guest_chara_id = if !m_guest_chara_id.is_null() {
            // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
            unsafe { call_i32(item, m_guest_chara_id) }
        } else {
            0
        };

        // Resolve name via MasterDataUtil.GetCharaNameByCharaId (static)
        let name = if !m_get_chara_name.is_null() {
            // SAFETY: IL2CPP static method call
            let str_obj = unsafe {
                let fp: extern "C" fn(i32, *const c_void) -> *mut c_void =
                    std::mem::transmute(method_ptr(m_get_chara_name));
                fp(target_id, m_get_chara_name)
            };
            // SAFETY: IL2CPP FFI call; host vtable and resolved symbols are valid for process lifetime.
            unsafe { read_il2cpp_string(str_obj) }.unwrap_or_default()
        } else {
            String::new()
        };

        evals.push(EvaluationInfo {
            target_id,
            value,
            is_appear,
            name,
            story_step,
            guest_chara_id,
        });
    }

    evals
}
