//! GUI helpers: `ui_from_ptr`, true-ABI window wrappers, menu-section closure adapters.
//!
//! # ⚠ HOST-EGUI = ABI LOCKSTEP
//!
//! Everything in this module that passes an `egui::Ui`/`egui::Context` across
//! the plugin↔host boundary (`ui_from_ptr`, `show_window`, `reshow_window`,
//! `register_menu_section*`) is only sound when the plugin is built from the
//! SAME egui source as the Edge binary's `Cargo.lock` with the SAME rustc.
//! **No shipped honse plugin calls these anymore** — all plugin UI renders on
//! the self-hosted overlay (`honse_services::overlay`). If you add a caller,
//! the lockstep contract (README "Compatibility") applies to your build again.
//! `register_menu_item` (label + callback, no egui) is lockstep-free.
//!
//! # The lockstep-free way to draw a menu section
//!
//! [`register_menu_section_raw`] plus the [`ui`] primitives are **exempt**: the
//! host's `Ui` stays an opaque `*mut c_void` that we only ever hand straight
//! back, and the host does the drawing. Nothing with a `repr(Rust)` layout
//! crosses the boundary, so a section built that way works on any rustc — the
//! same reason `register_menu_item` does. Prefer it; reach for the `egui::Ui`
//! versions above only when a primitive genuinely does not exist.
//!
//! # Host window lifetime (edge ABI)
//!
//! `gui_show_window` creates a **decorated** egui window (title bar + close [X]).
//! When the user closes it (or the plugin calls [`close_window`]), the host drops
//! the window from its vec permanently and **never** invokes our callbacks again —
//! the plugin is **not** notified. Therefore userdata `Box`es live in an
//! sdk-internal registry keyed by window id and are reclaimed only on our own
//! [`close_window`]. Process-lifetime windows that the user may close via [X]
//! intentionally leak one allocation per window id (bounded); [`reshow_window`]
//! can re-show a dropped window reusing the **same** registered closures.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::{
    collections::HashMap,
    ffi::{c_void, CString},
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::{
    api::Api,
    ffi::{GuiMenuCallback, GuiMenuSectionCallback, GuiWindowCallback},
};

/// Cast a host-provided `Ui` pointer to a real [`egui::Ui`].
///
/// # Safety
/// `ui` must be the non-null `*mut c_void` the host passed into a GUI callback
/// (really `&mut egui::Ui`), and the returned reference must not outlive that
/// callback invocation. Sound only when this plugin and the host compile the
/// **same egui 0.33.x** (pinned to `hachimi-edge` `Cargo.lock`) with the
/// **same rustc** — `repr(Rust)` layout is not stable across compiler
/// versions, so the workspace `rust-toolchain.toml` pins the exact rustc the
/// targeted Hachimi-Edge release binary was built with (verify with
/// `scripts/check-rustc-lockstep.ps1`). The sdk owns this cast in exactly
/// this function.
#[must_use]
pub unsafe fn ui_from_ptr<'a>(ui: *mut c_void) -> &'a mut egui::Ui {
    // SAFETY: caller + egui-version-lockstep invariant documented above.
    unsafe { &mut *(ui as *mut egui::Ui) }
}

type UiFn = Box<dyn FnMut(*mut c_void) + Send>;

struct WindowCallbacks {
    contents: UiFn,
    bottom: Option<UiFn>,
}

/// Registry owns the `Box<WindowCallbacks>` for reclaim on [`close_window`].
/// The raw pointer passed to the host aliases the heap allocation inside the Box.
static WINDOW_REGISTRY: Lazy<Mutex<HashMap<i32, Box<WindowCallbacks>>>> = Lazy::new(|| Mutex::new(HashMap::new()));

struct MenuSectionUserdata {
    callback: Box<dyn FnMut(*mut c_void) + Send>,
}

/// Allocate a fresh plugin window id from the host.
#[must_use]
pub fn new_window_id() -> i32 {
    let Some(api) = Api::try_get() else {
        return -1;
    };
    let Some(f) = api.gui_new_window_id else {
        return -1;
    };
    // SAFETY: edge allocates from its atomic counter; no pointers involved.
    unsafe { f() }
}

extern "C" fn window_contents_trampoline(ui: *mut c_void, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: userdata is `*mut WindowCallbacks` registered in WINDOW_REGISTRY.
    let cbs = unsafe { &mut *(userdata as *mut WindowCallbacks) };
    (cbs.contents)(ui);
}

extern "C" fn window_bottom_trampoline(ui: *mut c_void, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: userdata is `*mut WindowCallbacks` registered in WINDOW_REGISTRY.
    let cbs = unsafe { &mut *(userdata as *mut WindowCallbacks) };
    if let Some(ref mut bottom) = cbs.bottom {
        bottom(ui);
    }
}

/// Show a host window with contents + optional bottom bar callbacks.
///
/// Maps to edge `gui_show_window(id, title, contents_callback, bottom_callback, userdata)` —
/// **two** callbacks, **one** shared userdata; callbacks return **unit**.
pub fn show_window(
    id: i32,
    title: &str,
    mut contents: impl FnMut(&mut egui::Ui) + Send + 'static,
    bottom: Option<impl FnMut(&mut egui::Ui) + Send + 'static>,
) -> bool {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    let Some(f) = api.gui_show_window else {
        return false;
    };
    let Ok(title_c) = CString::new(title) else {
        return false;
    };

    let contents_fn: UiFn = Box::new(move |ui_ptr| {
        if ui_ptr.is_null() {
            return;
        }
        // SAFETY: non-null host ui pointer for this callback frame.
        let ui = unsafe { ui_from_ptr(ui_ptr) };
        contents(ui);
    });
    let bottom_fn: Option<UiFn> = bottom.map(|mut b| {
        let f: UiFn = Box::new(move |ui_ptr: *mut c_void| {
            if ui_ptr.is_null() {
                return;
            }
            // SAFETY: non-null host ui pointer for this callback frame.
            let ui = unsafe { ui_from_ptr(ui_ptr) };
            b(ui);
        });
        f
    });
    let has_bottom = bottom_fn.is_some();

    let boxed = Box::new(WindowCallbacks {
        contents: contents_fn,
        bottom: bottom_fn,
    });
    // into_raw → from_raw into the registry keeps the same heap address for the host.
    let userdata = Box::into_raw(boxed) as *mut c_void;
    {
        let mut reg = WINDOW_REGISTRY.lock();
        // SAFETY: userdata was just produced by Box::into_raw of WindowCallbacks.
        let boxed = unsafe { Box::from_raw(userdata as *mut WindowCallbacks) };
        reg.insert(id, boxed);
    }

    let contents_cb: Option<GuiWindowCallback> = Some(window_contents_trampoline);
    let bottom_cb: Option<GuiWindowCallback> = if has_bottom {
        Some(window_bottom_trampoline)
    } else {
        None
    };

    // SAFETY: title is NUL-terminated for the call; userdata points at registry-owned Box.
    unsafe { f(id, title_c.as_ptr(), contents_cb, bottom_cb, userdata) }
}

/// Re-show a window under a fresh host id, reusing the **same** registered
/// closures (userdata pointer unchanged). Used by honse-services' surface
/// reopen path after the host permanently drops a user-closed window.
///
/// Moves the registry entry from `old_id` → `new_id`. Returns `false` if
/// `old_id` is unknown or the host call fails.
pub fn reshow_window(old_id: i32, new_id: i32, title: &str) -> bool {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    let Some(f) = api.gui_show_window else {
        return false;
    };
    let Ok(title_c) = CString::new(title) else {
        return false;
    };

    let userdata = {
        let mut reg = WINDOW_REGISTRY.lock();
        let Some(boxed) = reg.remove(&old_id) else {
            return false;
        };
        let has_bottom = boxed.bottom.is_some();
        let userdata = Box::into_raw(boxed) as *mut c_void;
        // SAFETY: just produced by Box::into_raw of WindowCallbacks.
        let boxed = unsafe { Box::from_raw(userdata as *mut WindowCallbacks) };
        reg.insert(new_id, boxed);
        (userdata, has_bottom)
    };

    let contents_cb: Option<GuiWindowCallback> = Some(window_contents_trampoline);
    let bottom_cb: Option<GuiWindowCallback> = if userdata.1 {
        Some(window_bottom_trampoline)
    } else {
        None
    };

    // SAFETY: title NUL-terminated for the call; userdata is registry-owned.
    unsafe { f(new_id, title_c.as_ptr(), contents_cb, bottom_cb, userdata.0) }
}

/// Close a window and reclaim its registered userdata (if any).
pub fn close_window(id: i32) {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    if let Some(f) = api.gui_close_window {
        // SAFETY: edge removes the window by id; no other pointers involved.
        unsafe { f(id) };
    }
    let mut reg = WINDOW_REGISTRY.lock();
    let _ = reg.remove(&id);
}

struct MenuItemUserdata {
    on_click: Box<dyn FnMut() + Send>,
}

extern "C" fn menu_item_trampoline(userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: userdata is `*mut MenuItemUserdata` leaked for process lifetime.
    let data = unsafe { &mut *(userdata as *mut MenuItemUserdata) };
    (data.on_click)();
}

/// Register a clickable item in the host (Hachimi) menu from a Rust closure.
///
/// Maps to edge `gui_register_menu_item(label, callback, userdata)`. The
/// closure + userdata are intentionally leaked (process lifetime, one per item).
pub fn register_menu_item(label: &str, on_click: impl FnMut() + Send + 'static) -> bool {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    let Some(f) = api.gui_register_menu_item else {
        return false;
    };
    let Ok(label_c) = CString::new(label) else {
        return false;
    };
    let data = Box::new(MenuItemUserdata {
        on_click: Box::new(on_click),
    });
    let userdata = Box::into_raw(data) as *mut c_void;
    // SAFETY: label NUL-terminated for the call; trampoline + userdata are a
    // process-lifetime leak by design.
    unsafe {
        f(
            label_c.as_ptr(),
            Some(menu_item_trampoline as GuiMenuCallback),
            userdata,
        )
    }
}

extern "C" fn menu_section_trampoline(ui: *mut c_void, userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: userdata is `*mut MenuSectionUserdata` leaked for process lifetime.
    let data = unsafe { &mut *(userdata as *mut MenuSectionUserdata) };
    (data.callback)(ui);
}

/// Register a menu section that draws through the host's own widgets.
///
/// The callback receives the host `Ui` as the opaque pointer it really is;
/// pass it to the [`ui`] primitives and nothing else. **No egui lockstep** —
/// see the module header.
///
/// Returns `false` when the host does not export `gui_register_menu_section`,
/// which is the caller's cue to fall back to [`register_menu_item`] rather
/// than leave the menu empty.
pub fn register_menu_section_raw(mut draw: impl FnMut(*mut c_void) + Send + 'static) -> bool {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    let Some(f) = api.gui_register_menu_section else {
        return false;
    };
    let data = Box::new(MenuSectionUserdata {
        callback: Box::new(move |ui_ptr| {
            if ui_ptr.is_null() {
                return;
            }
            draw(ui_ptr);
        }),
    });
    let userdata = Box::into_raw(data) as *mut c_void;
    // SAFETY: trampoline + userdata remain valid for process lifetime (intentionally leaked).
    unsafe { f(Some(menu_section_trampoline as GuiMenuSectionCallback), userdata) }
}

/// Register a menu section from a Rust closure (trampoline + leaked userdata).
pub fn register_menu_section(mut draw: impl FnMut(&mut egui::Ui) + Send + 'static) -> bool {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    let Some(f) = api.gui_register_menu_section else {
        return false;
    };
    let data = Box::new(MenuSectionUserdata {
        callback: Box::new(move |ui_ptr| {
            if ui_ptr.is_null() {
                return;
            }
            // SAFETY: non-null host ui pointer for this callback frame.
            let ui = unsafe { ui_from_ptr(ui_ptr) };
            draw(ui);
        }),
    });
    let userdata = Box::into_raw(data) as *mut c_void;
    // SAFETY: trampoline + userdata remain valid for process lifetime (intentionally leaked).
    unsafe { f(Some(menu_section_trampoline as GuiMenuSectionCallback), userdata) }
}

/// Register a titled menu section with icon bytes from a Rust closure.
pub fn register_menu_section_with_icon(
    title: &str,
    icon_uri: Option<&str>,
    icon_bytes: &[u8],
    mut draw: impl FnMut(&mut egui::Ui) + Send + 'static,
) -> bool {
    let Some(api) = Api::try_get() else {
        return Default::default();
    };
    let Some(f) = api.gui_register_menu_section_with_icon else {
        return false;
    };
    let Ok(title_c) = CString::new(title) else {
        return false;
    };
    let uri_c = icon_uri.and_then(|u| CString::new(u).ok());
    let uri_ptr = uri_c.as_ref().map(|c| c.as_ptr()).unwrap_or(std::ptr::null());

    let data = Box::new(MenuSectionUserdata {
        callback: Box::new(move |ui_ptr| {
            if ui_ptr.is_null() {
                return;
            }
            // SAFETY: non-null host ui pointer for this callback frame.
            let ui = unsafe { ui_from_ptr(ui_ptr) };
            draw(ui);
        }),
    });
    let userdata = Box::into_raw(data) as *mut c_void;
    // SAFETY: title/icon valid for call; trampoline+userdata process-lifetime leak.
    unsafe {
        f(
            title_c.as_ptr(),
            uri_ptr,
            icon_bytes.as_ptr(),
            icon_bytes.len(),
            Some(menu_section_trampoline as GuiMenuSectionCallback),
            userdata,
        )
    }
}

/// Host-drawn widgets, for use inside a [`register_menu_section_raw`] callback.
///
/// Every function here takes the host `Ui` as the opaque pointer the callback
/// was handed and passes only C types across the boundary, so none of the
/// egui-lockstep rules in the module header apply. Each is a no-op when the
/// host does not export it — a missing widget, never a crash.
pub mod ui {
    use std::ffi::{c_void, CString};

    use crate::api::Api;

    /// Call one `fn(ui, *const c_char) -> bool` text widget.
    fn text_widget(
        ui: *mut c_void,
        text: &str,
        pick: impl FnOnce(&Api) -> Option<unsafe extern "C" fn(*mut c_void, *const std::ffi::c_char) -> bool>,
    ) -> bool {
        let Some(api) = Api::try_get() else {
            return false;
        };
        let Some(f) = pick(api) else {
            return false;
        };
        let Ok(text_c) = CString::new(text) else {
            return false;
        };
        // SAFETY: `ui` is the host's own pointer, passed straight back; the
        // string is NUL-terminated and outlives the call.
        unsafe { f(ui, text_c.as_ptr()) }
    }

    /// A section title.
    pub fn heading(ui: *mut c_void, text: &str) {
        let _ = text_widget(ui, text, |api| api.gui_ui_heading);
    }

    /// A line of body text.
    pub fn label(ui: *mut c_void, text: &str) {
        let _ = text_widget(ui, text, |api| api.gui_ui_label);
    }

    /// A line of small print — subheadings, hints, paths.
    pub fn small(ui: *mut c_void, text: &str) {
        let _ = text_widget(ui, text, |api| api.gui_ui_small);
    }

    /// A horizontal rule.
    pub fn separator(ui: *mut c_void) {
        let Some(api) = Api::try_get() else {
            return;
        };
        let Some(f) = api.gui_ui_separator else {
            return;
        };
        // SAFETY: `ui` is the host's own pointer, passed straight back.
        let _ = unsafe { f(ui) };
    }

    /// A button. Returns `true` on the frame it was clicked.
    #[must_use]
    pub fn button(ui: *mut c_void, text: &str) -> bool {
        text_widget(ui, text, |api| api.gui_ui_button)
    }

    /// A small button. Returns `true` on the frame it was clicked.
    #[must_use]
    pub fn small_button(ui: *mut c_void, text: &str) -> bool {
        text_widget(ui, text, |api| api.gui_ui_small_button)
    }

    /// A checkbox over `value`, which the host writes through.
    ///
    /// Returns the state *after* the frame, and whether it changed — read from
    /// `value` itself rather than from the host's return, so the answer does
    /// not depend on which of `changed()`/`clicked()` the host chose to report.
    pub fn checkbox(ui: *mut c_void, text: &str, value: bool) -> (bool, bool) {
        let Some(api) = Api::try_get() else {
            return (value, false);
        };
        let Some(f) = api.gui_ui_checkbox else {
            return (value, false);
        };
        let Ok(text_c) = CString::new(text) else {
            return (value, false);
        };
        let mut slot = value;
        // SAFETY: `ui` is the host's own pointer; the string is NUL-terminated
        // and `slot` is a live `bool` for the duration of the call.
        unsafe { f(ui, text_c.as_ptr(), &raw mut slot) };
        (slot, slot != value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_trampoline_invokes_boxed_userdata_with_null_ui() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static HITS: AtomicU32 = AtomicU32::new(0);
        HITS.store(0, Ordering::SeqCst);
        let mut cbs = WindowCallbacks {
            contents: Box::new(|ui| {
                assert!(ui.is_null());
                HITS.fetch_add(1, Ordering::SeqCst);
            }),
            bottom: None,
        };
        let userdata = &mut cbs as *mut WindowCallbacks as *mut c_void;
        window_contents_trampoline(std::ptr::null_mut(), userdata);
        assert_eq!(HITS.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn window_registry_reclaims_on_close_without_api() {
        // Insert a dummy entry and remove via registry path used by close_window.
        let boxed = Box::new(WindowCallbacks {
            contents: Box::new(|_| {}),
            bottom: None,
        });
        let id = 42_001;
        {
            let mut reg = WINDOW_REGISTRY.lock();
            reg.insert(id, boxed);
            assert!(reg.contains_key(&id));
        }
        // Simulate reclaim half of close_window (Api may be uninitialized).
        {
            let mut reg = WINDOW_REGISTRY.lock();
            assert!(reg.remove(&id).is_some());
            assert!(!reg.contains_key(&id));
        }
    }
}
