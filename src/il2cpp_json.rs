//! Convert an arbitrary live IL2CPP object into `serde_json::Value`.
//!
//! Every other reader in this plugin names the fields it wants. This one names
//! nothing: it walks whatever it is handed, field by field, through the IL2CPP
//! metadata. That is the right shape for exporting a server response, where the
//! point is to keep everything — including the parts nobody has decoded yet —
//! so the analysis can happen later and off the game's clock.
//!
//! Modelled on the same job in horse-act (`src/reflection.rs`).
//!
//! # What it understands
//!
//! - primitives, `System.String`, and enums (rendered as their member name)
//! - `ObscuredInt/Long/Bool/Float/Double`, decrypted rather than dumped raw
//! - arrays, both reference and value-typed, at the runtime's own stride
//! - nested objects and structs, to [`MAX_DEPTH`]
//!
//! # What it refuses
//!
//! Cycles (recorded as `<cycle>` rather than followed), anything past
//! [`MAX_DEPTH`], and arrays longer than [`MAX_ARRAY`]. A response that trips
//! one of those produces a marker string in place of that branch, never a hang
//! and never a partial file.
//!
//! # Thread contract
//!
//! Unity main thread, on an object the caller knows is live. It reads field
//! memory directly and calls no game code, so it cannot re-enter the runtime —
//! but it is still walking live objects, and a torn-down one is a bad pointer
//! like any other.

use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr};
use std::sync::OnceLock;

use serde_json::{Map, Number, Value};

use crate::compat::Sdk;

/// How deep to follow object graphs. A career response nests maybe six levels;
/// past this something is wrong and the file is not worth the recursion.
const MAX_DEPTH: usize = 24;
/// Arrays longer than this are summarised rather than expanded.
const MAX_ARRAY: usize = 4096;

/// IL2CPP field attribute bits (ECMA-335 `FieldAttributes`).
const FIELD_STATIC: i32 = 0x0010;
const FIELD_LITERAL: i32 = 0x0040;

/// Il2CppObject header size: klass pointer + monitor. Field offsets from the
/// metadata are measured from the object head and include it.
const OBJECT_HEADER: i32 = 0x10;

/// The IL2CPP C exports this walker needs, resolved once.
struct Api {
    class_get_fields: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> *mut c_void,
    class_get_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    class_from_type: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    class_is_enum: unsafe extern "C" fn(*mut c_void) -> bool,
    class_is_valuetype: unsafe extern "C" fn(*mut c_void) -> bool,
    class_get_element_class: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    class_value_size: unsafe extern "C" fn(*mut c_void, *mut u32) -> i32,
    field_get_name: unsafe extern "C" fn(*mut c_void) -> *const c_char,
    field_get_offset: unsafe extern "C" fn(*mut c_void) -> i32,
    field_get_flags: unsafe extern "C" fn(*mut c_void) -> i32,
    field_get_type: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    field_static_get_value: unsafe extern "C" fn(*mut c_void, *mut c_void),
    type_get_type: unsafe extern "C" fn(*mut c_void) -> i32,
    object_get_class: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    array_length: unsafe extern "C" fn(*mut c_void) -> u32,
}

// SAFETY: function pointers into the IL2CPP runtime, stable for process lifetime.
unsafe impl Send for Api {}
// SAFETY: as above.
unsafe impl Sync for Api {}

static API: OnceLock<Option<Api>> = OnceLock::new();

fn api() -> Option<&'static Api> {
    API.get_or_init(|| {
        let sdk = Sdk::get();
        // SAFETY: each name is an IL2CPP C API export with the signature declared
        // in `Api`; a missing one yields `None` and disables the walker entirely.
        unsafe {
            Some(Api {
                class_get_fields: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_fields")?),
                class_get_name: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_name")?),
                class_from_type: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_from_il2cpp_type")?),
                class_is_enum: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_is_enum")?),
                class_is_valuetype: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_is_valuetype")?),
                class_get_element_class: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_get_element_class")?),
                class_value_size: std::mem::transmute(sdk.resolve_symbol("il2cpp_class_value_size")?),
                field_get_name: std::mem::transmute(sdk.resolve_symbol("il2cpp_field_get_name")?),
                field_get_offset: std::mem::transmute(sdk.resolve_symbol("il2cpp_field_get_offset")?),
                field_get_flags: std::mem::transmute(sdk.resolve_symbol("il2cpp_field_get_flags")?),
                field_get_type: std::mem::transmute(sdk.resolve_symbol("il2cpp_field_get_type")?),
                field_static_get_value: std::mem::transmute(sdk.resolve_symbol("il2cpp_field_static_get_value")?),
                type_get_type: std::mem::transmute(sdk.resolve_symbol("il2cpp_type_get_type")?),
                object_get_class: std::mem::transmute(sdk.resolve_symbol("il2cpp_object_get_class")?),
                array_length: std::mem::transmute(sdk.resolve_symbol("il2cpp_array_length")?),
            })
        }
    })
    .as_ref()
}

/// Whether the walker has everything it needs. Worth checking once at install
/// time so a missing export is reported then rather than at the moment a
/// response arrives and there is nothing to be done about it.
#[must_use]
pub fn is_available() -> bool {
    api().is_some()
}

/// Convert a live IL2CPP object to JSON.
///
/// `None` when the runtime exports are unavailable or `obj` is null. Panics
/// inside the walk are caught: a malformed graph costs the export, not the game.
///
/// # Safety
/// `obj` must be a live IL2CPP object, and the caller must be on the Unity main
/// thread with the object not concurrently torn down.
#[must_use]
pub unsafe fn object_to_json(obj: *mut c_void) -> Option<Value> {
    if obj.is_null() {
        return None;
    }
    api()?;
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut visited = HashSet::new();
        // SAFETY: forwarded from this function's own contract.
        unsafe { object(obj, 0, &mut visited) }
    })) {
        Ok(value) => Some(value),
        Err(_) => {
            hlog_error!("il2cpp_json: object_to_json PANICKED");
            None
        }
    }
}

/// Read a NUL-terminated runtime string, or `""`.
unsafe fn name_of(ptr: *const c_char) -> String {
    if ptr.is_null() {
        return String::new();
    }
    // SAFETY: IL2CPP metadata names are NUL-terminated and live for the process.
    unsafe { CStr::from_ptr(ptr) }.to_string_lossy().into_owned()
}

/// One object or boxed value: `{ field: value, … }`.
unsafe fn object(obj: *mut c_void, depth: usize, visited: &mut HashSet<usize>) -> Value {
    if obj.is_null() {
        return Value::Null;
    }
    let Some(api) = api() else { return Value::Null };
    if depth > MAX_DEPTH {
        return Value::String("<max depth>".to_string());
    }
    // A response graph shares sub-objects (the same chara appears in several
    // branches). Following one twice is fine; following a cycle is not, so the
    // guard is scoped to the current path and released on the way out.
    if !visited.insert(obj as usize) {
        return Value::String("<cycle>".to_string());
    }

    // SAFETY: `obj` is a live IL2CPP object; its class pointer heads the object.
    let klass = unsafe { (api.object_get_class)(obj) };
    if klass.is_null() {
        visited.remove(&(obj as usize));
        return Value::Null;
    }
    // SAFETY: `klass` came from the runtime.
    let class_name = unsafe { name_of((api.class_get_name)(klass)) };

    let value = if class_name.ends_with("[]") {
        // SAFETY: the class name says array, so the object has an array header.
        unsafe { array(obj, klass, depth, visited) }
    } else if class_name == "String" {
        // SAFETY: `obj` is a System.String.
        unsafe { read_string(obj) }
    } else {
        // SAFETY: fields start one object header in from the object head.
        unsafe { fields(obj.byte_add(OBJECT_HEADER as usize), klass, depth, visited) }
    };

    visited.remove(&(obj as usize));
    value
}

/// Walk every instance field of `klass` over a buffer whose offset zero is the
/// first field (i.e. past the object header for a class, the struct base for a
/// value type).
unsafe fn fields(base: *mut c_void, klass: *mut c_void, depth: usize, visited: &mut HashSet<usize>) -> Value {
    let Some(api) = api() else { return Value::Null };
    let mut map = Map::new();
    let mut iter: *mut c_void = std::ptr::null_mut();
    loop {
        // SAFETY: standard il2cpp_class_get_fields iteration; ends on null.
        let field = unsafe { (api.class_get_fields)(klass, &raw mut iter) };
        if field.is_null() {
            break;
        }
        // SAFETY: `field` came from the iteration above.
        let (flags, offset, name, ftype) = unsafe {
            (
                (api.field_get_flags)(field),
                (api.field_get_offset)(field),
                name_of((api.field_get_name)(field)),
                (api.field_get_type)(field),
            )
        };
        if flags & FIELD_STATIC != 0 || offset < OBJECT_HEADER {
            continue; // statics and constants are not this object's state
        }
        let addr = {
            let Ok(rel) = usize::try_from(offset - OBJECT_HEADER) else {
                continue;
            };
            // SAFETY: `rel` is the runtime's own offset for a field of this class.
            unsafe { base.byte_add(rel) }
        };
        // SAFETY: `addr` is that field's storage and `ftype` describes it.
        let value = unsafe { read(addr, ftype, depth, visited) };
        map.insert(normalise(&name), value);
    }
    Value::Object(map)
}

/// Read one field, dispatching on its `Il2CppTypeEnum`.
unsafe fn read(addr: *mut c_void, ftype: *mut c_void, depth: usize, visited: &mut HashSet<usize>) -> Value {
    let Some(api) = api() else { return Value::Null };
    // SAFETY: `ftype` is the runtime's type for this field.
    let kind = unsafe { (api.type_get_type)(ftype) };
    // SAFETY: every arm reads `addr` as the type the runtime just reported.
    unsafe {
        match kind {
            // BOOLEAN
            0x02 => Value::Bool(*addr.cast::<u8>() != 0),
            // CHAR, I1..U4
            0x03..=0x09 => Value::Number(Number::from(*addr.cast::<i32>())),
            // I8, U8
            0x0A | 0x0B => Value::Number(Number::from(*addr.cast::<i64>())),
            // R4
            0x0C => finite(f64::from(*addr.cast::<f32>())),
            // R8
            0x0D => finite(*addr.cast::<f64>()),
            // STRING, CLASS, SZARRAY, ARRAY, OBJECT, GENERICINST
            0x0E | 0x12 | 0x1C | 0x1D | 0x14 | 0x15 => object(*addr.cast::<*mut c_void>(), depth + 1, visited),
            // VALUETYPE — an inline struct, an enum, or an Obscured wrapper.
            0x11 => value_type(addr, ftype, depth, visited),
            // Anything else: report the width we can safely read rather than
            // guessing at a shape.
            _ => Value::Number(Number::from(*addr.cast::<i32>())),
        }
    }
}

/// An inline value type: enum, `Obscured*`, or a plain struct.
unsafe fn value_type(addr: *mut c_void, ftype: *mut c_void, depth: usize, visited: &mut HashSet<usize>) -> Value {
    let Some(api) = api() else { return Value::Null };
    // SAFETY: `ftype` is a live runtime type.
    let klass = unsafe { (api.class_from_type)(ftype) };
    if klass.is_null() {
        return Value::String("<unknown struct>".to_string());
    }
    // SAFETY: `klass` came from the runtime.
    let name = unsafe { name_of((api.class_get_name)(klass)) };

    if name.starts_with("Obscured") {
        // SAFETY: `addr` is the wrapper's inline storage.
        if let Some(plain) = unsafe { obscured(addr, klass, &name) } {
            return plain;
        }
    }
    // SAFETY: `klass` came from the runtime.
    if unsafe { (api.class_is_enum)(klass) } {
        // SAFETY: an enum's storage is its underlying integer.
        return unsafe { enum_name(addr, klass) };
    }
    // A struct's fields are measured from the object head like a class's, so the
    // same header adjustment applies even though there is no header here.
    // SAFETY: `addr` is the struct's inline storage.
    unsafe { fields(addr, klass, depth, visited) }
}

/// Decrypt a CodeStage `Obscured*` wrapper by finding its two fields by name.
///
/// Offsets are read from the metadata rather than assumed: the struct carries
/// more than the two integers, and the layout is not ours to predict.
unsafe fn obscured(addr: *mut c_void, klass: *mut c_void, class_name: &str) -> Option<Value> {
    let api = api()?;
    let (mut hidden_at, mut key_at) = (None, None);
    let mut iter: *mut c_void = std::ptr::null_mut();
    loop {
        // SAFETY: standard field iteration.
        let field = unsafe { (api.class_get_fields)(klass, &raw mut iter) };
        if field.is_null() {
            break;
        }
        // SAFETY: `field` came from the iteration.
        let (flags, offset, name) = unsafe {
            (
                (api.field_get_flags)(field),
                (api.field_get_offset)(field),
                name_of((api.field_get_name)(field)),
            )
        };
        if flags & FIELD_STATIC != 0 || offset < OBJECT_HEADER {
            continue;
        }
        let rel = usize::try_from(offset - OBJECT_HEADER).ok()?;
        match name.as_str() {
            "hiddenValue" => hidden_at = Some(rel),
            "currentCryptoKey" | "cryptoKey" => key_at = Some(rel),
            _ => {}
        }
    }
    let (hidden_at, key_at) = (hidden_at?, key_at?);
    // SAFETY: both offsets are the runtime's own, inside this wrapper's storage.
    unsafe {
        let h32 = || *addr.byte_add(hidden_at).cast::<i32>();
        let k32 = || *addr.byte_add(key_at).cast::<i32>();
        let h64 = || *addr.byte_add(hidden_at).cast::<i64>();
        let k64 = || *addr.byte_add(key_at).cast::<i64>();
        match class_name {
            "ObscuredInt" => Some(Value::Number(Number::from(h32() ^ k32()))),
            "ObscuredLong" => Some(Value::Number(Number::from(h64() ^ k64()))),
            "ObscuredBool" => Some(Value::Bool((h32() ^ k32()) != 0)),
            "ObscuredFloat" => Some(finite(f64::from(f32::from_bits((h32() as u32) ^ (k32() as u32))))),
            "ObscuredDouble" => Some(finite(f64::from_bits((h64() as u64) ^ (k64() as u64)))),
            _ => None,
        }
    }
}

/// An enum as its member name, falling back to the raw number for a value with
/// no name (flags combinations, or a member the build added).
unsafe fn enum_name(addr: *mut c_void, klass: *mut c_void) -> Value {
    let Some(api) = api() else { return Value::Null };
    // SAFETY: enum storage is its underlying integer; Gallop's are all Int32.
    let current = unsafe { *addr.cast::<i32>() };
    let mut iter: *mut c_void = std::ptr::null_mut();
    loop {
        // SAFETY: standard field iteration.
        let field = unsafe { (api.class_get_fields)(klass, &raw mut iter) };
        if field.is_null() {
            break;
        }
        // SAFETY: `field` came from the iteration.
        let flags = unsafe { (api.field_get_flags)(field) };
        if flags & FIELD_STATIC == 0 || flags & FIELD_LITERAL == 0 {
            continue; // `value__` and anything else that is not a member
        }
        let mut member: i32 = 0;
        // SAFETY: reading a literal static's constant into a matching i32 slot.
        unsafe { (api.field_static_get_value)(field, (&raw mut member).cast()) };
        if member == current {
            // SAFETY: `field` came from the iteration.
            return Value::String(unsafe { name_of((api.field_get_name)(field)) });
        }
    }
    Value::Number(Number::from(current))
}

/// An array, at the runtime's own element stride.
unsafe fn array(obj: *mut c_void, klass: *mut c_void, depth: usize, visited: &mut HashSet<usize>) -> Value {
    let Some(api) = api() else { return Value::Null };
    // SAFETY: `obj` is an IL2CPP array object.
    let len = unsafe { (api.array_length)(obj) } as usize;
    if len > MAX_ARRAY {
        return Value::String(format!("<array len={len}, not expanded>"));
    }
    // Inline element storage begins after the array header.
    // SAFETY: `obj` is an array object; 0x20 is its data offset on 64-bit.
    let data = unsafe { obj.byte_add(0x20) };

    // SAFETY: `klass` is the array's class.
    let element = unsafe { (api.class_get_element_class)(klass) };
    if element.is_null() {
        return Value::Null;
    }
    // SAFETY: `element` came from the runtime.
    if !unsafe { (api.class_is_valuetype)(element) } {
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: i < len, and reference elements are one pointer each.
            let item = unsafe { *data.cast::<*mut c_void>().add(i) };
            // SAFETY: `item` is a live element or null.
            out.push(unsafe { object(item, depth + 1, visited) });
        }
        return Value::Array(out);
    }

    // Value-typed elements are stored inline, and the stride is the runtime's
    // to report — never `size_of` of what the field looks like. Getting this
    // wrong reads across element boundaries and yields plausible nonsense.
    let mut align: u32 = 0;
    // SAFETY: `element` came from the runtime.
    let stride = unsafe { (api.class_value_size)(element, &raw mut align) };
    let Ok(stride) = usize::try_from(stride).map(|s| s.max(1)) else {
        return Value::String("<array of unknown stride>".to_string());
    };
    // SAFETY: `element` came from the runtime.
    let is_enum = unsafe { (api.class_is_enum)(element) };
    // SAFETY: as above.
    let name = unsafe { name_of((api.class_get_name)(element)) };

    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        // SAFETY: i < len and `stride` is the runtime's element size, so every
        // element stays inside the array buffer.
        let at = unsafe { data.byte_add(i * stride) };
        let value = if is_enum {
            // SAFETY: `at` is one element's storage.
            unsafe { enum_name(at, element) }
        } else if name.starts_with("Obscured") {
            // SAFETY: as above.
            unsafe { obscured(at, element, &name) }.unwrap_or(Value::Null)
        } else {
            // SAFETY: as above.
            unsafe { fields(at, element, depth + 1, visited) }
        };
        out.push(value);
    }
    Value::Array(out)
}

/// `System.String` → a JSON string. Layout: length at `0x10`, UTF-16 at `0x14`.
unsafe fn read_string(obj: *mut c_void) -> Value {
    // SAFETY: `obj` is a System.String object.
    let len = unsafe { *obj.byte_add(0x10).cast::<i32>() };
    let Ok(len) = usize::try_from(len) else {
        return Value::Null;
    };
    if len == 0 {
        return Value::String(String::new());
    }
    if len > 1 << 20 {
        return Value::String("<string too long>".to_string());
    }
    // SAFETY: `len` UTF-16 units start at 0x14 and belong to this string.
    let units = unsafe { std::slice::from_raw_parts(obj.byte_add(0x14).cast::<u16>(), len) };
    Value::String(String::from_utf16_lossy(units))
}

/// JSON has no NaN or infinity; those become null rather than failing the file.
fn finite(v: f64) -> Value {
    Number::from_f64(v).map_or(Value::Null, Value::Number)
}

/// Strip the compiler's decoration from a field name so the export reads like
/// the API it came from: `<Foo>k__BackingField` and `_foo` both become `foo`.
///
/// The response types themselves use plain snake_case names, which pass
/// through untouched; the decoration shows up in the game-side objects hanging
/// off them.
fn normalise(raw: &str) -> String {
    let mut name = raw.trim_start_matches('_');
    if let Some(inner) = name.strip_prefix('<').and_then(|s| s.strip_suffix(">k__BackingField")) {
        name = inner.trim_start_matches('_');
    }
    let mut chars = name.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().chain(chars).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::{finite, normalise};
    use serde_json::Value;

    #[test]
    fn field_names_lose_their_decoration() {
        assert_eq!(normalise("card_id"), "card_id");
        assert_eq!(normalise("_totalTurnNum"), "totalTurnNum");
        assert_eq!(normalise("<SimDataBase64>k__BackingField"), "simDataBase64");
        assert_eq!(normalise("<_charaName>k__BackingField"), "charaName");
        assert_eq!(normalise("CardId"), "cardId");
        assert_eq!(normalise(""), "");
        assert_eq!(normalise("_"), "");
    }

    /// JSON cannot hold NaN or infinity. Writing null keeps the rest of a large
    /// export rather than failing the whole file over one field.
    #[test]
    fn non_finite_numbers_become_null() {
        assert_eq!(finite(f64::NAN), Value::Null);
        assert_eq!(finite(f64::INFINITY), Value::Null);
        assert_eq!(finite(1.5), Value::from(1.5));
    }
}
