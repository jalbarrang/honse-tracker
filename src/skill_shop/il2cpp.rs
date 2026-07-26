//! Low-level IL2CPP read/call primitives used by the shop reader.

use std::ffi::c_void;

use crate::compat::Sdk;

use super::crypto::decrypt_obscured_int_raw;

#[inline]
unsafe fn mptr(mi: *const c_void) -> usize {
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    unsafe { *(mi as *const usize) }
}

#[inline]
pub(super) unsafe fn call_obj(this: *mut c_void, mi: *const c_void) -> *mut c_void {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let f: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, mi)
}

#[inline]
pub(super) unsafe fn call_i32(this: *mut c_void, mi: *const c_void) -> i32 {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let f: extern "C" fn(*mut c_void, *const c_void) -> i32 = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, mi)
}

#[inline]
pub(super) unsafe fn call_obj_i32(this: *mut c_void, mi: *const c_void, arg: i32) -> *mut c_void {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let f: extern "C" fn(*mut c_void, i32, *const c_void) -> *mut c_void = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, arg, mi)
}

#[inline]
pub(super) unsafe fn call_i32_i32(this: *mut c_void, mi: *const c_void, arg: i32) -> i32 {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let f: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 = unsafe { std::mem::transmute(mptr(mi)) };
    f(this, arg, mi)
}

pub(super) unsafe fn read_string(s: *mut c_void) -> Option<String> {
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    unsafe {
        if s.is_null() {
            return None;
        }
        let len = *(s.byte_add(0x10) as *const i32);
        if len <= 0 || len > 4096 {
            return None;
        }
        String::from_utf16(std::slice::from_raw_parts(s.byte_add(0x14) as *const u16, len as usize)).ok()
    }
}

pub(super) unsafe fn read_field_i32(obj: *mut c_void, field: *mut c_void) -> i32 {
    let mut v: i32 = 0;
    // SAFETY: IL2CPP object and field pointers from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), &mut v as *mut _ as *mut c_void);
    }
    v
}

pub(super) unsafe fn decrypt_obscured_int(obj: *mut c_void, field: *mut c_void) -> i32 {
    let mut buf = [0u8; 16];
    // SAFETY: IL2CPP object and field pointers from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), buf.as_mut_ptr() as *mut c_void);
    }
    let raw: [u8; 8] = [buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]];
    decrypt_obscured_int_raw(&raw)
}

pub(super) unsafe fn read_i32_list(list: *mut c_void) -> Vec<i32> {
    if list.is_null() {
        return Vec::new();
    }
    // SAFETY: IL2CPP list object layout — klass pointer at object head.
    let list_klass = unsafe { *(list as *const *mut c_void) };
    let sdk = Sdk::get();
    let Some(m_cnt) = sdk.get_method(list_klass.cast(), "get_Count", 0) else {
        return Vec::new();
    };
    let Some(m_itm) = sdk.get_method(list_klass.cast(), "get_Item", 1) else {
        return Vec::new();
    };
    if m_cnt.is_null() || m_itm.is_null() {
        return Vec::new();
    }
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let count = unsafe { call_i32(list, m_cnt) };
    if count <= 0 || count > 32 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        // SAFETY: List<Int32>.get_Item returns the Int32 value directly, not a boxed object.
        out.push(unsafe { call_i32_i32(list, m_itm, i) });
    }
    out
}
