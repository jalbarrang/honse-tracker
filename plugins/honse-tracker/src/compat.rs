//! Compatibility shim for the training-tracker plugin.
//!
//! Re-implements the SDK surface the tracker was authored against, delegating to
//! `edge_sdk::Sdk` (IL2CPP / hooking / notifications / menu sections / data_path)
//! and `honse_services` (events, overlays/panels/tabs, hotkeys, gametora, view_name).
//!
//! Public surface matches the fork `training_tracker/compat.rs` (`rg -n 'pub (unsafe )?fn'`).
//! Method → provider mapping is recorded in `PORT_NOTES.md` for hiker facts (t-005).

use std::ffi::{c_char, c_void, CStr};

use edge_sdk::ffi::{GuiMenuCallback, GuiMenuSectionCallback};
use edge_sdk::Sdk as EdgeSdk;

// ── Re-exports so moved tracker files keep their `crate::compat::…` imports. ──
pub use ::egui;

// Fork ABI used `type Il2CppClass = c_void` (and friends). Keep that so moved
// tracker code that casts through `*mut c_void` compiles without churn. Casts
// to/from edge-sdk's opaque FFI structs happen only inside Sdk methods.
pub type Il2CppClass = c_void;
pub type Il2CppImage = c_void;
pub type Il2CppObject = c_void;
pub type MethodInfo = c_void;
pub type FieldInfo = c_void;

/// Host→plugin event ids (fork `hachimi_plugin_abi::event`).
pub mod event {
    pub use honse_services::{FRAME, SHUTDOWN, VIEW_CHANGE};
    /// Fired after the host reloads its config. `data` is null. (fork id; unused here)
    pub const CONFIG_RELOAD: u32 = 2;
    /// Fired when a Single Mode (career) run becomes active. `data` is null.
    pub const CAREER_START: u32 = 5;
    /// Fired when a Single Mode (career) run ends. `data` is null.
    pub const CAREER_END: u32 = 6;
    /// Fired when the player submits a training command. Dropped in this port (t-003).
    pub const TRAINING_COMMAND: u32 = 7;
    /// Fired once when the splash screen is first shown. `data` is null.
    pub const SPLASH_SHOWN: u32 = 8;
}

/// Host capability bitflags (fork `hachimi_plugin_abi::capability`).
/// Single-version world: [`Sdk::has_capability`] always returns true.
pub mod capability {
    pub const GUI: u64 = 1 << 0;
    pub const OVERLAY: u64 = 1 << 1;
    pub const EVENTS: u64 = 1 << 2;
    pub const IL2CPP: u64 = 1 << 3;
    pub const DATA_PATHS: u64 = 1 << 4;
}

/// Overlay presentation flags (fork `hachimi_plugin_abi::overlay_flags`).
pub mod overlay_flags {
    pub use honse_services::surface::overlay_flags::*;
}

/// Event callback shape (fork `PluginEventFn`).
pub type PluginEventFn = honse_services::EventFn;

/// View-change payload (fork ABI).
pub use honse_services::ViewChangeEvent;

// ── Logging: forward to `log` (edge-sdk installs a log adapter). ──

#[allow(unused_macros)]
macro_rules! hlog_info {
    (target: $target:literal, $($arg:tt)*) => { ::log::info!(target: $target, $($arg)*) };
    ($($arg:tt)*) => { ::log::info!($($arg)*) };
}
macro_rules! hlog_warn {
    (target: $target:literal, $($arg:tt)*) => { ::log::warn!(target: $target, $($arg)*) };
    ($($arg:tt)*) => { ::log::warn!($($arg)*) };
}
macro_rules! hlog_error {
    (target: $target:literal, $($arg:tt)*) => { ::log::error!(target: $target, $($arg)*) };
    ($($arg:tt)*) => { ::log::error!($($arg)*) };
}
#[allow(unused_macros)]
macro_rules! hlog_debug {
    (target: $target:literal, $($arg:tt)*) => { ::log::debug!(target: $target, $($arg)*) };
    ($($arg:tt)*) => { ::log::debug!($($arg)*) };
}
#[allow(unused_macros)]
macro_rules! hlog_trace {
    (target: $target:literal, $($arg:tt)*) => { ::log::trace!(target: $target, $($arg)*) };
    ($($arg:tt)*) => { ::log::trace!($($arg)*) };
}

/// Host API version. Single-version world: `at_least` is always true.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct ApiVersion(i32);

impl ApiVersion {
    #[must_use]
    pub const fn new(v: i32) -> Self {
        Self(v)
    }
    #[must_use]
    pub const fn raw(self) -> i32 {
        self.0
    }
    #[must_use]
    pub const fn at_least(self, _min: i32) -> bool {
        true
    }
}

/// Cast a host-provided `Ui` pointer to a real [`egui::Ui`].
///
/// # Safety
/// `ptr` must be the live `*mut egui::Ui` the host passed into the callback.
#[must_use]
pub unsafe fn ui_from_ptr<'a>(ptr: *mut c_void) -> &'a mut egui::Ui {
    // SAFETY: same contract as edge_sdk::ui_from_ptr.
    unsafe { edge_sdk::ui_from_ptr(ptr) }
}

/// Stateless façade over edge-sdk + honse-services. Obtained via [`Sdk::get`].
#[derive(Clone, Copy)]
pub struct Sdk;

static SDK: Sdk = Sdk;

impl Sdk {
    #[must_use]
    pub fn get() -> &'static Sdk {
        &SDK
    }
    #[must_use]
    pub fn try_get() -> Option<&'static Sdk> {
        Some(&SDK)
    }
    #[must_use]
    pub fn version(&self) -> ApiVersion {
        ApiVersion::new(3) // edge API VERSION; single-version world
    }
    #[must_use]
    pub fn has_capability(&self, _cap: u64) -> bool {
        true
    }

    // ── IL2CPP (edge-sdk) ──
    // Casts: fork ABI types are `c_void` aliases; edge-sdk uses opaque ZST structs.
    // Pointer addresses are identical — only the Rust type name differs.

    #[must_use]
    pub fn resolve_symbol(&self, name: &str) -> Option<*mut c_void> {
        EdgeSdk::get().resolve_symbol(name)
    }

    #[must_use]
    pub fn dlsym(&self, name: &str) -> Option<*mut c_void> {
        EdgeSdk::get().dlsym(name)
    }

    #[must_use]
    pub fn get_assembly_image(&self, assembly: &str) -> Option<*const Il2CppImage> {
        EdgeSdk::get().get_assembly_image(assembly).map(|p| p.cast())
    }

    #[must_use]
    pub fn get_class(&self, image: *const Il2CppImage, namespace: &str, class_name: &str) -> Option<*mut Il2CppClass> {
        EdgeSdk::get()
            .get_class(image.cast(), namespace, class_name)
            .map(|p| p.cast())
    }

    #[must_use]
    pub fn get_method(&self, class: *mut Il2CppClass, name: &str, args_count: i32) -> Option<*const MethodInfo> {
        EdgeSdk::get()
            .get_method(class.cast(), name, args_count)
            .map(|p| p.cast())
    }

    #[must_use]
    pub fn get_method_addr(&self, class: *mut Il2CppClass, name: &str, args_count: i32) -> Option<*mut c_void> {
        EdgeSdk::get().get_method_addr(class.cast(), name, args_count)
    }

    #[must_use]
    pub fn find_nested_class(&self, parent: *mut Il2CppClass, name: &str) -> Option<*mut Il2CppClass> {
        EdgeSdk::get()
            .find_nested_class(parent.cast(), name)
            .map(|p| p.cast())
    }

    #[must_use]
    pub fn get_field_from_name(&self, class: *mut Il2CppClass, name: &str) -> Option<*mut FieldInfo> {
        EdgeSdk::get()
            .get_field_from_name(class.cast(), name)
            .map(|p| p.cast())
    }

    /// Read a field value into `out_value`.
    ///
    /// # Safety
    /// `obj`, `field`, and `out_value` must be valid IL2CPP pointers with a size
    /// matching the field type.
    pub unsafe fn get_field_value(&self, obj: *mut Il2CppObject, field: *mut FieldInfo, out_value: *mut c_void) {
        // SAFETY: caller guarantees valid IL2CPP pointers; cast is address-preserving.
        unsafe { EdgeSdk::get().get_field_value(obj.cast(), field.cast(), out_value) };
    }

    #[must_use]
    pub fn get_singleton(&self, class: *mut Il2CppClass) -> Option<*mut Il2CppObject> {
        EdgeSdk::get().get_singleton(class.cast()).map(|p| p.cast())
    }

    #[must_use]
    pub fn class_get_methods(&self, klass: *mut Il2CppClass, iter: *mut *mut c_void) -> *const MethodInfo {
        EdgeSdk::get().class_get_methods(klass.cast(), iter).cast()
    }

    pub fn schedule_on_main_thread(&self, callback: unsafe extern "C" fn()) {
        EdgeSdk::get().schedule_on_main_thread(callback);
    }

    pub fn free_il2cpp_string(&self, ptr: *mut c_char) {
        EdgeSdk::get().free_il2cpp_string(ptr);
    }

    // ── Hooking (edge-sdk) ──

    #[must_use]
    pub fn hook(&self, orig_addr: *mut c_void, hook_addr: *mut c_void) -> Option<*mut c_void> {
        EdgeSdk::get().hook(orig_addr, hook_addr)
    }

    pub fn unhook(&self, hook_addr: *mut c_void) -> Option<*mut c_void> {
        EdgeSdk::get().unhook(hook_addr)
    }

    // ── Events (honse-services) ──

    pub fn on(&self, event_id: u32, callback: PluginEventFn, userdata: *mut c_void) -> u64 {
        honse_services::on(event_id, callback, userdata)
    }

    pub fn off(&self, handle: u64) {
        honse_services::off(handle);
    }

    // ── GUI registration ──

    pub fn register_page(&self, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        // Services require a title; fork page had none. Fixed title — see PORT_NOTES.
        honse_services::register_page("Training Tracker", callback, userdata)
    }

    pub fn register_tab<F>(&self, draw: F) -> u64
    where
        F: Fn(&mut egui::Ui) + Send + Sync + 'static,
    {
        type DrawFn = Box<dyn Fn(&mut egui::Ui) + Send + Sync>;
        // Box the fat pointer so userdata is a thin `*mut c_void`.
        let heap: *mut DrawFn = Box::into_raw(Box::new(Box::new(draw) as DrawFn));
        extern "C" fn trampoline(ui: *mut c_void, userdata: *mut c_void) {
            // SAFETY: userdata is the heap Box leaked in register_tab; ui is host-live.
            let draw = unsafe { &*(userdata as *const DrawFn) };
            let ui = unsafe { ui_from_ptr(ui) };
            draw(ui);
        }
        honse_services::register_tab("Tracker", trampoline, heap as *mut c_void)
    }

    pub fn register_page_with_icon(
        &self,
        title: &str,
        icon_uri: &str,
        icon_bytes: &[u8],
        callback: GuiMenuSectionCallback,
        userdata: *mut c_void,
    ) -> u64 {
        // Services take a Rust closure; wrap the C callback.
        let cb = callback;
        let ud = userdata as usize;
        let ok = honse_services::register_page_with_icon(
            title,
            Some(icon_uri),
            icon_bytes,
            move |ui| {
                cb(ui as *mut egui::Ui as *mut c_void, ud as *mut c_void);
            },
        );
        u64::from(ok)
    }

    pub fn register_menu_section(&self, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        EdgeSdk::get().register_menu_section(callback, userdata)
    }

    pub fn register_panel(&self, id: &str, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        honse_services::register_panel(id, callback, userdata)
    }

    pub fn register_overlay(&self, id: &str, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        honse_services::register_overlay(id, callback, userdata)
    }

    pub fn register_panel_chromeless(&self, id: &str, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        honse_services::register_panel_chromeless(id, callback, userdata)
    }

    pub fn register_panel_chromeless_fixed(
        &self,
        id: &str,
        callback: GuiMenuSectionCallback,
        userdata: *mut c_void,
    ) -> u64 {
        honse_services::register_panel_chromeless_fixed(id, callback, userdata)
    }

    pub fn set_overlay_visible(&self, id: &str, visible: bool) -> bool {
        honse_services::set_overlay_visible(id, visible)
    }

    pub fn overlay_set_visible(&self, id: &str, visible: bool) -> bool {
        honse_services::overlay_set_visible(id, visible)
    }

    #[must_use]
    pub fn overlay_visible(&self, id: &str) -> bool {
        honse_services::overlay_visible(id)
    }

    pub fn toggle_overlay(&self, id: &str) -> bool {
        honse_services::toggle_overlay(id)
    }

    pub fn register_hotkey(
        &self,
        id: &str,
        label: &str,
        default_mods: u8,
        default_vk: u16,
        callback: GuiMenuCallback,
        userdata: *mut c_void,
    ) -> u64 {
        honse_services::register_hotkey(id, label, default_mods, default_vk, callback, userdata)
    }

    pub fn unregister(&self, handle: u64) -> bool {
        let a = honse_services::unregister(handle);
        let b = honse_services::surface::unregister(handle);
        a || b
    }

    // ── Host services ──

    pub fn show_notification(&self, message: &str) -> bool {
        EdgeSdk::get().show_notification(message)
    }

    #[must_use]
    pub fn host_data_path(&self, rel: &str) -> Option<std::path::PathBuf> {
        EdgeSdk::get().data_path(rel)
    }

    #[must_use]
    pub fn gametora_data_dir(&self) -> Option<std::path::PathBuf> {
        honse_services::gametora_data_dir()
    }

    #[must_use]
    pub fn view_name(&self, view_id: i32) -> Option<&'static str> {
        honse_services::view_name(view_id)
    }
}

/// Match the SDK helper used by the tracker's class dumper.
#[allow(dead_code)]
pub(crate) fn cstr_to_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    // SAFETY: caller passes a valid NUL-terminated C string from il2cpp.
    unsafe { CStr::from_ptr(ptr) }.to_str().ok().map(ToOwned::to_owned)
}

/// Set overlay visibility only if no prior value exists (fork host helper).
pub fn set_overlay_visible_if_unset(id: &str, visible: bool) {
    honse_services::surface::set_overlay_visible_if_unset(id, visible);
}
