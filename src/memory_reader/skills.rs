//! Acquired-skill list reading from `WorkSingleModeCharaData._acquiredSkillList`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::compat::Sdk;

use super::chain::get_chara_ptr;
use super::il2cpp::{call_i32, call_obj, call_obj_with_i32, read_il2cpp_string, read_list_field};

/// A single acquired skill read from game memory.
#[derive(Debug, Clone)]
pub struct AcquiredSkillInfo {
    pub master_id: i32,
    pub level: i32,
    pub name: String,
}

/// Read the acquired skill list from the chara object.
/// Returns (list_ptr, count) for diagnostics, or None.
pub fn read_acquired_skill_list() -> Option<(*mut c_void, i32)> {
    let chara = get_chara_ptr()?;
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    unsafe {
        let (list_ptr, count, _) = read_list_field(chara, c"_acquiredSkillList")?;
        Some((list_ptr, count))
    }
}

/// Read all acquired skills with names.
pub fn read_acquired_skills() -> Vec<AcquiredSkillInfo> {
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { read_acquired_skills_inner() })) {
        Ok(v) => v,
        Err(_) => {
            hlog_error!("read_acquired_skills PANICKED");
            Vec::new()
        }
    }
}

unsafe fn read_acquired_skills_inner() -> Vec<AcquiredSkillInfo> {
    let chara = match get_chara_ptr() {
        Some(c) => c,
        None => return Vec::new(),
    };

    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let (list_ptr, count, m_get_item) = match unsafe { read_list_field(chara, c"_acquiredSkillList") } {
        Some(v) => v,
        None => return Vec::new(),
    };

    if count <= 0 || count > 200 {
        return Vec::new();
    }

    let sdk = Sdk::get();
    let mut skills = Vec::with_capacity(count as usize);
    let mut m_master_id: *const c_void = std::ptr::null();
    let mut m_level: *const c_void = std::ptr::null();
    let mut m_master_data: *const c_void = std::ptr::null();
    let mut m_name: *const c_void = std::ptr::null();
    let mut methods_resolved = false;

    for i in 0..count {
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let item = unsafe { call_obj_with_i32(list_ptr, m_get_item, i) };
        if item.is_null() {
            continue;
        }

        // Resolve methods on first element (inherited from SkillDataBase)
        if !methods_resolved {
            methods_resolved = true;
            // SAFETY: IL2CPP object header — klass pointer at offset 0.
            let klass = unsafe { *(item as *const *mut c_void) };

            let (Some(mid), Some(lvl)) = (
                sdk.get_method(klass.cast(), "get_MasterId", 0),
                sdk.get_method(klass.cast(), "get_Level", 0),
            ) else {
                hlog_warn!("SkillDataBase methods not found (get_MasterId/get_Level)");
                return Vec::new();
            };
            m_master_id = mid.cast();
            m_level = lvl.cast();
            m_master_data = sdk
                .get_method(klass.cast(), "get_MasterData", 0)
                .map(|m| m.cast())
                .unwrap_or(std::ptr::null());

            static LOGGED: AtomicBool = AtomicBool::new(false);
            if !LOGGED.swap(true, Ordering::Relaxed) {
                hlog_info!(
                    "AcquiredSkill: resolved get_MasterId={} get_Level={} get_MasterData={}",
                    !m_master_id.is_null(),
                    !m_level.is_null(),
                    !m_master_data.is_null()
                );
            }
        }

        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let master_id = unsafe { call_i32(item, m_master_id) };
        // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
        let level = unsafe { call_i32(item, m_level) };

        // Try to get the name via get_MasterData() -> get_Name()
        let name = if !m_master_data.is_null() {
            // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
            let master_obj = unsafe { call_obj(item, m_master_data) };
            if !master_obj.is_null() {
                if m_name.is_null() {
                    // SAFETY: IL2CPP object header — klass pointer at offset 0.
                    let master_klass = unsafe { *(master_obj as *const *mut c_void) };
                    m_name = sdk
                        .get_method(master_klass.cast(), "get_Name", 0)
                        .map(|m| m.cast())
                        .unwrap_or(std::ptr::null());
                }
                if !m_name.is_null() {
                    // SAFETY: IL2CPP FFI call; host vtable and resolved symbols are valid for process lifetime.
                    let str_obj = unsafe { call_obj(master_obj, m_name) };
                    // SAFETY: IL2CPP FFI call; host vtable and resolved symbols are valid for process lifetime.
                    unsafe { read_il2cpp_string(str_obj) }.unwrap_or_default()
                } else {
                    String::new()
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        skills.push(AcquiredSkillInfo { master_id, level, name });
    }

    skills
}
