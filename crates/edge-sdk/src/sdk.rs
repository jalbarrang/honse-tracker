//! Safe `Sdk` facade over edge's resolved [`crate::api::Api`].
//!
//! # Compat surface partition (fork `training_tracker/compat.rs`)
//!
//! Full list from `rg -n 'pub (unsafe )?fn' compat.rs`, annotated with the
//! owning provider. This list feeds plan 3's `facts.json` (`unique_provider` /
//! `assigned` laws). Methods marked `edge-sdk` are implemented in this module
//! (or in `gui`/`log`/`entry` within this crate). Methods marked
//! `honse-services` are deferred to plan 2 and must NOT appear here.
//!
//! | method | provider |
//! |---|---|
//! | `ui_from_ptr` | edge-sdk (`gui.rs`, t-004) |
//! | `get` | edge-sdk |
//! | `try_get` | edge-sdk |
//! | `version` | edge-sdk |
//! | `has_capability` | honse-services (fork ABI concept; edge has no caps) |
//! | `resolve_symbol` | edge-sdk |
//! | `dlsym` | edge-sdk |
//! | `get_assembly_image` | edge-sdk |
//! | `get_class` | edge-sdk |
//! | `get_method` | edge-sdk |
//! | `get_method_addr` | edge-sdk |
//! | `find_nested_class` | edge-sdk |
//! | `get_field_from_name` | edge-sdk |
//! | `get_field_value` | edge-sdk |
//! | `get_singleton` | edge-sdk |
//! | `class_get_methods` | edge-sdk |
//! | `schedule_on_main_thread` | edge-sdk |
//! | `free_il2cpp_string` | edge-sdk (via `il2cpp_free` dlsym; edge has no dedicated get_api) |
//! | `hook` | edge-sdk |
//! | `unhook` | edge-sdk |
//! | `on` | honse-services |
//! | `off` | honse-services |
//! | `register_page` | honse-services |
//! | `register_tab` | honse-services |
//! | `register_page_with_icon` | honse-services |
//! | `register_menu_section` | edge-sdk |
//! | `register_panel` | honse-services |
//! | `register_overlay` | honse-services |
//! | `register_panel_chromeless` | honse-services |
//! | `register_panel_chromeless_fixed` | honse-services |
//! | `set_overlay_visible` | honse-services |
//! | `overlay_set_visible` | honse-services |
//! | `overlay_visible` | honse-services |
//! | `toggle_overlay` | honse-services |
//! | `register_hotkey` | honse-services |
//! | `unregister` | honse-services |
//! | `show_notification` | edge-sdk |
//! | `host_data_path` | edge-sdk (as `data_path`) |
//! | `gametora_data_dir` | honse-services |
//! | `view_name` | honse-services |
//! | `register_menu_section_with_icon` | edge-sdk (edge API; not in compat list but required by HANDOFF) |
//! | `base_dir` | edge-sdk (edge `hachimi_get_base_dir`) |
//!
//! Compat does **not** expose `set_field_value` / static-field helpers; those
//! edge get_api names are available via [`crate::api::Api`] if needed later.

#![allow(clippy::not_unsafe_ptr_arg_deref)] // facade mirrors fork compat: safe methods take host raw pointers

use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::{Path, PathBuf},
    sync::Mutex,
};

use once_cell::sync::OnceCell;

use crate::{
    api::Api,
    ffi::{
        FieldInfo, GuiMenuSectionCallback, Il2CppClass, Il2CppImage, Il2CppObject, Il2CppThread, Interceptor,
        MethodInfo,
    },
};

/// Edge plugin API version (`hachimi-edge` `VERSION = 3`).
pub const EDGE_API_VERSION: i32 = 3;

/// Stateless façade over the process-wide [`Api`].
#[derive(Clone, Copy)]
pub struct Sdk;

/// Process-lifetime interceptor pointer from the host (Send+Sync for static storage).
struct CachedInterceptor(*const Interceptor);
// SAFETY: the host interceptor is a process-lifetime singleton; we only read the pointer.
unsafe impl Send for CachedInterceptor {}
// SAFETY: same as Send — pointer is never mutated and outlives the process.
unsafe impl Sync for CachedInterceptor {}

static INTERCEPTOR: OnceCell<CachedInterceptor> = OnceCell::new();
static INTERCEPTOR_LOCK: Mutex<()> = Mutex::new(());

impl Sdk {
    /// Panic if `Api::init` has not run.
    #[must_use]
    pub fn get() -> &'static Sdk {
        let _ = Api::get();
        &Sdk
    }

    /// `None` before `Api::init`.
    #[must_use]
    pub fn try_get() -> Option<&'static Sdk> {
        Api::try_get().map(|_| &Sdk)
    }

    /// Always `3` for the edge host.
    #[must_use]
    pub fn version(&self) -> i32 {
        EDGE_API_VERSION
    }

    // ── IL2CPP ──

    #[must_use]
    pub fn resolve_symbol(&self, name: &str) -> Option<*mut c_void> {
        let api = Api::get();
        let f = api.il2cpp_resolve_symbol?;
        let c_name = CString::new(name).ok()?;
        // SAFETY: `name` is a valid CString; edge resolves against the loaded il2cpp export table.
        let ptr = unsafe { f(c_name.as_ptr()) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn dlsym(&self, name: &str) -> Option<*mut c_void> {
        self.resolve_symbol(name)
    }

    #[must_use]
    pub fn get_assembly_image(&self, assembly: &str) -> Option<*const Il2CppImage> {
        let api = Api::get();
        let f = api.il2cpp_get_assembly_image?;
        let c_name = CString::new(assembly).ok()?;
        // SAFETY: NUL-terminated assembly name; edge returns null on miss.
        let ptr = unsafe { f(c_name.as_ptr()) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn get_class(&self, image: *const Il2CppImage, namespace: &str, class_name: &str) -> Option<*mut Il2CppClass> {
        let api = Api::get();
        let f = api.il2cpp_get_class?;
        let ns = CString::new(namespace).ok()?;
        let cls = CString::new(class_name).ok()?;
        // SAFETY: `image` must be a valid Il2CppImage from `get_assembly_image`.
        let ptr = unsafe { f(image, ns.as_ptr(), cls.as_ptr()) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn get_method(&self, class: *mut Il2CppClass, name: &str, args_count: i32) -> Option<*const MethodInfo> {
        let api = Api::get();
        let f = api.il2cpp_get_method?;
        let c_name = CString::new(name).ok()?;
        // SAFETY: `class` must be a valid Il2CppClass pointer.
        let ptr = unsafe { f(class, c_name.as_ptr(), args_count) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn get_method_addr(&self, class: *mut Il2CppClass, name: &str, args_count: i32) -> Option<*mut c_void> {
        let api = Api::get();
        let f = api.il2cpp_get_method_addr?;
        let c_name = CString::new(name).ok()?;
        // SAFETY: `class` must be a valid Il2CppClass pointer.
        let ptr = unsafe { f(class, c_name.as_ptr(), args_count) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn find_nested_class(&self, parent: *mut Il2CppClass, name: &str) -> Option<*mut Il2CppClass> {
        let api = Api::get();
        let f = api.il2cpp_find_nested_class?;
        let c_name = CString::new(name).ok()?;
        // SAFETY: `parent` must be a valid Il2CppClass pointer.
        let ptr = unsafe { f(parent, c_name.as_ptr()) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn get_field_from_name(&self, class: *mut Il2CppClass, name: &str) -> Option<*mut FieldInfo> {
        let api = Api::get();
        let f = api.il2cpp_get_field_from_name?;
        let c_name = CString::new(name).ok()?;
        // SAFETY: `class` must be a valid Il2CppClass pointer.
        let ptr = unsafe { f(class, c_name.as_ptr()) };
        (!ptr.is_null()).then_some(ptr)
    }

    /// Read a field value into `out_value`.
    ///
    /// # Safety
    /// `obj`, `field`, and `out_value` must be valid IL2CPP pointers with a size
    /// matching the field type.
    pub unsafe fn get_field_value(&self, obj: *mut Il2CppObject, field: *mut FieldInfo, out_value: *mut c_void) {
        let api = Api::get();
        let Some(f) = api.il2cpp_get_field_value else {
            return;
        };
        // SAFETY: caller guarantees valid IL2CPP pointers and matching out buffer size.
        unsafe { f(obj, field, out_value) };
    }

    #[must_use]
    pub fn get_singleton(&self, class: *mut Il2CppClass) -> Option<*mut Il2CppObject> {
        let api = Api::get();
        let f = api.il2cpp_get_singleton_like_instance?;
        // SAFETY: `class` must be a valid Il2CppClass pointer.
        let ptr = unsafe { f(class) };
        (!ptr.is_null()).then_some(ptr)
    }

    #[must_use]
    pub fn class_get_methods(&self, klass: *mut Il2CppClass, iter: *mut *mut c_void) -> *const MethodInfo {
        let api = Api::get();
        let Some(f) = api.il2cpp_class_get_methods else {
            return std::ptr::null();
        };
        // SAFETY: `klass`/`iter` follow il2cpp_class_get_methods iteration contract.
        unsafe { f(klass, iter) }
    }

    /// Post `callback` onto the IL2CPP main (game) thread.
    pub fn schedule_on_main_thread(&self, callback: unsafe extern "C" fn()) {
        let api = Api::get();
        let Some(get_main) = api.il2cpp_get_main_thread else {
            return;
        };
        let Some(schedule) = api.il2cpp_schedule_on_thread else {
            return;
        };
        // SAFETY: edge returns the main Il2CppThread; callback has no captures and must
        // remain valid until invoked (same contract as the fork compat shim).
        let thread: *mut Il2CppThread = unsafe { get_main() };
        if thread.is_null() {
            return;
        }
        // SAFETY: main thread pointer from edge; callback is capture-free and must stay valid until run.
        unsafe { schedule(thread, callback) };
    }

    /// Free a string returned by il2cpp introspection (resolves `il2cpp_free`).
    ///
    /// Edge exposes no dedicated `free_il2cpp_string` get_api; this mirrors the
    /// fork compat shim by dlsym'ing `il2cpp_free`.
    pub fn free_il2cpp_string(&self, ptr: *mut c_char) {
        if ptr.is_null() {
            return;
        }
        if let Some(free_fn) = self.resolve_symbol("il2cpp_free") {
            type Il2CppFree = unsafe extern "C" fn(*mut c_void);
            // SAFETY: `il2cpp_free` signature; pointer came from the il2cpp allocator.
            let free: Il2CppFree = unsafe { std::mem::transmute_copy(&free_fn) };
            // SAFETY: `ptr` was allocated by il2cpp; free matches the allocator.
            unsafe { free(ptr.cast()) };
        }
    }

    // ── Hooking ──

    fn interceptor(&self) -> Option<*const Interceptor> {
        if let Some(CachedInterceptor(p)) = INTERCEPTOR.get() {
            return Some(*p);
        }
        let _guard = INTERCEPTOR_LOCK.lock().ok()?;
        if let Some(CachedInterceptor(p)) = INTERCEPTOR.get() {
            return Some(*p);
        }
        let api = Api::get();
        let instance = api.hachimi_instance?;
        let get_interceptor = api.hachimi_get_interceptor?;
        // SAFETY: edge returns the live Hachimi singleton.
        let hachimi = unsafe { instance() };
        if hachimi.is_null() {
            return None;
        }
        // SAFETY: `hachimi` is the live host singleton; returns its Interceptor.
        let interceptor = unsafe { get_interceptor(hachimi) };
        if interceptor.is_null() {
            return None;
        }
        let _ = INTERCEPTOR.set(CachedInterceptor(interceptor));
        Some(interceptor)
    }

    #[must_use]
    pub fn hook(&self, orig_addr: *mut c_void, hook_addr: *mut c_void) -> Option<*mut c_void> {
        let api = Api::get();
        let f = api.interceptor_hook?;
        let interceptor = self.interceptor()?;
        // SAFETY: interceptor from edge; addresses must be valid code pointers.
        let ptr = unsafe { f(interceptor, orig_addr, hook_addr) };
        (!ptr.is_null()).then_some(ptr)
    }

    pub fn unhook(&self, hook_addr: *mut c_void) -> Option<*mut c_void> {
        let api = Api::get();
        let f = api.interceptor_unhook?;
        let interceptor = self.interceptor()?;
        // SAFETY: interceptor from edge; `hook_addr` must be a previously hooked address.
        let ptr = unsafe { f(interceptor, hook_addr) };
        (!ptr.is_null()).then_some(ptr)
    }

    // ── GUI (raw C-callback wrappers; closure adapters live in `gui`) ──

    /// Register a menu section. Edge returns `bool`; we map true→1 / false→0 to
    /// mirror the fork compat `u64` handle shape (edge has no unregister handles).
    pub fn register_menu_section(&self, callback: GuiMenuSectionCallback, userdata: *mut c_void) -> u64 {
        let api = Api::get();
        let Some(f) = api.gui_register_menu_section else {
            return 0;
        };
        // SAFETY: callback/userdata must remain valid for the process lifetime
        // (edge stores them without reclaiming).
        let ok = unsafe { f(Some(callback), userdata) };
        u64::from(ok)
    }

    /// Register a titled menu section with an icon (edge-only; not in fork compat).
    pub fn register_menu_section_with_icon(
        &self,
        title: &str,
        icon_uri: Option<&str>,
        icon_bytes: &[u8],
        callback: GuiMenuSectionCallback,
        userdata: *mut c_void,
    ) -> u64 {
        let api = Api::get();
        let Some(f) = api.gui_register_menu_section_with_icon else {
            return 0;
        };
        let Ok(title_c) = CString::new(title) else {
            return 0;
        };
        let uri_c = icon_uri.and_then(|u| CString::new(u).ok());
        let uri_ptr = uri_c.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null());
        // SAFETY: title/icon buffers valid for the call; callback/userdata process-lifetime.
        let ok = unsafe {
            f(
                title_c.as_ptr(),
                uri_ptr,
                icon_bytes.as_ptr(),
                icon_bytes.len(),
                Some(callback),
                userdata,
            )
        };
        u64::from(ok)
    }

    pub fn show_notification(&self, message: &str) -> bool {
        let api = Api::get();
        let Some(f) = api.gui_show_notification else {
            return false;
        };
        let Ok(msg) = CString::new(message) else {
            return false;
        };
        // SAFETY: NUL-terminated message; edge copies it into its notification queue.
        unsafe { f(msg.as_ptr()) }
    }

    /// Register a per-frame present callback (`hachimi_register_present_callback`).
    ///
    /// `callback` and `userdata` must remain valid for the process lifetime —
    /// edge stores them without reclaiming.
    pub fn register_present_callback(&self, callback: crate::ffi::PresentCallback, userdata: *mut c_void) -> bool {
        let api = Api::get();
        let Some(f) = api.hachimi_register_present_callback else {
            return false;
        };
        // SAFETY: callback/userdata must remain valid for the process lifetime.
        unsafe { f(Some(callback), userdata) }
    }

    /// Register a one-shot game-initialized callback (`hachimi_register_on_game_initialized`).
    pub fn register_on_game_initialized(
        &self,
        callback: crate::ffi::GameInitializedCallback,
        userdata: *mut c_void,
    ) -> bool {
        let api = Api::get();
        let Some(f) = api.hachimi_register_on_game_initialized else {
            return false;
        };
        // SAFETY: callback/userdata must remain valid for the process lifetime.
        unsafe { f(Some(callback), userdata) }
    }

    // ── Paths ──

    /// Host base / data directory (`hachimi_get_base_dir`).
    #[must_use]
    pub fn base_dir(&self) -> Option<PathBuf> {
        let api = Api::get();
        let f = api.hachimi_get_base_dir?;
        // SAFETY: edge returns a process-lifetime CString pointer.
        let ptr = unsafe { f() };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: pointer from edge is NUL-terminated and immortal for the process.
        let s = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?;
        Some(PathBuf::from(s))
    }

    /// Resolve `rel` under the host data path (`hachimi_get_data_path` + join).
    /// Rejects absolute paths and `..` components (same policy as fork `host_data_path`).
    #[must_use]
    pub fn data_path(&self, rel: &str) -> Option<PathBuf> {
        let p = Path::new(rel);
        if p.has_root() || rel.split(['/', '\\']).any(|c| c == "..") {
            return None;
        }
        let api = Api::get();
        let f = api.hachimi_get_data_path?;
        // SAFETY: edge returns a process-lifetime CString pointer.
        let ptr = unsafe { f() };
        if ptr.is_null() {
            return None;
        }
        // SAFETY: pointer from edge is NUL-terminated and immortal for the process.
        let root = unsafe { CStr::from_ptr(ptr) }.to_str().ok()?;
        Some(Path::new(root).join(rel))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_get_is_none_before_init() {
        assert!(Sdk::try_get().is_none());
    }
}
