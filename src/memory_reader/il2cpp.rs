//! Low-level IL2CPP call/read primitives shared by the entity readers.

use std::ffi::c_void;
use std::sync::OnceLock;

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

/// Call an instance method that returns `i64`.
#[inline]
pub(super) unsafe fn call_i64(this: *mut c_void, mi: *const c_void) -> i64 {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, *const c_void) -> i64 = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, mi)
}

/// Call an instance method that returns `bool` (IL2CPP uses u8).
#[inline]
pub(super) unsafe fn call_bool(this: *mut c_void, mi: *const c_void) -> bool {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, *const c_void) -> u8 = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, mi) != 0
}

/// Call an instance method that takes one `i32` arg and returns `bool`.
/// Used for `WorkSingleModeScenarioLive.CanGetTreeSquare(squareId)`.
#[inline]
pub(super) unsafe fn call_bool_with_i32(this: *mut c_void, mi: *const c_void, arg: i32) -> bool {
    // SAFETY: Transmuting IL2CPP MethodInfo pointer to callable function pointer.
    let fp: extern "C" fn(*mut c_void, i32, *const c_void) -> u8 = unsafe { std::mem::transmute(method_ptr(mi)) };
    fp(this, arg, mi) != 0
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

/// Scratch space for reading one `Obscured*` value out of a field.
///
/// `il2cpp_field_get_value` copies the **whole** value type, not the part the
/// caller wants, so the buffer has to fit the largest of them. Measured from
/// the game's own metadata (`il2cpp_classes.txt`, Global build):
///
/// ```text
/// ObscuredInt    currentCryptoKey i32, hiddenValue i32, inited bool,
///                fakeValue i32, fakeValueActive bool        →  20 bytes
/// ObscuredLong   the same shape in i64                      →  40 bytes
/// ObscuredBool   byte key, i32 hidden, three bools          →  12 bytes
/// ```
///
/// Sizing these to the two fields instead of the whole struct smashed the
/// stack on every read, and took the game down (`0xc0000005`) once the veteran
/// export made thousands of them. 64 leaves room for a field being added.
const OBSCURED_BUF: usize = 64;

/// `(class_from_type, class_value_size)`, resolved once, or `None` on a build
/// that does not export them.
fn value_size_api() -> Option<&'static (
    unsafe extern "C" fn(*const c_void) -> *mut c_void,
    unsafe extern "C" fn(*mut c_void, *mut u32) -> i32,
)> {
    #[allow(clippy::type_complexity)]
    static API: OnceLock<
        Option<(
            unsafe extern "C" fn(*const c_void) -> *mut c_void,
            unsafe extern "C" fn(*mut c_void, *mut u32) -> i32,
        )>,
    > = OnceLock::new();
    API.get_or_init(|| {
        let sdk = Sdk::get();
        // SAFETY: both are IL2CPP C API exports with the signatures declared here.
        unsafe {
            Some((
                std::mem::transmute(sdk.resolve_symbol("il2cpp_class_from_type")?),
                std::mem::transmute(sdk.resolve_symbol("il2cpp_class_value_size")?),
            ))
        }
    })
    .as_ref()
}

/// Whether a value-type field fits [`OBSCURED_BUF`], asked of the runtime
/// rather than assumed.
///
/// The buffer is sized from layouts read out of a metadata dump, and a dump is
/// a fact about one build. This is the same question the runtime can answer
/// about the build actually running, so a field that outgrows the buffer is
/// skipped instead of overrunning it. `true` when the runtime cannot be asked,
/// which leaves the documented sizes as the fallback.
unsafe fn value_fits_buffer(field: *mut c_void) -> bool {
    let Some((class_from_type, class_value_size)) = value_size_api() else {
        return true;
    };
    // SAFETY: IL2CPP FieldInfo — the type pointer follows the name pointer.
    let ftype = unsafe { *(field as *const *const c_void).add(1) };
    if ftype.is_null() {
        return true;
    }
    // SAFETY: `ftype` came from a resolved FieldInfo.
    let klass = unsafe { class_from_type(ftype) };
    if klass.is_null() {
        return true;
    }
    let mut align: u32 = 0;
    // SAFETY: `klass` came from the runtime.
    let size = unsafe { class_value_size(klass, &raw mut align) };
    if size <= OBSCURED_BUF as i32 {
        return true;
    }
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        hlog_error!("obscured read skipped: field wants {size} bytes, buffer is {OBSCURED_BUF}");
    }
    false
}

/// Read a CodeStage `ObscuredInt` field and decrypt it.
/// Layout: the struct's first 8 bytes are `currentCryptoKey` (i32 LE) then
/// `hiddenValue` (i32 LE); the plaintext is `hiddenValue ^ currentCryptoKey`.
pub(super) unsafe fn read_obscured_int_field(obj: *mut c_void, field: *mut c_void) -> i32 {
    // SAFETY: `field` is a resolved FieldInfo.
    if !unsafe { value_fits_buffer(field) } {
        return 0;
    }
    let mut buf = [0u8; OBSCURED_BUF];
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

/// Read an IL2CPP `List<T>` field from an object, by logical name.
///
/// Resolved through [`field_by_name`], so `"successionCharaList"` finds
/// `<SuccessionCharaList>k__BackingField` and `"upgradeHistoryList"` finds
/// `_upgradeHistoryList` without the caller knowing which.
///
/// Returns (list_ptr, count, get_Item method) or None.
pub unsafe fn read_list_field(obj: *mut c_void, field_name: &str) -> Option<(*mut c_void, i32, *const c_void)> {
    let sdk = Sdk::get();
    // SAFETY: `obj` is a live IL2CPP object; a missing field yields None.
    let field = unsafe { field_by_name(obj, &[field_name]) }?;

    let mut list_ptr: *mut c_void = std::ptr::null_mut();
    // SAFETY: IL2CPP object and field from resolved metadata.
    unsafe {
        sdk.get_field_value(obj.cast(), field.cast(), &mut list_ptr as *mut _ as *mut c_void);
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

/// The elements of a `List<T>` field, read out of the list rather than asked
/// for.
///
/// `List<T>` keeps its storage in `_items` (a `T[]`, usually longer than the
/// list) and its length in `_size`. Reading both is equivalent to calling
/// `get_Count` and `get_Item`, minus the two managed calls — and a call into
/// game code can throw, which across this FFI boundary aborts the process.
///
/// Empty when the field is missing, the list is null, or the length is not
/// plausible for the array behind it.
pub(super) unsafe fn read_list_items(obj: *mut c_void, names: &[&str]) -> Vec<*mut c_void> {
    // SAFETY: reads a named managed field; null when it is not there.
    let list = unsafe { read_ref_field(obj, names) };
    if list.is_null() {
        return Vec::new();
    }
    // SAFETY: `list` is a live List<T>.
    let items = unsafe { read_ref_field(list, &["items"]) };
    // SAFETY: as above.
    let size = unsafe { read_i32_field_any(list, &["size"]) };
    // SAFETY: `items` is the live backing array, or null.
    let Some((base, capacity)) = (unsafe { read_obj_array(items) }) else {
        return Vec::new();
    };
    let len = usize::try_from(size).unwrap_or(0).min(capacity);
    (0..len)
        // SAFETY: i < len <= capacity, and each slot holds an object pointer.
        .map(|i| unsafe { *base.add(i) })
        .filter(|item| !item.is_null())
        .collect()
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
    // Element size comes from the runtime, never assumed. `ObscuredInt` is not
    // two ints: it carries `currentCryptoKey`, `hiddenValue`, `inited`,
    // `fakeValue` and `fakeValueActive`, so the stride is well over 8 bytes.
    // Striding by 8 decodes element 0 correctly — the two ints it needs are
    // first — and then reads across field boundaries for every element after,
    // which looks like plausible-but-wrong data rather than an obvious failure.
    // Element 1 came back as `fakeValue ^ inited`, which is a very convincing 1.
    // SAFETY: `array` is a live IL2CPP array object.
    let Some(stride) = (unsafe { array_element_size(array) }) else {
        return Vec::new();
    };
    // SAFETY: inline value-type elements begin at offset 0x20.
    let base = unsafe { array.byte_add(0x20) as *const u8 };
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        // SAFETY: i < len and `stride` is the runtime's own element size, so
        // each read stays inside the array buffer. `currentCryptoKey` and
        // `hiddenValue` are the first two fields of the element.
        let key = unsafe { *(base.add(i * stride) as *const i32) };
        // SAFETY: hiddenValue immediately follows the key.
        let hidden = unsafe { *(base.add(i * stride + 4) as *const i32) };
        out.push(hidden ^ key);
    }
    out
}

/// Element size of an IL2CPP array, from the runtime's own metadata.
///
/// # Ask about the element, not the array
///
/// `il2cpp_class_array_element_size` answers "how big is this class *when used
/// as an array element*". Handed an array class it answers truthfully and
/// uselessly: 8, the size of a reference. The element class has to be fetched
/// first, and only then does the question mean what it looks like it means.
/// This cost a run to find, because 8 is also the wrong stride that decodes
/// element zero perfectly.
///
/// Returns `None` when either export is unavailable — better to read nothing
/// than to walk an array on a guessed stride.
///
/// # Safety
/// `array` must be a valid non-null IL2CPP array object.
unsafe fn array_element_size(array: *mut c_void) -> Option<usize> {
    let element_size_fn = array_metadata_fns().1?;
    // SAFETY: forwarded — `array` is a valid non-null IL2CPP array object.
    let element_class = unsafe { array_element_class(array) }?;
    // SAFETY: resolved IL2CPP export, called with the class the question is
    // actually about.
    let element_size: unsafe extern "C" fn(*mut c_void) -> i32 = unsafe { std::mem::transmute(element_size_fn) };
    let size = unsafe { element_size(element_class) };
    let stride = usize::try_from(size).ok().filter(|&s| s >= 8);
    // Logged once: the reported size has been wrong before, and the symptom —
    // every Nth element decoding correctly and the rest looking like plausible
    // ids — is not something the values alone reveal.
    static REPORTED: std::sync::Once = std::sync::Once::new();
    REPORTED.call_once(|| {
        hlog_info!("il2cpp array element size reported as {size} (using {stride:?})");
    });
    stride
}

/// `(il2cpp_class_get_element_class, il2cpp_class_array_element_size)`,
/// resolved once. Both or neither: one without the other cannot answer.
static ARRAY_METADATA_FNS: OnceLock<(Option<usize>, Option<usize>)> = OnceLock::new();

/// The two array-metadata exports, resolved on first use.
fn array_metadata_fns() -> (Option<usize>, Option<usize>) {
    *ARRAY_METADATA_FNS.get_or_init(|| {
        let sdk = Sdk::get();
        let fns = (
            sdk.resolve_symbol("il2cpp_class_get_element_class"),
            sdk.resolve_symbol("il2cpp_class_array_element_size"),
        );
        if fns.0.is_none() || fns.1.is_none() {
            hlog_error!("il2cpp array metadata exports unavailable; typed arrays cannot be read");
        }
        (fns.0.map(|p| p as usize), fns.1.map(|p| p as usize))
    })
}

/// The class of an array's elements, from the runtime.
///
/// # Safety
/// `array` must be a valid non-null IL2CPP array object.
pub(super) unsafe fn array_element_class(array: *mut c_void) -> Option<*mut c_void> {
    let element_class_fn = array_metadata_fns().0?;
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let array_class = unsafe { *(array as *const *mut c_void) };
    if array_class.is_null() {
        return None;
    }
    // SAFETY: resolved IL2CPP export, called with this array's own class.
    let get_element_class: unsafe extern "C" fn(*mut c_void) -> *mut c_void =
        unsafe { std::mem::transmute(element_class_fn) };
    // SAFETY: as above.
    let element = unsafe { get_element_class(array_class) };
    (!element.is_null()).then_some(element)
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

// ---------------------------------------------------------------------------
// Reading by field name
// ---------------------------------------------------------------------------

/// The spellings the game gives one field, in the order they are tried.
///
/// C# writes the same value four ways depending on how the field was declared,
/// and `WorkTrainedCharaData.TrainedCharaData` uses all four: `_viewerId`
/// (private), `<ScenarioId>k__BackingField` (auto-property), `Type` (public),
/// and the bare name. Asking for one spelling on a class that chose another
/// returns a silent zero, which reads as data.
///
/// Callers pass the logical name (`"viewerId"`) and this covers the rest.
fn field_name_variants(name: &str) -> [String; 4] {
    let base = name.strip_prefix('_').unwrap_or(name);
    let mut capitalized = String::with_capacity(base.len());
    let mut chars = base.chars();
    if let Some(first) = chars.next() {
        capitalized.extend(first.to_uppercase());
        capitalized.push_str(chars.as_str());
    }
    [
        name.to_string(),
        format!("_{base}"),
        format!("<{capitalized}>k__BackingField"),
        capitalized,
    ]
}

/// Resolve an instance field from an object's runtime klass, trying each
/// candidate name and each spelling of it.
///
/// Several *names* because the same field is called different things across
/// builds and across the two `Dictionary` implementations Unity has shipped
/// (`entries` vs `_entries`); several *spellings* of each because of
/// [`field_name_variants`]. The exact name given is always tried first, so a
/// caller that knows the real spelling is never second-guessed.
///
/// The runtime's own lookup walks the class hierarchy, so a field declared on
/// a base class (`AcquiredSkill` keeps `_masterId` on `SkillDataBase`) resolves
/// from the derived klass.
unsafe fn field_by_name(obj: *mut c_void, names: &[&str]) -> Option<*mut c_void> {
    if obj.is_null() {
        return None;
    }
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let klass = unsafe { *(obj as *const *mut c_void) };
    let sdk = Sdk::get();
    names
        .iter()
        .flat_map(|name| field_name_variants(name))
        .find_map(|name| sdk.get_field_from_name(klass.cast(), &name))
        .map(|f| f.cast())
}

/// Read a managed reference field (object, array or string) by name.
/// Null when the object is null, the field is missing, or the field is null.
///
/// The destination is over-sized and only its first word is read, because
/// `il2cpp_field_get_value` copies whatever the field's type is: a caller that
/// names a value-type field by mistake then gets a wrong answer instead of a
/// smashed stack. Same guard, same reason as [`dict_try_get_obj`].
pub(super) unsafe fn read_ref_field(obj: *mut c_void, names: &[&str]) -> *mut c_void {
    let Some(field) = (unsafe { field_by_name(obj, names) }) else {
        return std::ptr::null_mut();
    };
    let mut out = [0u8; OBSCURED_BUF];
    // SAFETY: IL2CPP object and field from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), out.as_mut_ptr().cast());
    }
    // SAFETY: the buffer is at least word-wide; read unaligned because a `u8`
    // array carries no alignment guarantee of its own.
    unsafe { out.as_ptr().cast::<*mut c_void>().read_unaligned() }
}

/// Read a plain `System.Int32` field by name, trying each candidate.
pub(super) unsafe fn read_i32_field_any(obj: *mut c_void, names: &[&str]) -> i32 {
    let Some(field) = (unsafe { field_by_name(obj, names) }) else {
        return 0;
    };
    let mut val: i32 = 0;
    // SAFETY: IL2CPP object and field from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), (&raw mut val).cast());
    }
    val
}

/// Read a CodeStage `ObscuredInt` field by name. `0` when it is not there.
pub(super) unsafe fn read_obscured_int(obj: *mut c_void, name: &str) -> i32 {
    let Some(field) = (unsafe { field_by_name(obj, &[name]) }) else {
        return 0;
    };
    // SAFETY: field resolved from this object's own klass.
    unsafe { read_obscured_int_field(obj, field) }
}

/// Read a CodeStage `ObscuredLong` field by name.
///
/// Same shape as [`read_obscured_int_field`] at 64-bit width: `currentCryptoKey`
/// then `hiddenValue`, plaintext = `hiddenValue ^ currentCryptoKey`.
pub(super) unsafe fn read_obscured_long(obj: *mut c_void, name: &str) -> i64 {
    let Some(field) = (unsafe { field_by_name(obj, &[name]) }) else {
        return 0;
    };
    // SAFETY: `field` was just resolved from this object's klass.
    if !unsafe { value_fits_buffer(field) } {
        return 0;
    }
    let mut buf = [0u8; OBSCURED_BUF];
    // SAFETY: IL2CPP object and field pointers from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), buf.as_mut_ptr().cast());
    }
    let key = i64::from_le_bytes(buf[0..8].try_into().unwrap_or_default());
    let hidden = i64::from_le_bytes(buf[8..16].try_into().unwrap_or_default());
    hidden ^ key
}

/// Read a CodeStage `ObscuredBool` field by name.
///
/// Layout is a `byte currentCryptoKey` then, at the next 4-byte boundary, an
/// `int hiddenValue` — so the key is one byte wide even though what it keys is
/// four.
pub(super) unsafe fn read_obscured_bool(obj: *mut c_void, name: &str) -> bool {
    let Some(field) = (unsafe { field_by_name(obj, &[name]) }) else {
        return false;
    };
    // SAFETY: `field` was just resolved from this object's klass.
    if !unsafe { value_fits_buffer(field) } {
        return false;
    }
    let mut buf = [0u8; OBSCURED_BUF];
    // SAFETY: IL2CPP object and field pointers from resolved metadata.
    unsafe {
        Sdk::get().get_field_value(obj.cast(), field.cast(), buf.as_mut_ptr().cast());
    }
    let key = i32::from(buf[0]);
    let hidden = i32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    (hidden ^ key) != crate::il2cpp_json::OBSCURED_FALSE
}

/// Read a CodeStage `ObscuredString` field by name, decrypted. `""` when the
/// field, the wrapper or its key is missing.
///
/// `ObscuredString` is a class, so the field holds a reference: the key is a
/// plain `System.String` and the ciphertext a `byte[]` of UTF-16 code units.
pub(super) unsafe fn read_obscured_string(obj: *mut c_void, name: &str) -> String {
    // SAFETY: reading a managed reference field from a live object.
    let wrapper = unsafe { read_ref_field(obj, &[name]) };
    if wrapper.is_null() {
        return String::new();
    }
    // SAFETY: `wrapper` is a live ObscuredString.
    let key = unsafe { read_ref_field(wrapper, &["currentCryptoKey"]) };
    // SAFETY: as above.
    let hidden = unsafe { read_ref_field(wrapper, &["hiddenValue"]) };
    // SAFETY: `key` is a System.String or null.
    let Some(key) = (unsafe { read_il2cpp_string(key) }) else {
        return String::new();
    };
    // SAFETY: `hidden` is a byte[] or null.
    let bytes = unsafe { read_byte_array(hidden) };
    decode_obscured_string(&key, &bytes)
}

/// Decrypt a CodeStage `ObscuredString` payload.
///
/// The ciphertext is the UTF-16LE encoding of the string with every code unit
/// XORed against the key's code units, cycling. Ported from umadump's
/// `ObscuredString.value` (`game_structs.py`).
///
/// Pure, so the algorithm can be tested without a game attached.
pub(super) fn decode_obscured_string(key: &str, hidden: &[u8]) -> String {
    let key: Vec<u16> = key.encode_utf16().collect();
    if key.is_empty() || hidden.len() < 2 {
        return String::new();
    }
    let decoded: Vec<u16> = hidden
        .chunks_exact(2)
        .enumerate()
        .map(|(i, pair)| u16::from_le_bytes([pair[0], pair[1]]) ^ key[i % key.len()])
        .collect();
    String::from_utf16_lossy(&decoded).trim_end_matches('\u{0}').to_owned()
}

/// Read an IL2CPP `byte[]` into a `Vec<u8>`. Empty when null or implausible.
pub(super) unsafe fn read_byte_array(array: *mut c_void) -> Vec<u8> {
    if array.is_null() {
        return Vec::new();
    }
    // SAFETY: IL2CPP array header — element count at offset 0x18.
    let len = unsafe { *(array.byte_add(0x18) as *const usize) };
    if len > (1 << 20) {
        return Vec::new();
    }
    // SAFETY: inline element storage begins at 0x20; `len` bytes of it.
    unsafe { std::slice::from_raw_parts(array.byte_add(0x20) as *const u8, len) }.to_vec()
}

// ---------------------------------------------------------------------------
// Dictionary
// ---------------------------------------------------------------------------

/// Every live value in a `Dictionary<K, V>` whose `V` is a reference type.
///
/// # Why the backing array and not the API
///
/// `Dictionary` offers no enumeration this side of the boundary that does not
/// mean building a managed enumerator struct and calling game code with it.
/// The backing `entries` array is one field read, and the same thing umadump
/// walks — but where umadump hardcodes the entry offsets from a ctypes layout,
/// everything here comes from the runtime's own metadata, so a build that
/// reorders the struct is followed rather than misread.
///
/// A slot is live when its `hashCode` is positive; the rest are holes left by
/// removals and hold stale pointers.
pub(super) unsafe fn dict_values(dict: *mut c_void) -> Vec<*mut c_void> {
    if dict.is_null() {
        return Vec::new();
    }
    // SAFETY: reading managed fields from a live Dictionary.
    let entries = unsafe { read_ref_field(dict, &["entries", "_entries"]) };
    if entries.is_null() {
        return Vec::new();
    }
    // `count` is how far into `entries` the dictionary has ever written, which
    // is the bound to walk — `Count` (live pairs) would stop short of entries
    // that sit past a hole.
    // SAFETY: as above.
    let count = unsafe { read_i32_field_any(dict, &["count", "_count"]) };

    // SAFETY: `entries` is a live IL2CPP array.
    let Some((stride, element)) = (unsafe { array_layout(entries) }) else {
        return Vec::new();
    };
    let sdk = Sdk::get();
    let (Some(hash_at), Some(value_at)) = (
        field_offset(sdk.get_field_from_name(element.cast(), "hashCode")),
        field_offset(sdk.get_field_from_name(element.cast(), "value")),
    ) else {
        hlog_warn!(target: "training-tracker", "dict_values: Entry has no hashCode/value field");
        return Vec::new();
    };

    // SAFETY: IL2CPP array header — element count at offset 0x18.
    let capacity = unsafe { *(entries.byte_add(0x18) as *const usize) };
    let used = usize::try_from(count).unwrap_or(0).min(capacity);
    // SAFETY: inline element storage begins at 0x20.
    let base = unsafe { entries.byte_add(0x20) as *const u8 };

    let mut out = Vec::with_capacity(used);
    for i in 0..used {
        // SAFETY: i < used <= capacity, and both offsets are the runtime's own
        // within an element of `stride` bytes.
        let (hash, value) = unsafe {
            let slot = base.add(i * stride);
            (
                *(slot.add(hash_at) as *const i32),
                *(slot.add(value_at) as *const *mut c_void),
            )
        };
        if hash > 0 && !value.is_null() {
            out.push(value);
        }
    }
    out
}

/// `(element stride, element class)` for a live array, both from the runtime.
///
/// # Safety
/// `array` must be a valid non-null IL2CPP array object.
pub(super) unsafe fn array_layout(array: *mut c_void) -> Option<(usize, *mut c_void)> {
    // SAFETY: forwarded — `array` is a live IL2CPP array object.
    let stride = unsafe { array_element_size(array) }?;
    // SAFETY: as above.
    let element = unsafe { array_element_class(array) }?;
    Some((stride, element))
}

/// A field's offset from the start of its struct — i.e. what the runtime
/// reports, less the object header it measures from.
///
/// `None` for a missing field, or a static or constant, which are not part of
/// any instance.
fn field_offset(field: Option<*mut crate::compat::FieldInfo>) -> Option<usize> {
    /// Il2CppObject header (klass pointer + monitor); reported offsets include it.
    const OBJECT_HEADER: i32 = 0x10;
    let field = field?;
    let get_offset =
        (*FIELD_OFFSET_FN.get_or_init(|| Sdk::get().resolve_symbol("il2cpp_field_get_offset").map(|p| p as usize)))?;
    // SAFETY: resolved IL2CPP export, called with a FieldInfo from metadata.
    let f: unsafe extern "C" fn(*mut c_void) -> i32 = unsafe { std::mem::transmute(get_offset) };
    // SAFETY: as above.
    let offset = unsafe { f(field.cast()) };
    (offset >= OBJECT_HEADER).then(|| (offset - OBJECT_HEADER) as usize)
}

/// `il2cpp_field_get_offset`, resolved once.
static FIELD_OFFSET_FN: OnceLock<Option<usize>> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::{decode_obscured_string, field_name_variants};

    /// The encryption is its own inverse, so the test can build the ciphertext
    /// the way the game does and check the reader gets the plaintext back.
    fn encrypt(plain: &str, key: &str) -> Vec<u8> {
        let key: Vec<u16> = key.encode_utf16().collect();
        plain
            .encode_utf16()
            .enumerate()
            .flat_map(|(i, unit)| (unit ^ key[i % key.len()]).to_le_bytes())
            .collect()
    }

    #[test]
    fn an_obscured_string_round_trips() {
        let plain = "2026-03-14 23:42:18";
        let key = "k3y";
        assert_eq!(decode_obscured_string(key, &encrypt(plain, key)), plain);
    }

    #[test]
    fn a_trailing_terminator_is_dropped() {
        let key = "abc";
        assert_eq!(decode_obscured_string(key, &encrypt("hi\u{0}\u{0}", key)), "hi");
    }

    #[test]
    fn nothing_readable_decodes_to_nothing() {
        assert_eq!(decode_obscured_string("", &[1, 2]), "");
        assert_eq!(decode_obscured_string("key", &[]), "");
    }

    /// The four spellings, against real names from the Global build's dump.
    /// A regression here is a reader that silently returns zero, so the cases
    /// are the exact fields that caught it: `_viewerId` (private),
    /// `<ScenarioId>k__BackingField` (auto-property) and `Type` (public).
    #[test]
    fn one_logical_name_covers_the_four_ways_csharp_spells_a_field() {
        assert!(field_name_variants("viewerId").contains(&"_viewerId".to_string()));
        assert!(field_name_variants("scenarioId").contains(&"<ScenarioId>k__BackingField".to_string()));
        assert!(field_name_variants("type").contains(&"Type".to_string()));
        // The name as given is always tried first, so a caller that already
        // knows the real spelling is never second-guessed.
        assert_eq!(field_name_variants("_dataDic")[0], "_dataDic");
        // An underscored name still reaches the undecorated spellings.
        assert!(field_name_variants("_skillTipsList").contains(&"<SkillTipsList>k__BackingField".to_string()));
    }
}
