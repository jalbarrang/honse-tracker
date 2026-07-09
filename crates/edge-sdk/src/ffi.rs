//! Raw C types and function-pointer typedefs transcribed from
//! `hachimi-edge/src/core/plugin_api.rs` (API VERSION = 3).
//!
//! Do not invent signatures — every `Fn*` type here was read from the host source.
//! See `FFI_AUDIT.md` for the line-by-line audit table.

#![allow(non_camel_case_types)]
#![allow(non_upper_case_globals)] // edge Il2CppTypeEnum_* const names

use std::ffi::{c_char, c_uint, c_void};

// ── Opaque host / IL2CPP types ──────────────────────────────────────────────

/// Opaque host singleton (edge `Hachimi`).
#[repr(C)]
pub struct Hachimi {
    _private: [u8; 0],
}

/// Opaque interceptor handle (edge `Interceptor`).
#[repr(C)]
pub struct Interceptor {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Il2CppClass {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Il2CppImage {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Il2CppObject {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Il2CppString {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Il2CppArray {
    _private: [u8; 0],
}

#[repr(C)]
pub struct Il2CppThread {
    _private: [u8; 0],
}

#[repr(C)]
pub struct MethodInfo {
    _private: [u8; 0],
}

#[repr(C)]
pub struct FieldInfo {
    _private: [u8; 0],
}

/// Edge `Il2CppMethodPointer` (`usize`).
pub type Il2CppMethodPointer = usize;

/// Edge `il2cpp_array_size_t` (`usize`).
pub type il2cpp_array_size_t = usize;

// ── Il2CppTypeEnum (from edge `src/il2cpp/types.rs`) ─────────────────────────
// NOTE: edge defines this as a `c_uint` typedef + consts, NOT a Rust `enum`.
// Transcribed faithfully from the host source (task wording said "enum"; source wins).

pub type Il2CppTypeEnum = c_uint;

pub const Il2CppTypeEnum_IL2CPP_TYPE_END: Il2CppTypeEnum = 0;
pub const Il2CppTypeEnum_IL2CPP_TYPE_VOID: Il2CppTypeEnum = 1;
pub const Il2CppTypeEnum_IL2CPP_TYPE_BOOLEAN: Il2CppTypeEnum = 2;
pub const Il2CppTypeEnum_IL2CPP_TYPE_CHAR: Il2CppTypeEnum = 3;
pub const Il2CppTypeEnum_IL2CPP_TYPE_I1: Il2CppTypeEnum = 4;
pub const Il2CppTypeEnum_IL2CPP_TYPE_U1: Il2CppTypeEnum = 5;
pub const Il2CppTypeEnum_IL2CPP_TYPE_I2: Il2CppTypeEnum = 6;
pub const Il2CppTypeEnum_IL2CPP_TYPE_U2: Il2CppTypeEnum = 7;
pub const Il2CppTypeEnum_IL2CPP_TYPE_I4: Il2CppTypeEnum = 8;
pub const Il2CppTypeEnum_IL2CPP_TYPE_U4: Il2CppTypeEnum = 9;
pub const Il2CppTypeEnum_IL2CPP_TYPE_I8: Il2CppTypeEnum = 10;
pub const Il2CppTypeEnum_IL2CPP_TYPE_U8: Il2CppTypeEnum = 11;
pub const Il2CppTypeEnum_IL2CPP_TYPE_R4: Il2CppTypeEnum = 12;
pub const Il2CppTypeEnum_IL2CPP_TYPE_R8: Il2CppTypeEnum = 13;
pub const Il2CppTypeEnum_IL2CPP_TYPE_STRING: Il2CppTypeEnum = 14;
pub const Il2CppTypeEnum_IL2CPP_TYPE_PTR: Il2CppTypeEnum = 15;
pub const Il2CppTypeEnum_IL2CPP_TYPE_BYREF: Il2CppTypeEnum = 16;
pub const Il2CppTypeEnum_IL2CPP_TYPE_VALUETYPE: Il2CppTypeEnum = 17;
pub const Il2CppTypeEnum_IL2CPP_TYPE_CLASS: Il2CppTypeEnum = 18;
pub const Il2CppTypeEnum_IL2CPP_TYPE_VAR: Il2CppTypeEnum = 19;
pub const Il2CppTypeEnum_IL2CPP_TYPE_ARRAY: Il2CppTypeEnum = 20;
pub const Il2CppTypeEnum_IL2CPP_TYPE_GENERICINST: Il2CppTypeEnum = 21;
pub const Il2CppTypeEnum_IL2CPP_TYPE_TYPEDBYREF: Il2CppTypeEnum = 22;
pub const Il2CppTypeEnum_IL2CPP_TYPE_I: Il2CppTypeEnum = 24;
pub const Il2CppTypeEnum_IL2CPP_TYPE_U: Il2CppTypeEnum = 25;
pub const Il2CppTypeEnum_IL2CPP_TYPE_FNPTR: Il2CppTypeEnum = 27;
pub const Il2CppTypeEnum_IL2CPP_TYPE_OBJECT: Il2CppTypeEnum = 28;
pub const Il2CppTypeEnum_IL2CPP_TYPE_SZARRAY: Il2CppTypeEnum = 29;
pub const Il2CppTypeEnum_IL2CPP_TYPE_MVAR: Il2CppTypeEnum = 30;
pub const Il2CppTypeEnum_IL2CPP_TYPE_CMOD_REQD: Il2CppTypeEnum = 31;
pub const Il2CppTypeEnum_IL2CPP_TYPE_CMOD_OPT: Il2CppTypeEnum = 32;
pub const Il2CppTypeEnum_IL2CPP_TYPE_INTERNAL: Il2CppTypeEnum = 33;
pub const Il2CppTypeEnum_IL2CPP_TYPE_MODIFIER: Il2CppTypeEnum = 64;
pub const Il2CppTypeEnum_IL2CPP_TYPE_SENTINEL: Il2CppTypeEnum = 65;
pub const Il2CppTypeEnum_IL2CPP_TYPE_PINNED: Il2CppTypeEnum = 69;
pub const Il2CppTypeEnum_IL2CPP_TYPE_ENUM: Il2CppTypeEnum = 85;
pub const Il2CppTypeEnum_IL2CPP_TYPE_IL2CPP_TYPE_INDEX: Il2CppTypeEnum = 255;

// ── Init / entry typedefs (plugin_api.rs:14-32) ─────────────────────────────

pub type HachimiGetApiFn = extern "C" fn(name: *const c_char) -> *mut c_void;
pub type HachimiInitV3Fn = extern "C" fn(get_api: HachimiGetApiFn, version: i32) -> InitResult;

pub type GuiMenuCallback = extern "C" fn(userdata: *mut c_void);
pub type GuiMenuSectionCallback = extern "C" fn(ui: *mut c_void, userdata: *mut c_void);
pub type GuiUiCallback = extern "C" fn(ui: *mut c_void, userdata: *mut c_void);
pub type GameInitializedCallback = unsafe extern "C" fn(userdata: *mut c_void);
pub type PresentCallback = unsafe extern "C" fn(swapchain: *mut c_void, userdata: *mut c_void);
/// Returns unit (not bool). Two window callbacks share one userdata.
pub type GuiWindowCallback = extern "C" fn(ui: *mut c_void, userdata: *mut c_void);

#[repr(i32)]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum InitResult {
    Error = 0,
    Ok = 1,
}

// ── Resolved get_api function pointer types ─────────────────────────────────
/// plugin_api.rs:42
pub type Fn_hachimi_instance = unsafe extern "C" fn() -> *const Hachimi;

/// plugin_api.rs:46
pub type Fn_hachimi_get_interceptor = unsafe extern "C" fn(this: *const Hachimi) -> *const Interceptor;

/// plugin_api.rs:50
pub type Fn_interceptor_hook =
    unsafe extern "C" fn(this: *const Interceptor, orig_addr: *mut c_void, hook_addr: *mut c_void) -> *mut c_void;

/// plugin_api.rs:58
pub type Fn_interceptor_hook_vtable = unsafe extern "C" fn(
    this: *const Interceptor,
    vtable: *mut *mut c_void,
    vtable_index: usize,
    hook_addr: *mut c_void,
) -> *mut c_void;

/// plugin_api.rs:66
pub type Fn_interceptor_get_trampoline_addr =
    unsafe extern "C" fn(this: *const Interceptor, hook_addr: *mut c_void) -> *mut c_void;

/// plugin_api.rs:70
pub type Fn_interceptor_unhook = unsafe extern "C" fn(this: *const Interceptor, hook_addr: *mut c_void) -> *mut c_void;

/// plugin_api.rs:79
pub type Fn_il2cpp_resolve_symbol = unsafe extern "C" fn(name: *const c_char) -> *mut c_void;

/// plugin_api.rs:86
pub type Fn_il2cpp_get_assembly_image = unsafe extern "C" fn(assembly_name: *const c_char) -> *const Il2CppImage;

/// plugin_api.rs:92
pub type Fn_il2cpp_get_class = unsafe extern "C" fn(
    image: *const Il2CppImage,
    namespace: *const c_char,
    class_name: *const c_char,
) -> *mut Il2CppClass;

/// plugin_api.rs:100
pub type Fn_il2cpp_get_method =
    unsafe extern "C" fn(class: *mut Il2CppClass, name: *const c_char, args_count: i32) -> *const MethodInfo;

/// plugin_api.rs:108
pub type Fn_il2cpp_get_method_overload = unsafe extern "C" fn(
    class: *mut Il2CppClass,
    name: *const c_char,
    params: *const Il2CppTypeEnum,
    param_count: usize,
) -> *const MethodInfo;

/// plugin_api.rs:118
pub type Fn_il2cpp_get_method_addr =
    unsafe extern "C" fn(class: *mut Il2CppClass, name: *const c_char, args_count: i32) -> *mut c_void;

/// plugin_api.rs:124
pub type Fn_il2cpp_get_method_overload_addr = unsafe extern "C" fn(
    class: *mut Il2CppClass,
    name: *const c_char,
    params: *const Il2CppTypeEnum,
    param_count: usize,
) -> *mut c_void;

/// plugin_api.rs:132
pub type Fn_il2cpp_get_method_cached =
    unsafe extern "C" fn(class: *mut Il2CppClass, name: *const c_char, args_count: i32) -> *const MethodInfo;

/// plugin_api.rs:140
pub type Fn_il2cpp_get_method_addr_cached =
    unsafe extern "C" fn(class: *mut Il2CppClass, name: *const c_char, args_count: i32) -> *mut c_void;

/// plugin_api.rs:146
pub type Fn_il2cpp_find_nested_class =
    unsafe extern "C" fn(class: *mut Il2CppClass, name: *const c_char) -> *mut Il2CppClass;

/// plugin_api.rs:154
pub type Fn_il2cpp_resolve_icall = unsafe extern "C" fn(name: *const c_char) -> Il2CppMethodPointer;

/// plugin_api.rs:158
pub type Fn_il2cpp_class_get_methods =
    unsafe extern "C" fn(klass: *mut Il2CppClass, iter: *mut *mut c_void) -> *const MethodInfo;

/// plugin_api.rs:162
pub type Fn_il2cpp_get_field_from_name =
    unsafe extern "C" fn(class: *mut Il2CppClass, name: *const c_char) -> *mut FieldInfo;

/// plugin_api.rs:168
pub type Fn_il2cpp_get_field_value =
    unsafe extern "C" fn(obj: *mut Il2CppObject, field: *mut FieldInfo, out_value: *mut c_void);

/// plugin_api.rs:174
pub type Fn_il2cpp_set_field_value =
    unsafe extern "C" fn(obj: *mut Il2CppObject, field: *mut FieldInfo, value: *const c_void);

/// plugin_api.rs:180
pub type Fn_il2cpp_get_static_field_value = unsafe extern "C" fn(field: *mut FieldInfo, out_value: *mut c_void);

/// plugin_api.rs:186
pub type Fn_il2cpp_set_static_field_value = unsafe extern "C" fn(field: *mut FieldInfo, value: *const c_void);

/// plugin_api.rs:192
pub type Fn_il2cpp_object_new = unsafe extern "C" fn(klass: *const Il2CppClass) -> *mut Il2CppObject;

/// plugin_api.rs:196
pub type Fn_il2cpp_runtime_object_init = unsafe extern "C" fn(object: *mut Il2CppObject);

/// plugin_api.rs:200
pub type Fn_il2cpp_string_new = unsafe extern "C" fn(text: *const c_char) -> *mut Il2CppString;

/// plugin_api.rs:204
pub type Fn_il2cpp_string_chars = unsafe extern "C" fn(s: *mut Il2CppString) -> *mut u16;

/// plugin_api.rs:211
pub type Fn_il2cpp_string_length = unsafe extern "C" fn(s: *mut Il2CppString) -> i32;

/// plugin_api.rs:218
pub type Fn_il2cpp_unbox = unsafe extern "C" fn(obj: *mut Il2CppObject) -> *mut c_void;

/// plugin_api.rs:222
pub type Fn_il2cpp_get_main_thread = unsafe extern "C" fn() -> *mut Il2CppThread;

/// plugin_api.rs:226
pub type Fn_il2cpp_get_attached_threads = unsafe extern "C" fn(out_size: *mut usize) -> *mut *mut Il2CppThread;

/// plugin_api.rs:230
pub type Fn_il2cpp_schedule_on_thread =
    unsafe extern "C" fn(thread: *mut Il2CppThread, callback: unsafe extern "C" fn());

/// plugin_api.rs:234
pub type Fn_il2cpp_create_array =
    unsafe extern "C" fn(element_type: *mut Il2CppClass, length: il2cpp_array_size_t) -> *mut Il2CppArray;

/// plugin_api.rs:240
pub type Fn_il2cpp_get_singleton_like_instance = unsafe extern "C" fn(class: *mut Il2CppClass) -> *mut Il2CppObject;

/// plugin_api.rs:246
pub type Fn_log = unsafe extern "C" fn(level: i32, target: *const c_char, message: *const c_char);

/// plugin_api.rs:261
pub type Fn_gui_register_menu_item =
    unsafe extern "C" fn(label: *const c_char, callback: Option<GuiMenuCallback>, userdata: *mut c_void) -> bool;

/// plugin_api.rs:276
pub type Fn_gui_register_menu_section =
    unsafe extern "C" fn(callback: Option<GuiMenuSectionCallback>, userdata: *mut c_void) -> bool;

/// plugin_api.rs:287
pub type Fn_hachimi_register_on_game_initialized =
    unsafe extern "C" fn(callback: Option<GameInitializedCallback>, userdata: *mut c_void) -> bool;

/// plugin_api.rs:301
pub type Fn_hachimi_register_present_callback =
    unsafe extern "C" fn(callback: Option<PresentCallback>, userdata: *mut c_void) -> bool;

/// plugin_api.rs:322
pub type Fn_gui_show_notification = unsafe extern "C" fn(message: *const c_char) -> bool;

/// plugin_api.rs:347
pub type Fn_gui_ui_heading = unsafe extern "C" fn(ui: *mut c_void, text: *const c_char) -> bool;

/// plugin_api.rs:353
pub type Fn_gui_ui_label = unsafe extern "C" fn(ui: *mut c_void, text: *const c_char) -> bool;

/// plugin_api.rs:359
pub type Fn_gui_ui_small = unsafe extern "C" fn(ui: *mut c_void, text: *const c_char) -> bool;

/// plugin_api.rs:365
pub type Fn_gui_ui_separator = unsafe extern "C" fn(ui: *mut c_void) -> bool;

/// plugin_api.rs:371
pub type Fn_gui_ui_button = unsafe extern "C" fn(ui: *mut c_void, text: *const c_char) -> bool;

/// plugin_api.rs:376
pub type Fn_gui_ui_small_button = unsafe extern "C" fn(ui: *mut c_void, text: *const c_char) -> bool;

/// plugin_api.rs:381
pub type Fn_gui_ui_checkbox = unsafe extern "C" fn(ui: *mut c_void, text: *const c_char, value: *mut bool) -> bool;

/// plugin_api.rs:396
pub type Fn_gui_ui_text_edit_singleline =
    unsafe extern "C" fn(ui: *mut c_void, buffer: *mut c_char, buffer_len: usize) -> bool;

/// plugin_api.rs:438
pub type Fn_gui_ui_horizontal =
    unsafe extern "C" fn(ui: *mut c_void, callback: Option<GuiUiCallback>, userdata: *mut c_void) -> bool;

/// plugin_api.rs:451
pub type Fn_gui_ui_grid = unsafe extern "C" fn(
    ui: *mut c_void,
    id: *const c_char,
    columns: usize,
    spacing_x: f32,
    spacing_y: f32,
    callback: Option<GuiUiCallback>,
    userdata: *mut c_void,
) -> bool;

/// plugin_api.rs:472
pub type Fn_gui_ui_end_row = unsafe extern "C" fn(ui: *mut c_void) -> bool;

/// plugin_api.rs:478
pub type Fn_gui_ui_colored_label =
    unsafe extern "C" fn(ui: *mut c_void, r: u8, g: u8, b: u8, a: u8, text: *const c_char) -> bool;

/// plugin_api.rs:491
pub type Fn_gui_ui_combo_menu = unsafe extern "C" fn(
    ui: *mut c_void,
    id: *const c_char,
    selected_index: *mut i32,
    items: *const *const c_char,
    item_count: usize,
    search_term: *mut c_char,
    search_term_len: usize,
) -> bool;

/// plugin_api.rs:641
pub type Fn_gui_register_menu_item_icon =
    unsafe extern "C" fn(label: *const c_char, icon_uri: *const c_char, icon_ptr: *const u8, icon_len: usize) -> bool;

/// plugin_api.rs:666
pub type Fn_gui_register_menu_section_with_icon = unsafe extern "C" fn(
    title: *const c_char,
    icon_uri: *const c_char,
    icon_ptr: *const u8,
    icon_len: usize,
    callback: Option<GuiMenuSectionCallback>,
    userdata: *mut c_void,
) -> bool;

/// plugin_api.rs:702
pub type Fn_gui_new_window_id = unsafe extern "C" fn() -> i32;

/// plugin_api.rs:706
pub type Fn_gui_show_window = unsafe extern "C" fn(
    id: i32,
    title: *const c_char,
    contents_callback: Option<GuiWindowCallback>,
    bottom_callback: Option<GuiWindowCallback>,
    userdata: *mut c_void,
) -> bool;

/// plugin_api.rs:726
pub type Fn_gui_close_window = unsafe extern "C" fn(id: i32);

/// plugin_api.rs:731
pub type Fn_android_dex_load =
    unsafe extern "C" fn(dex_ptr: *const u8, dex_len: usize, class_name: *const c_char) -> u64;

/// plugin_api.rs:741
pub type Fn_android_dex_unload = unsafe extern "C" fn(handle: u64) -> bool;

/// plugin_api.rs:751
pub type Fn_android_dex_call_static_noargs =
    unsafe extern "C" fn(handle: u64, method: *const c_char, sig: *const c_char) -> bool;

/// plugin_api.rs:763
pub type Fn_android_dex_call_static_string =
    unsafe extern "C" fn(handle: u64, method: *const c_char, sig: *const c_char, arg: *const c_char) -> bool;

/// plugin_api.rs:775
pub type Fn_gui_get_menu_width = unsafe extern "C" fn() -> f32;

/// plugin_api.rs:779
pub type Fn_gui_set_menu_width = unsafe extern "C" fn(width: f32);

/// plugin_api.rs:783
pub type Fn_hachimi_get_base_dir = unsafe extern "C" fn() -> *const c_char;

/// plugin_api.rs:791
pub type Fn_hachimi_get_data_path = unsafe extern "C" fn() -> *const c_char;
