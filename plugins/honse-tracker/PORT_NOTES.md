# Port notes — `plugins/honse-tracker`

Living audit trail for the plan-3 port. Finalized in t-005.

## Files moved (t-001)

- 67 `.rs` files from fork `training_tracker/` → `plugins/honse-tracker/src/` (`mod.rs` → `lib.rs`)
- Assets → `plugins/honse-tracker/assets/` (`course_params.json`, `skill_grades.json`, `icons/`)
- Path rename: `crate::core::modules::training_tracker::` → `crate::`
- `include_bytes!` paths adjusted to `../../assets/icons/…`
- Test asset paths: `CARGO_MANIFEST_DIR/assets/…`

## Compat method → provider (t-002)

Provider codes: `1` = edge-sdk, `2` = honse-services, `3` = local/shim (egui re-export, always-true caps).

| method | provider |
|---|---|
| `ui_from_ptr` | 1 |
| `get` | 3 (local singleton; edge `try_get` gated on Api init) |
| `try_get` | 3 |
| `version` | 1 (returns edge API 3 wrapped in ApiVersion) |
| `has_capability` | 3 (always true) |
| `resolve_symbol` | 1 |
| `dlsym` | 1 |
| `get_assembly_image` | 1 |
| `get_class` | 1 |
| `get_method` | 1 |
| `get_method_addr` | 1 |
| `find_nested_class` | 1 |
| `get_field_from_name` | 1 |
| `get_field_value` | 1 |
| `get_singleton` | 1 |
| `class_get_methods` | 1 |
| `schedule_on_main_thread` | 1 |
| `free_il2cpp_string` | 1 |
| `hook` | 1 |
| `unhook` | 1 |
| `on` | 2 |
| `off` | 2 |
| `register_page` | 2 (title forced to `"Training Tracker"` — services require a title) |
| `register_tab` | 2 (title `"Tracker"`; C-callback trampoline over Rust closure) |
| `register_page_with_icon` | 2 |
| `register_menu_section` | 1 |
| `register_panel` | 2 |
| `register_overlay` | 2 |
| `register_panel_chromeless` | 2 |
| `register_panel_chromeless_fixed` | 2 |
| `set_overlay_visible` | 2 |
| `overlay_set_visible` | 2 |
| `overlay_visible` | 2 |
| `toggle_overlay` | 2 |
| `register_hotkey` | 2 |
| `unregister` | 2 (hotkeys + surface) |
| `show_notification` | 1 |
| `host_data_path` | 1 (`Sdk::data_path`) |
| `gametora_data_dir` | 2 |
| `view_name` | 2 |

`hlog_*` macros → `log::*` (edge-sdk installs log adapter). Chosen path documented here.

## Other intentional deviations (t-002+)

- Local `crate::il2cpp` shim for skill-shop purchase (`object_new`, `set_field_value`, `Array`, `Thread::schedule` → edge schedule_on_main_thread, `create_delegate`). Opaque FFI types → byte-offset layout (`K_IL2CPP_SIZE_OF_ARRAY=32`, delegate method_ptr @16 / invoke_impl @24).
- `set_overlay_visible_if_unset` added to `honse_services::surface` (fork host helper).
- `compat::{event, capability, overlay_flags}` local modules replace `hachimi_plugin_abi` imports (hiker `fork_abi_import`).

## Fixtures (t-002)

- Copied fork `veterans/*.json` → `plugins/honse-tracker/veterans/` and updated `evaluation::tests::validated_runners_match_exactly` path from `{manifest}/../../veterans` to `{manifest}/veterans` (layout adaptation only).
