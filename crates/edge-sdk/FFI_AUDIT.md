# FFI Audit — edge-sdk transcription of hachimi-edge `get_api`

Source of truth: `/Users/jalbarran/fun/drekki/hachimi-edge/src/core/plugin_api.rs`.
Each row was filled by reading the cited `unsafe extern "C" fn` definition, not from memory.

| api name | plugin_api.rs line | ffi.rs line | param-count | return type |
|---|---:|---:|---:|---|
| `hachimi_instance` | 42 | 138 | 0 | `*const Hachimi` |
| `hachimi_get_interceptor` | 46 | 141 | 1 | `*const Interceptor` |
| `interceptor_hook` | 50 | 144 | 3 | `*mut c_void` |
| `interceptor_hook_vtable` | 58 | 148 | 4 | `*mut c_void` |
| `interceptor_get_trampoline_addr` | 66 | 156 | 2 | `*mut c_void` |
| `interceptor_unhook` | 70 | 160 | 2 | `*mut c_void` |
| `il2cpp_resolve_symbol` | 79 | 163 | 1 | `*mut c_void` |
| `il2cpp_get_assembly_image` | 86 | 166 | 1 | `*const Il2CppImage` |
| `il2cpp_get_class` | 92 | 169 | 3 | `*mut Il2CppClass` |
| `il2cpp_get_method` | 100 | 176 | 3 | `*const MethodInfo` |
| `il2cpp_get_method_overload` | 108 | 180 | 4 | `*const MethodInfo` |
| `il2cpp_get_method_addr` | 118 | 188 | 3 | `*mut c_void` |
| `il2cpp_get_method_overload_addr` | 124 | 192 | 4 | `*mut c_void` |
| `il2cpp_get_method_cached` | 132 | 200 | 3 | `*const MethodInfo` |
| `il2cpp_get_method_addr_cached` | 140 | 204 | 3 | `*mut c_void` |
| `il2cpp_find_nested_class` | 146 | 208 | 2 | `*mut Il2CppClass` |
| `il2cpp_resolve_icall` | 154 | 212 | 1 | `Il2CppMethodPointer` |
| `il2cpp_class_get_methods` | 158 | 215 | 2 | `*const MethodInfo` |
| `il2cpp_get_field_from_name` | 162 | 219 | 2 | `*mut FieldInfo` |
| `il2cpp_get_field_value` | 168 | 223 | 3 | `()` |
| `il2cpp_set_field_value` | 174 | 227 | 3 | `()` |
| `il2cpp_get_static_field_value` | 180 | 231 | 2 | `()` |
| `il2cpp_set_static_field_value` | 186 | 234 | 2 | `()` |
| `il2cpp_object_new` | 192 | 237 | 1 | `*mut Il2CppObject` |
| `il2cpp_unbox` | 218 | 252 | 1 | `*mut c_void` |
| `il2cpp_get_main_thread` | 222 | 255 | 0 | `*mut Il2CppThread` |
| `il2cpp_get_attached_threads` | 226 | 258 | 1 | `*mut *mut Il2CppThread` |
| `il2cpp_schedule_on_thread` | 230 | 261 | 2 | `()` |
| `il2cpp_create_array` | 234 | 265 | 2 | `*mut Il2CppArray` |
| `il2cpp_get_singleton_like_instance` | 240 | 269 | 1 | `*mut Il2CppObject` |
| `log` | 246 | 272 | 3 | `()` |
| `gui_register_menu_item` | 261 | 275 | 3 | `bool` |
| `gui_register_menu_section` | 276 | 279 | 2 | `bool` |
| `gui_show_notification` | 322 | 291 | 1 | `bool` |
| `gui_ui_heading` | 347 | 294 | 2 | `bool` |
| `gui_ui_label` | 353 | 297 | 2 | `bool` |
| `gui_ui_small` | 359 | 300 | 2 | `bool` |
| `gui_ui_separator` | 365 | 303 | 1 | `bool` |
| `gui_ui_button` | 371 | 306 | 2 | `bool` |
| `gui_ui_small_button` | 376 | 309 | 2 | `bool` |
| `gui_ui_checkbox` | 381 | 312 | 3 | `bool` |
| `gui_ui_text_edit_singleline` | 396 | 315 | 3 | `bool` |
| `gui_ui_horizontal` | 438 | 319 | 3 | `bool` |
| `gui_ui_grid` | 451 | 323 | 7 | `bool` |
| `gui_ui_end_row` | 472 | 334 | 1 | `bool` |
| `gui_ui_colored_label` | 478 | 337 | 6 | `bool` |
| `gui_register_menu_item_icon` | 641 | 352 | 4 | `bool` |
| `gui_register_menu_section_with_icon` | 666 | 356 | 6 | `bool` |
| `gui_new_window_id` | 702 | 366 | 0 | `i32` |
| `gui_show_window` | 706 | 369 | 5 | `bool` |
| `gui_close_window` | 726 | 378 | 1 | `()` |
| `android_dex_load` | 731 | 381 | 3 | `u64` |
| `android_dex_unload` | 741 | 385 | 1 | `bool` |
| `android_dex_call_static_noargs` | 751 | 388 | 3 | `bool` |
| `android_dex_call_static_string` | 763 | 392 | 4 | `bool` |
| `il2cpp_runtime_object_init` | 196 | 240 | 1 | `()` |
| `il2cpp_string_new` | 200 | 243 | 1 | `*mut Il2CppString` |
| `il2cpp_string_chars` | 204 | 246 | 1 | `*mut u16` |
| `il2cpp_string_length` | 211 | 249 | 1 | `i32` |
| `gui_ui_combo_menu` | 491 | 341 | 7 | `bool` |
| `hachimi_register_on_game_initialized` | 287 | 283 | 2 | `bool` |
| `hachimi_register_present_callback` | 301 | 287 | 2 | `bool` |
| `gui_get_menu_width` | 775 | 396 | 0 | `f32` |
| `gui_set_menu_width` | 779 | 399 | 1 | `()` |
| `hachimi_get_base_dir` | 783 | 402 | 0 | `*const c_char` |
| `hachimi_get_data_path` | 791 | 405 | 0 | `*const c_char` |

## Counts

- `get_api` match arms: 66
- `Api` fields: 66
- FFI_AUDIT rows: 66

## Notes

- `Il2CppTypeEnum` in edge is a `c_uint` typedef + consts (`types.rs:425`), not a Rust `enum`. Transcribed as the typedef + consts.
- Android dex fns use the full (android) signatures; Windows stubs share the same ABI.
- `GuiWindowCallback` returns unit; `gui_show_window` takes five params (id, title, contents, bottom, userdata).
