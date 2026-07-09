//! Resolved `HachimiGetApiFn` function pointers for every edge `get_api` name.

use std::ffi::CString;

use once_cell::sync::OnceCell;

use crate::ffi::{self, HachimiGetApiFn};

static API: OnceCell<Api> = OnceCell::new();

/// All 66 edge `get_api` entry points, resolved once at plugin init.
pub struct Api {
    pub hachimi_instance: Option<ffi::Fn_hachimi_instance>,
    pub hachimi_get_interceptor: Option<ffi::Fn_hachimi_get_interceptor>,
    pub interceptor_hook: Option<ffi::Fn_interceptor_hook>,
    pub interceptor_hook_vtable: Option<ffi::Fn_interceptor_hook_vtable>,
    pub interceptor_get_trampoline_addr: Option<ffi::Fn_interceptor_get_trampoline_addr>,
    pub interceptor_unhook: Option<ffi::Fn_interceptor_unhook>,
    pub il2cpp_resolve_symbol: Option<ffi::Fn_il2cpp_resolve_symbol>,
    pub il2cpp_get_assembly_image: Option<ffi::Fn_il2cpp_get_assembly_image>,
    pub il2cpp_get_class: Option<ffi::Fn_il2cpp_get_class>,
    pub il2cpp_get_method: Option<ffi::Fn_il2cpp_get_method>,
    pub il2cpp_get_method_overload: Option<ffi::Fn_il2cpp_get_method_overload>,
    pub il2cpp_get_method_addr: Option<ffi::Fn_il2cpp_get_method_addr>,
    pub il2cpp_get_method_overload_addr: Option<ffi::Fn_il2cpp_get_method_overload_addr>,
    pub il2cpp_get_method_cached: Option<ffi::Fn_il2cpp_get_method_cached>,
    pub il2cpp_get_method_addr_cached: Option<ffi::Fn_il2cpp_get_method_addr_cached>,
    pub il2cpp_find_nested_class: Option<ffi::Fn_il2cpp_find_nested_class>,
    pub il2cpp_resolve_icall: Option<ffi::Fn_il2cpp_resolve_icall>,
    pub il2cpp_class_get_methods: Option<ffi::Fn_il2cpp_class_get_methods>,
    pub il2cpp_get_field_from_name: Option<ffi::Fn_il2cpp_get_field_from_name>,
    pub il2cpp_get_field_value: Option<ffi::Fn_il2cpp_get_field_value>,
    pub il2cpp_set_field_value: Option<ffi::Fn_il2cpp_set_field_value>,
    pub il2cpp_get_static_field_value: Option<ffi::Fn_il2cpp_get_static_field_value>,
    pub il2cpp_set_static_field_value: Option<ffi::Fn_il2cpp_set_static_field_value>,
    pub il2cpp_object_new: Option<ffi::Fn_il2cpp_object_new>,
    pub il2cpp_unbox: Option<ffi::Fn_il2cpp_unbox>,
    pub il2cpp_get_main_thread: Option<ffi::Fn_il2cpp_get_main_thread>,
    pub il2cpp_get_attached_threads: Option<ffi::Fn_il2cpp_get_attached_threads>,
    pub il2cpp_schedule_on_thread: Option<ffi::Fn_il2cpp_schedule_on_thread>,
    pub il2cpp_create_array: Option<ffi::Fn_il2cpp_create_array>,
    pub il2cpp_get_singleton_like_instance: Option<ffi::Fn_il2cpp_get_singleton_like_instance>,
    pub log: Option<ffi::Fn_log>,
    pub gui_register_menu_item: Option<ffi::Fn_gui_register_menu_item>,
    pub gui_register_menu_section: Option<ffi::Fn_gui_register_menu_section>,
    pub gui_show_notification: Option<ffi::Fn_gui_show_notification>,
    pub gui_ui_heading: Option<ffi::Fn_gui_ui_heading>,
    pub gui_ui_label: Option<ffi::Fn_gui_ui_label>,
    pub gui_ui_small: Option<ffi::Fn_gui_ui_small>,
    pub gui_ui_separator: Option<ffi::Fn_gui_ui_separator>,
    pub gui_ui_button: Option<ffi::Fn_gui_ui_button>,
    pub gui_ui_small_button: Option<ffi::Fn_gui_ui_small_button>,
    pub gui_ui_checkbox: Option<ffi::Fn_gui_ui_checkbox>,
    pub gui_ui_text_edit_singleline: Option<ffi::Fn_gui_ui_text_edit_singleline>,
    pub gui_ui_horizontal: Option<ffi::Fn_gui_ui_horizontal>,
    pub gui_ui_grid: Option<ffi::Fn_gui_ui_grid>,
    pub gui_ui_end_row: Option<ffi::Fn_gui_ui_end_row>,
    pub gui_ui_colored_label: Option<ffi::Fn_gui_ui_colored_label>,
    pub gui_register_menu_item_icon: Option<ffi::Fn_gui_register_menu_item_icon>,
    pub gui_register_menu_section_with_icon: Option<ffi::Fn_gui_register_menu_section_with_icon>,
    pub gui_new_window_id: Option<ffi::Fn_gui_new_window_id>,
    pub gui_show_window: Option<ffi::Fn_gui_show_window>,
    pub gui_close_window: Option<ffi::Fn_gui_close_window>,
    pub android_dex_load: Option<ffi::Fn_android_dex_load>,
    pub android_dex_unload: Option<ffi::Fn_android_dex_unload>,
    pub android_dex_call_static_noargs: Option<ffi::Fn_android_dex_call_static_noargs>,
    pub android_dex_call_static_string: Option<ffi::Fn_android_dex_call_static_string>,
    pub il2cpp_runtime_object_init: Option<ffi::Fn_il2cpp_runtime_object_init>,
    pub il2cpp_string_new: Option<ffi::Fn_il2cpp_string_new>,
    pub il2cpp_string_chars: Option<ffi::Fn_il2cpp_string_chars>,
    pub il2cpp_string_length: Option<ffi::Fn_il2cpp_string_length>,
    pub gui_ui_combo_menu: Option<ffi::Fn_gui_ui_combo_menu>,
    pub hachimi_register_on_game_initialized: Option<ffi::Fn_hachimi_register_on_game_initialized>,
    pub hachimi_register_present_callback: Option<ffi::Fn_hachimi_register_present_callback>,
    pub gui_get_menu_width: Option<ffi::Fn_gui_get_menu_width>,
    pub gui_set_menu_width: Option<ffi::Fn_gui_set_menu_width>,
    pub hachimi_get_base_dir: Option<ffi::Fn_hachimi_get_base_dir>,
    pub hachimi_get_data_path: Option<ffi::Fn_hachimi_get_data_path>,
}

impl Api {
    /// Resolve every API name via `get_api` and store in the process-wide `OnceCell`.
    pub fn init(get_api: HachimiGetApiFn) {
        let api = Api {
            hachimi_instance: resolve(get_api, "hachimi_instance"),
            hachimi_get_interceptor: resolve(get_api, "hachimi_get_interceptor"),
            interceptor_hook: resolve(get_api, "interceptor_hook"),
            interceptor_hook_vtable: resolve(get_api, "interceptor_hook_vtable"),
            interceptor_get_trampoline_addr: resolve(get_api, "interceptor_get_trampoline_addr"),
            interceptor_unhook: resolve(get_api, "interceptor_unhook"),
            il2cpp_resolve_symbol: resolve(get_api, "il2cpp_resolve_symbol"),
            il2cpp_get_assembly_image: resolve(get_api, "il2cpp_get_assembly_image"),
            il2cpp_get_class: resolve(get_api, "il2cpp_get_class"),
            il2cpp_get_method: resolve(get_api, "il2cpp_get_method"),
            il2cpp_get_method_overload: resolve(get_api, "il2cpp_get_method_overload"),
            il2cpp_get_method_addr: resolve(get_api, "il2cpp_get_method_addr"),
            il2cpp_get_method_overload_addr: resolve(get_api, "il2cpp_get_method_overload_addr"),
            il2cpp_get_method_cached: resolve(get_api, "il2cpp_get_method_cached"),
            il2cpp_get_method_addr_cached: resolve(get_api, "il2cpp_get_method_addr_cached"),
            il2cpp_find_nested_class: resolve(get_api, "il2cpp_find_nested_class"),
            il2cpp_resolve_icall: resolve(get_api, "il2cpp_resolve_icall"),
            il2cpp_class_get_methods: resolve(get_api, "il2cpp_class_get_methods"),
            il2cpp_get_field_from_name: resolve(get_api, "il2cpp_get_field_from_name"),
            il2cpp_get_field_value: resolve(get_api, "il2cpp_get_field_value"),
            il2cpp_set_field_value: resolve(get_api, "il2cpp_set_field_value"),
            il2cpp_get_static_field_value: resolve(get_api, "il2cpp_get_static_field_value"),
            il2cpp_set_static_field_value: resolve(get_api, "il2cpp_set_static_field_value"),
            il2cpp_object_new: resolve(get_api, "il2cpp_object_new"),
            il2cpp_unbox: resolve(get_api, "il2cpp_unbox"),
            il2cpp_get_main_thread: resolve(get_api, "il2cpp_get_main_thread"),
            il2cpp_get_attached_threads: resolve(get_api, "il2cpp_get_attached_threads"),
            il2cpp_schedule_on_thread: resolve(get_api, "il2cpp_schedule_on_thread"),
            il2cpp_create_array: resolve(get_api, "il2cpp_create_array"),
            il2cpp_get_singleton_like_instance: resolve(get_api, "il2cpp_get_singleton_like_instance"),
            log: resolve(get_api, "log"),
            gui_register_menu_item: resolve(get_api, "gui_register_menu_item"),
            gui_register_menu_section: resolve(get_api, "gui_register_menu_section"),
            gui_show_notification: resolve(get_api, "gui_show_notification"),
            gui_ui_heading: resolve(get_api, "gui_ui_heading"),
            gui_ui_label: resolve(get_api, "gui_ui_label"),
            gui_ui_small: resolve(get_api, "gui_ui_small"),
            gui_ui_separator: resolve(get_api, "gui_ui_separator"),
            gui_ui_button: resolve(get_api, "gui_ui_button"),
            gui_ui_small_button: resolve(get_api, "gui_ui_small_button"),
            gui_ui_checkbox: resolve(get_api, "gui_ui_checkbox"),
            gui_ui_text_edit_singleline: resolve(get_api, "gui_ui_text_edit_singleline"),
            gui_ui_horizontal: resolve(get_api, "gui_ui_horizontal"),
            gui_ui_grid: resolve(get_api, "gui_ui_grid"),
            gui_ui_end_row: resolve(get_api, "gui_ui_end_row"),
            gui_ui_colored_label: resolve(get_api, "gui_ui_colored_label"),
            gui_register_menu_item_icon: resolve(get_api, "gui_register_menu_item_icon"),
            gui_register_menu_section_with_icon: resolve(get_api, "gui_register_menu_section_with_icon"),
            gui_new_window_id: resolve(get_api, "gui_new_window_id"),
            gui_show_window: resolve(get_api, "gui_show_window"),
            gui_close_window: resolve(get_api, "gui_close_window"),
            android_dex_load: resolve(get_api, "android_dex_load"),
            android_dex_unload: resolve(get_api, "android_dex_unload"),
            android_dex_call_static_noargs: resolve(get_api, "android_dex_call_static_noargs"),
            android_dex_call_static_string: resolve(get_api, "android_dex_call_static_string"),
            il2cpp_runtime_object_init: resolve(get_api, "il2cpp_runtime_object_init"),
            il2cpp_string_new: resolve(get_api, "il2cpp_string_new"),
            il2cpp_string_chars: resolve(get_api, "il2cpp_string_chars"),
            il2cpp_string_length: resolve(get_api, "il2cpp_string_length"),
            gui_ui_combo_menu: resolve(get_api, "gui_ui_combo_menu"),
            hachimi_register_on_game_initialized: resolve(get_api, "hachimi_register_on_game_initialized"),
            hachimi_register_present_callback: resolve(get_api, "hachimi_register_present_callback"),
            gui_get_menu_width: resolve(get_api, "gui_get_menu_width"),
            gui_set_menu_width: resolve(get_api, "gui_set_menu_width"),
            hachimi_get_base_dir: resolve(get_api, "hachimi_get_base_dir"),
            hachimi_get_data_path: resolve(get_api, "hachimi_get_data_path"),
        };
        if API.set(api).is_err() {
            // Already initialized — ignore (idempotent for tests / double-load).
        }
    }

    /// Panic if `Api::init` has not been called.
    pub fn get() -> &'static Api {
        API.get()
            .expect("edge-sdk Api not initialized; call Api::init from hachimi_init_v3 first")
    }

    /// Returns `None` before `Api::init`.
    pub fn try_get() -> Option<&'static Api> {
        API.get()
    }
}

fn resolve<T>(get_api: HachimiGetApiFn, name: &str) -> Option<T> {
    let c_name = CString::new(name).ok()?;
    let ptr = get_api(c_name.as_ptr());
    if ptr.is_null() {
        return None;
    }
    // `T` is always a function-pointer type (same size as `*mut c_void`).
    debug_assert_eq!(std::mem::size_of::<T>(), std::mem::size_of::<*mut std::ffi::c_void>());
    // SAFETY: edge's hachimi_get_api returns a function pointer of the type
    // documented for `name` in plugin_api.rs. We cast to that exact Fn_* type.
    let typed = unsafe { std::mem::transmute_copy(&ptr) };
    Some(typed)
}
