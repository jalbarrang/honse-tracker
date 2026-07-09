//! In-core compatibility shim for the training-tracker module.
//!
//! Training-tracker was authored as a cdylib plugin against `hachimi-plugin-sdk`,
//! reaching the host over the C ABI vtable. In-core it needs none of that: this
//! module re-implements the small slice of the SDK surface the tracker actually
//! uses ([`Sdk`], [`ui_from_ptr`], the `egui` re-export and
//! the `hlog_*` macros) but delegates straight to host internals — direct
//! `crate::il2cpp` calls and the owner-scoped `crate::core::plugin` registries.
//!
//! The tracker's source moved in almost verbatim: `crate::compat::X` simply
//! became `crate::compat::X`. The only behavioural
//! change is that there is no vtable, no CString marshaling and no manifest — the
//! shim is a thin, safe façade over code already linked into the host.

use std::ffi::{c_char, c_void, CStr, CString};

use hachimi_plugin_abi::{
    FieldInfo, GuiMenuCallback, GuiMenuSectionCallback, Il2CppClass, Il2CppImage, Il2CppObject, MethodInfo,
    PluginEventFn, API_VERSION,
};

use crate::core::plugin::{events, hotkeys, menu, notification, overlay, tab};
use crate::core::Hachimi;
use crate::il2cpp;

// ── Re-exports so moved tracker files keep their `crate::compat::…` imports
//    after a mechanical `crate::compat` → `…compat` rename. ──
pub use ::egui;
pub use hachimi_plugin_abi::{capability, event};

// ── Logging: the SDK's `hlog_*` macros log through the plugin vtable, which does
//    not exist in-core. Redefine them to forward to the host `log` facade. The
//    parent module pulls these into scope for the tracker submodules via
//    `#[macro_use] mod compat;`, so the moved source keeps calling them unqualified. ──

/// In-core `hlog_*!` — forward to the `log` crate. Brought into the tracker
/// submodules' scope by `#[macro_use] mod compat;` (declared before them), so the
/// moved source keeps calling `hlog_info!` etc. unqualified.
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

/// Host API version, mirroring [`crate::compat::ApiVersion`]. In-core every
/// capability/version gate is satisfied, so `at_least` is always true.
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

/// Cast a host-provided `Ui` pointer (handed to a menu/overlay callback) to a real
/// [`egui::Ui`].
///
/// # Safety
/// `ptr` must be the live `*mut egui::Ui` the host passed into the callback, and the
/// reference must not outlive that callback invocation.
#[must_use]
pub unsafe fn ui_from_ptr<'a>(ptr: *mut c_void) -> &'a mut egui::Ui {
    // SAFETY: caller guarantees `ptr` is the host's live `&mut egui::Ui`.
    unsafe { &mut *(ptr as *mut egui::Ui) }
}

/// In-core stand-in for `crate::compat::Sdk`: a stateless façade over host
/// services. Obtained via [`Sdk::get`] exactly like the cdylib SDK.
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
        ApiVersion::new(API_VERSION)
    }
    #[must_use]
    pub fn has_capability(&self, _cap: u64) -> bool {
        true
    }

    // ── IL2CPP ──

    #[must_use]
    pub fn resolve_symbol(&self, name: &str) -> Option<*mut c_void> {
        // SAFETY: dlsym only reads the loaded il2cpp export table.
        let addr = unsafe { il2cpp::symbols::dlsym(name) };
        (addr != 0).then_some(addr as *mut c_void)
    }

    #[must_use]
    pub fn dlsym(&self, name: &str) -> Option<*mut c_void> {
        self.resolve_symbol(name)
    }

    #[must_use]
    pub fn get_assembly_image(&self, assembly: &str) -> Option<*const Il2CppImage> {
        let name = CString::new(assembly).ok()?;
        il2cpp::symbols::get_assembly_image(&name)
            .ok()
            .map(|p| p as *const Il2CppImage)
    }

    #[must_use]
    pub fn get_class(&self, image: *const Il2CppImage, namespace: &str, class_name: &str) -> Option<*mut Il2CppClass> {
        let (ns, cls) = (CString::new(namespace).ok()?, CString::new(class_name).ok()?);
        il2cpp::symbols::get_class(image.cast(), &ns, &cls)
            .ok()
            .map(|p| p as *mut Il2CppClass)
    }

    #[must_use]
    pub fn get_method(&self, class: *mut Il2CppClass, name: &str, args_count: i32) -> Option<*const MethodInfo> {
        let name = CString::new(name).ok()?;
        il2cpp::symbols::get_method(class.cast(), &name, args_count)
            .ok()
            .map(|p| p as *const MethodInfo)
    }

    #[must_use]
    pub fn get_method_addr(&self, class: *mut Il2CppClass, name: &str, args_count: i32) -> Option<*mut c_void> {
        let name = CString::new(name).ok()?;
        let addr = il2cpp::symbols::get_method_addr(class.cast(), &name, args_count);
        (addr != 0).then_some(addr as *mut c_void)
    }

    #[must_use]
    pub fn find_nested_class(&self, parent: *mut Il2CppClass, name: &str) -> Option<*mut Il2CppClass> {
        let name = CString::new(name).ok()?;
        il2cpp::symbols::find_nested_class(parent.cast(), &name)
            .ok()
            .map(|p| p as *mut Il2CppClass)
    }

    #[must_use]
    pub fn get_field_from_name(&self, class: *mut Il2CppClass, name: &str) -> Option<*mut FieldInfo> {
        let name = CString::new(name).ok()?;
        let ptr = il2cpp::symbols::get_field_from_name(class.cast(), &name);
        (!ptr.is_null()).then_some(ptr as *mut FieldInfo)
    }

    /// Read a field value into `out_value`.
    ///
    /// # Safety
    /// `obj`, `field`, and `out_value` must be valid IL2CPP pointers with a size
    /// matching the field type.
    pub unsafe fn get_field_value(&self, obj: *mut Il2CppObject, field: *mut FieldInfo, out_value: *mut c_void) {
        il2cpp::api::il2cpp_field_get_value(obj.cast(), field.cast(), out_value);
    }

    #[must_use]
    pub fn get_singleton(&self, class: *mut Il2CppClass) -> Option<*mut Il2CppObject> {
        il2cpp::symbols::SingletonLike::new(class.cast()).map(|s| s.instance() as *mut Il2CppObject)
    }

    #[must_use]
    pub fn class_get_methods(&self, klass: *mut Il2CppClass, iter: *mut *mut c_void) -> *const MethodInfo {
        il2cpp::api::il2cpp_class_get_methods(klass.cast(), iter) as *const MethodInfo
    }

    /// Post `callback` onto the IL2CPP main (game) thread.
    pub fn schedule_on_main_thread(&self, callback: unsafe extern "C" fn()) {
        // SAFETY: `callback` must remain valid until invoked (it has no captures).
        il2cpp::symbols::Thread::main_thread().schedule(unsafe { std::mem::transmute(callback) });
    }

    /// Free a string returned by il2cpp introspection (resolves `il2cpp_free`).
    pub fn free_il2cpp_string(&self, ptr: *mut c_char) {
        if ptr.is_null() {
            return;
        }
        if let Some(free_fn) = self.resolve_symbol("il2cpp_free") {
            type Il2CppFree = unsafe extern "C" fn(*mut c_void);
            // SAFETY: `il2cpp_free` signature; pointer came from the il2cpp allocator.
            unsafe { std::mem::transmute::<_, Il2CppFree>(free_fn)(ptr.cast()) }
        }
    }

    // ── Hooking ──

    #[must_use]
    pub fn hook(&self, orig_addr: *mut c_void, hook_addr: *mut c_void) -> Option<*mut c_void> {
        Hachimi::instance()
            .interceptor
            .hook(orig_addr as usize, hook_addr as usize)
            .inspect_err(|e| log::error!("{}", e))
            .ok()
            .map(|a| a as *mut c_void)
    }

    pub fn unhook(&self, hook_addr: *mut c_void) -> Option<*mut c_void> {
        Hachimi::instance()
            .interceptor
            .unhook(hook_addr as usize)
            .map(|h| h.orig_addr as *mut c_void)
    }

    // ── Events ──

    pub fn on(&self, event_id: u32, callback: PluginEventFn, userdata: *mut c_void) -> u64 {
        events::subscribe(event_id, callback, userdata)
    }

    pub fn off(&self, handle: u64) {
        events::unsubscribe(handle);
    }

    // ── GUI registration (the tracker passes `extern "C"` callbacks, so these route
    //    straight to the host's C-tier registries). ──

    pub fn register_page(&self, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        menu::register_plugin_menu_section(callback, userdata)
    }

    /// Register an in-core top-level Control Center tab body with a Rust closure.
    /// The host surfaces it as its own tab (not a Plugins L1 page).
    pub fn register_tab<F>(&self, draw: F) -> u64
    where
        F: Fn(&mut egui::Ui) + Send + Sync + 'static,
    {
        tab::register_tab_rust(std::sync::Arc::new(draw))
    }

    pub fn register_page_with_icon(
        &self,
        title: &str,
        icon_uri: &str,
        icon_bytes: &[u8],
        callback: GuiMenuSectionCallback,
        userdata: *mut c_void,
    ) -> u64 {
        menu::register_plugin_menu_section_with_icon(
            title.to_owned(),
            icon_uri.to_owned(),
            icon_bytes.to_vec(),
            callback,
            userdata,
        )
    }

    pub fn register_menu_section(&self, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        menu::register_plugin_menu_section(callback, userdata)
    }

    pub fn register_panel(&self, id: &str, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        overlay::register_plugin_overlay(id.to_owned(), callback, userdata)
    }

    pub fn register_overlay(&self, id: &str, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        overlay::register_plugin_overlay(id.to_owned(), callback, userdata)
    }

    pub fn register_panel_chromeless(&self, id: &str, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        overlay::register_plugin_overlay_ex(
            id.to_owned(),
            hachimi_plugin_abi::overlay_flags::CHROMELESS,
            callback,
            userdata,
        )
    }

    pub fn register_panel_chromeless_fixed(
        &self,
        id: &str,
        callback: GuiMenuSectionCallback,
        userdata: *mut c_void,
    ) -> u64 {
        let flags = hachimi_plugin_abi::overlay_flags::CHROMELESS | hachimi_plugin_abi::overlay_flags::FIXED;
        overlay::register_plugin_overlay_ex(id.to_owned(), flags, callback, userdata)
    }

    pub fn set_overlay_visible(&self, id: &str, visible: bool) -> bool {
        overlay::set_overlay_visible(id, visible);
        true
    }

    pub fn overlay_set_visible(&self, id: &str, visible: bool) -> bool {
        self.set_overlay_visible(id, visible)
    }

    #[must_use]
    pub fn overlay_visible(&self, id: &str) -> bool {
        overlay::is_overlay_visible(id)
    }

    pub fn toggle_overlay(&self, id: &str) -> bool {
        self.set_overlay_visible(id, !self.overlay_visible(id))
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
        hotkeys::register_plugin(
            id.to_owned(),
            label.to_owned(),
            hotkeys::Chord::new(default_mods, default_vk),
            callback,
            userdata,
        )
    }

    pub fn unregister(&self, handle: u64) -> bool {
        crate::core::plugin::unregister(handle)
    }

    // ── Host services ──

    pub fn show_notification(&self, message: &str) -> bool {
        notification::enqueue(message.to_owned());
        true
    }

    #[must_use]
    pub fn host_data_path(&self, rel: &str) -> Option<std::path::PathBuf> {
        // Reject absolute/escaping paths, matching the host vtable service.
        let p = std::path::Path::new(rel);
        if p.has_root() || rel.split(['/', '\\']).any(|c| c == "..") {
            return None;
        }
        Some(Hachimi::instance().get_data_path(rel))
    }

    #[must_use]
    pub fn gametora_data_dir(&self) -> Option<std::path::PathBuf> {
        self.host_data_path(hachimi_plugin_abi::GAMETORA_DATA_SUBDIR)
    }

    #[must_use]
    pub fn view_name(&self, view_id: i32) -> Option<&'static str> {
        crate::core::scene_views::view_name(view_id)
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
