//! Low-level IL2CPP call/read primitives shared by the entity readers.

use std::ffi::c_void;

use crate::compat::Sdk;

/// IL2CPP MethodInfo starts with the method_pointer at offset 0.
/// We read it to get the callable function pointer.
#[inline]
pub(super) unsafe fn method_ptr(method_info: *const c_void) -> usize {
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    unsafe { *(method_info as *const usize) }
}

/// Call an instance method that returns `*mut c_void` (an IL2CPP object).
#[inline]
pub(super) unsafe fn call_obj(this: *mut c_void, mi: *const c_void) -> *mut c_void {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, *const c_void) -> *mut c_void = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, mi)
}

/// Call an instance method that returns `i32`.
#[inline]
pub(super) unsafe fn call_i32(this: *mut c_void, mi: *const c_void) -> i32 {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, *const c_void) -> i32 = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, mi)
}

/// Call an instance method that returns `bool` (IL2CPP uses u8).
#[inline]
pub(super) unsafe fn call_bool(this: *mut c_void, mi: *const c_void) -> bool {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, *const c_void) -> u8 = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, mi) != 0
}

/// Call an instance method that takes one `i32` arg and returns `i32`.
/// IL2CPP calling convention: `fn(this, arg1, method_info) -> i32`.
#[inline]
pub(super) unsafe fn call_i32_with_i32(this: *mut c_void, mi: *const c_void, arg: i32) -> i32 {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> i32 = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, arg, mi)
}

/// Call an instance method that takes one `i32` arg and returns `*mut c_void`.
#[inline]
pub(super) unsafe fn call_obj_with_i32(this: *mut c_void, mi: *const c_void, arg: i32) -> *mut c_void {
    let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> *mut c_void =
        // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
        unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, arg, mi)
}

/// Call an instance method that takes two `i32` args and returns `*mut c_void`.
/// IL2CPP calling convention: `fn(this, arg1, arg2, method_info) -> *mut c_void`.
/// Used for `MasterString.GetText(Category, int)` (the enum is Int32-backed).
#[inline]
pub(super) unsafe fn call_obj_with_2i32(this: *mut c_void, mi: *const c_void, a: i32, b: i32) -> *mut c_void {
    let fp: extern "C" fn(*mut c_void, i32, i32, *const c_void) -> *mut c_void =
        // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
        unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, a, b, mi)
}

/// Read an IL2CPP `System.String` object and convert to a Rust `String`.
/// IL2CppString layout (64-bit):
///   offset 0x00: Il2CppObject header (klass + monitor = 16 bytes)
///   offset 0x10: int32 length (in UTF-16 code units)
///   offset 0x14: char16_t[] chars (UTF-16 data)
pub(super) unsafe fn read_il2cpp_string(str_obj: *mut c_void) -> Option<String> {
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    unsafe {
        if str_obj.is_null() {
            return None;
        }
        let len = *(str_obj.byte_add(0x10) as *const i32);
        if len <= 0 || len > 4096 {
            return None;
        }
        let chars = str_obj.byte_add(0x14) as *const u16;
        let slice = std::slice::from_raw_parts(chars, len as usize);
        String::from_utf16(slice).ok()
    }
}

/// Read a CodeStage `ObscuredInt` field and decrypt it.
/// Layout: the struct's first 8 bytes are `cryptoKey` (i32 LE) then `hiddenValue`
/// (i32 LE); the plaintext is `hiddenValue ^ cryptoKey`.
pub(super) unsafe fn read_obscured_int_field(obj: *mut c_void, field: *mut c_void) -> i32 {
    let mut buf = [0u8; 16];
    // SAFETY: IL2CPP object and field pointers from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), buf.as_mut_ptr() as *mut c_void);
    }
    let key = i32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let hidden = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    hidden ^ key
}

/// Resolve an instance method by name/arg-count from an object's runtime klass.
/// Returns the MethodInfo pointer, or `None` if the object or method is missing.
pub(super) unsafe fn resolve_obj_method(obj: *mut c_void, name: &str, args: i32) -> Option<*const c_void> {
    if obj.is_null() {
        return None;
    }
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let klass = unsafe { *(obj as *const *mut c_void) };
    Sdk::get().get_method(klass.cast(), name, args).map(|m| m.cast())
}

/// Call `Dictionary<K,V>.TryGetValue(K key, out V)` with an i32/enum key.
/// Returns the value object pointer (reference-type `V`), or null if the key is
/// absent. The out buffer is over-sized so a small value-type `V` cannot corrupt
/// the stack; only the first word (the object pointer) is interpreted.
pub(super) unsafe fn dict_try_get_obj(dict: *mut c_void, mi: *const c_void, key: i32) -> *mut c_void {
    let mut out = [std::ptr::null_mut::<c_void>(); 4];
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, i32, *mut c_void, *const c_void) -> u8 =
        unsafe { std::mem::transmute(method_ptr(mi)) };
    let ok = fp(dict, key, out.as_mut_ptr() as *mut c_void, mi);
    if ok != 0 {
        out[0]
    } else {
        std::ptr::null_mut()
    }
}

/// Read an IL2CPP `List<T>` field from an object.
/// Returns (list_ptr, count, get_Item method) or None.
pub unsafe fn read_list_field(
    obj: *mut c_void,
    field_name: &std::ffi::CStr,
) -> Option<(*mut c_void, i32, *const c_void)> {
    let sdk = Sdk::get();
    let field_s = field_name.to_str().ok()?;
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let obj_klass = unsafe { *(obj as *const *mut c_void) };
    let field = sdk.get_field_from_name(obj_klass.cast(), field_s)?;

    let mut list_ptr: *mut c_void = std::ptr::null_mut();
    // SAFETY: IL2CPP object and field from resolved metadata.
    unsafe {
        sdk.get_field_value(obj.cast(), field, &mut list_ptr as *mut _ as *mut c_void);
    }
    if list_ptr.is_null() {
        return None;
    }

    // SAFETY: IL2CPP list object layout — klass pointer at object head.
    let list_klass = unsafe { *(list_ptr as *const *mut c_void) };
    let m_count = sdk.get_method(list_klass.cast(), "get_Count", 0)?;
    let m_item = sdk.get_method(list_klass.cast(), "get_Item", 1)?;

    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let count = unsafe { call_i32(list_ptr, m_count.cast()) };
    Some((list_ptr, count, m_item.cast()))
}

/// Read an IL2CPP managed reference-type array (`T[]`).
/// Layout (64-bit): bounds/length live in the array header; `max_length`
/// (element count) is at offset `0x18` and the inline element buffer starts at
/// `0x20`. For reference-type arrays each element slot is an object pointer.
/// Returns `(elements_base_ptr, length)` or `None` if null/implausible.
pub(super) unsafe fn read_obj_array(array: *mut c_void) -> Option<(*const *mut c_void, usize)> {
    if array.is_null() {
        return None;
    }
    // SAFETY: IL2CPP array header — element count at offset 0x18.
    let len = unsafe { *(array.byte_add(0x18) as *const usize) };
    if len > 4096 {
        return None;
    }
    // SAFETY: element pointers begin at offset 0x20.
    let base = unsafe { array.byte_add(0x20) as *const *mut c_void };
    Some((base, len))
}

/// Read an IL2CPP value-type `ObscuredInt[]` array and decrypt each element.
/// Layout (64-bit): element count at offset `0x18`, inline element buffer at
/// `0x20`; each `ObscuredInt` is 8 bytes (`cryptoKey` i32 LE, `hiddenValue` i32
/// LE), plaintext = `hiddenValue ^ cryptoKey`. Returns an empty vec if null or
/// implausibly large.
pub(super) unsafe fn read_obscured_int_array(array: *mut c_void) -> Vec<i32> {
    if array.is_null() {
        return Vec::new();
    }
    // SAFETY: IL2CPP array header — element count at offset 0x18.
    let len = unsafe { *(array.byte_add(0x18) as *const usize) };
    if len > 4096 {
        return Vec::new();
    }
    // SAFETY: inline value-type elements begin at offset 0x20, 8 bytes each.
    let base = unsafe { array.byte_add(0x20) as *const u8 };
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        // SAFETY: i < len, each element is 8 bytes within the array buffer.
        let key = unsafe { *(base.add(i * 8) as *const i32) };
        // SAFETY: hiddenValue immediately follows the key.
        let hidden = unsafe { *(base.add(i * 8 + 4) as *const i32) };
        out.push(hidden ^ key);
    }
    out
}

/// Read a plain `System.Int32` instance field by name from an object.
/// Returns `0` if the object is null or the field cannot be resolved.
pub(super) unsafe fn read_i32_field(obj: *mut c_void, field_name: &str) -> i32 {
    if obj.is_null() {
        return 0;
    }
    let sdk = Sdk::get();
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let klass = unsafe { *(obj as *const *mut c_void) };
    let Some(field) = sdk.get_field_from_name(klass.cast(), field_name) else {
        return 0;
    };
    let mut val: i32 = 0;
    // SAFETY: IL2CPP object and field from resolved metadata.
    unsafe {
        sdk.get_field_value(obj.cast(), field, &mut val as *mut _ as *mut c_void);
    }
    val
}
