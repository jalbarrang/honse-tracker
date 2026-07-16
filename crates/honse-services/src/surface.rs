//! Surface layer: watchdog-persistent host window + overlay/panel/tab registries.
//!
//! # Window reality (edge ABI)
//!
//! Host plugin windows are decorated and user-closable. On close the host
//! **permanently drops** the window and never calls our callbacks again. While
//! alive they render every frame regardless of menu visibility.
//!
//! # Design
//!
//! 1. [`Surface::ensure`] creates one surface window titled "Honse Tracker" via
//!    edge-sdk [`show_window`].
//! 2. A present-callback watchdog compares `last_drawn_frame` to the current
//!    frame counter; if the contents callback has not run for >2 frames, the
//!    host dropped the window — the user clicked [X]. **The close is
//!    respected**: the window (and every overlay it hosts — see below) stays
//!    hidden until the user asks for it back via the host-menu item
//!    ("Show <title>") or by turning any overlay visible (panel hotkey /
//!    checkbox), both of which call [`reopen`] → [`reshow_window`] with a
//!    fresh `gui_new_window_id`, reusing the same registered closures.
//! 3. Overlay/panel egui state uses **our** stable ids
//!    (`egui::Id::new("honse-overlay").with(name)`), never the host window id,
//!    so positions survive re-shows.
//! 4. Rendering constraint: overlays/panels are painted from *inside* this
//!    window's contents callback (the only per-frame egui entry point edge
//!    gives plugins), so a closed surface window also stops the overlays.
//!    That is why turning an overlay visible force-reopens the window.
//!
//! Menu sections / pages with icons delegate to edge-sdk wrappers — never raw
//! `get_api`.

use std::{
    collections::HashMap,
    ffi::c_void,
    sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering},
};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::next_handle;

/// Overlay presentation flags (fork `legacy plugin ABI::overlay_flags`).
pub mod overlay_flags {
    pub const CHROMELESS: u64 = 1 << 0;
    pub const FIXED: u64 = 1 << 1;
}

type GuiDrawFn = extern "C" fn(*mut c_void, *mut c_void);

struct OverlaySnapshot {
    id: String,
    chromeless: bool,
    fixed: bool,
    callback: GuiDrawFn,
    userdata: usize,
    fixed_pos: Option<egui::Pos2>,
}

struct OverlayEntry {
    handle: u64,
    id: String,
    chromeless: bool,
    fixed: bool,
    /// Stored as usize so the entry is Send+Sync.
    callback: GuiDrawFn,
    userdata: usize,
}

struct TabEntry {
    handle: u64,
    title: String,
    callback: GuiDrawFn,
    userdata: usize,
}

struct SurfaceState {
    overlays: Vec<OverlayEntry>,
    tabs: Vec<TabEntry>,
    /// Visibility keyed by overlay id. Unknown id → default `true` (fork
    /// `is_overlay_visible`).
    visible: HashMap<String, bool>,
    /// Optional fixed position for chromeless_fixed panels.
    fixed_pos: HashMap<String, egui::Pos2>,
    selected_tab: usize,
}

static STATE: Lazy<Mutex<SurfaceState>> = Lazy::new(|| {
    Mutex::new(SurfaceState {
        overlays: Vec::new(),
        tabs: Vec::new(),
        visible: HashMap::new(),
        fixed_pos: HashMap::new(),
        selected_tab: 0,
    })
});

static FRAME_COUNTER: AtomicU64 = AtomicU64::new(0);
static LAST_DRAWN_FRAME: AtomicU64 = AtomicU64::new(0);
static SURFACE_WINDOW_ID: AtomicI32 = AtomicI32::new(-1);
static SURFACE_ENSURED: AtomicBool = AtomicBool::new(false);
static WATCHDOG_INSTALLED: AtomicBool = AtomicBool::new(false);
/// Set when the watchdog detects a user [X]; cleared by [`reopen`].
static USER_CLOSED: AtomicBool = AtomicBool::new(false);
static MENU_ITEM_REGISTERED: AtomicBool = AtomicBool::new(false);

/// Per-plugin surface title. Each plugin DLL statically links its own copy of
/// honse-services, so this is per-DLL state — set it in `init` BEFORE any
/// `register_*` call, otherwise two plugins both show "Honse Tracker" windows
/// and duplicate "Show Honse Tracker" host-menu items.
static TITLE: Lazy<Mutex<String>> = Lazy::new(|| Mutex::new("Honse Tracker".to_owned()));

/// Set this plugin's surface window title (and host-menu reopen label).
pub fn set_surface_title(title: &str) {
    *TITLE.lock() = title.to_owned();
}

fn surface_title() -> String {
    TITLE.lock().clone()
}
/// Re-show if contents callback missed more than this many present ticks.
const WATCHDOG_MISS_THRESHOLD: u64 = 2;

/// Pure watchdog decision: has the host dropped the surface window (user [X])?
///
/// `last_drawn == 0` means the contents callback has never run (just created /
/// not yet painted, or already marked closed) — not a drop.
#[must_use]
pub fn watchdog_should_reshow(last_drawn_frame: u64, current_frame: u64) -> bool {
    if last_drawn_frame == 0 {
        return false;
    }
    current_frame.saturating_sub(last_drawn_frame) > WATCHDOG_MISS_THRESHOLD
}

/// Namespace for surface lifecycle helpers.
pub struct Surface;

impl Surface {
    /// Ensure the surface window exists and the present-callback watchdog is armed.
    pub fn ensure() {
        ensure();
    }
}

/// Ensure the surface window exists and the present-callback watchdog is armed.
pub fn ensure() {
    install_watchdog_job();
    register_reopen_menu_item();
    if SURFACE_ENSURED.swap(true, Ordering::SeqCst) {
        return;
    }
    show_surface_fresh();
}

fn install_watchdog_job() {
    if WATCHDOG_INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    crate::frame::register_frame_job(Box::new(|| {
        let frame = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed) + 1;
        let last = LAST_DRAWN_FRAME.load(Ordering::Relaxed);
        if watchdog_should_reshow(last, frame) {
            // Host dropped the window — the user closed it. Respect it.
            LAST_DRAWN_FRAME.store(0, Ordering::Relaxed);
            USER_CLOSED.store(true, Ordering::SeqCst);
            log::info!("honse-services: surface window closed by user; staying closed");
            if let Some(sdk) = edge_sdk::Sdk::try_get() {
                sdk.show_notification(&format!(
                    "{} hidden — reopen from the Hachimi menu or a panel hotkey",
                    surface_title()
                ));
            }
        }
    }));
}

/// Re-show the surface window after a user close (or ensure it on first use).
/// No-op when the window is already alive.
pub fn reopen() {
    if !SURFACE_ENSURED.load(Ordering::SeqCst) {
        ensure();
        return;
    }
    if USER_CLOSED.swap(false, Ordering::SeqCst) {
        reshow_surface();
    }
}

fn register_reopen_menu_item() {
    if MENU_ITEM_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }
    if !edge_sdk::register_menu_item(&format!("Show {}", surface_title()), reopen) {
        log::warn!("honse-services: gui_register_menu_item failed for surface reopen");
        MENU_ITEM_REGISTERED.store(false, Ordering::SeqCst);
    }
}

fn show_surface_fresh() {
    let id = edge_sdk::new_window_id();
    if id < 0 {
        log::error!("honse-services: gui_new_window_id failed");
        SURFACE_ENSURED.store(false, Ordering::SeqCst);
        return;
    }
    SURFACE_WINDOW_ID.store(id, Ordering::SeqCst);
    LAST_DRAWN_FRAME.store(0, Ordering::SeqCst);
    let ok = edge_sdk::show_window(
        id,
        &surface_title(),
        |ui| {
            LAST_DRAWN_FRAME.store(FRAME_COUNTER.load(Ordering::Relaxed), Ordering::Relaxed);
            draw_surface_contents(ui);
        },
        None::<fn(&mut egui::Ui)>,
    );
    if !ok {
        log::error!("honse-services: show_window failed for surface");
        SURFACE_ENSURED.store(false, Ordering::SeqCst);
    }
}

fn reshow_surface() {
    let old_id = SURFACE_WINDOW_ID.load(Ordering::SeqCst);
    let new_id = edge_sdk::new_window_id();
    if new_id < 0 {
        log::error!("honse-services: gui_new_window_id failed during reshow");
        return;
    }
    if edge_sdk::reshow_window(old_id, new_id, &surface_title()) {
        SURFACE_WINDOW_ID.store(new_id, Ordering::SeqCst);
        // Reset so we don't immediately re-trigger before the new window paints.
        LAST_DRAWN_FRAME.store(0, Ordering::SeqCst);
        log::debug!("honse-services: surface re-shown (host dropped window {old_id} → {new_id})");
    } else {
        // Registry miss (e.g. first ensure never succeeded) — create fresh.
        SURFACE_ENSURED.store(false, Ordering::SeqCst);
        ensure();
    }
}

fn draw_surface_contents(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();

    // Overlays / panels paint on the shared egui context with stable ids.
    draw_overlays(&ctx);

    // Tabs / pages draw inside our own tabbed strip in the surface window.
    draw_tabs(ui);
}

fn draw_overlays(ctx: &egui::Context) {
    // Snapshot under lock, invoke outside.
    let snapshot: Vec<OverlaySnapshot> = {
        let state = STATE.lock();
        state
            .overlays
            .iter()
            .filter_map(|o| {
                let visible = state.visible.get(&o.id).copied().unwrap_or(true);
                if !visible {
                    return None;
                }
                let pos = state.fixed_pos.get(&o.id).copied();
                Some(OverlaySnapshot {
                    id: o.id.clone(),
                    chromeless: o.chromeless,
                    fixed: o.fixed,
                    callback: o.callback,
                    userdata: o.userdata,
                    fixed_pos: pos,
                })
            })
            .collect()
    };

    for snap in snapshot {
        let egui_id = egui::Id::new("honse-overlay").with(&snap.id);
        if snap.chromeless {
            let mut area = egui::Area::new(egui_id).interactable(true);
            if snap.fixed {
                if let Some(pos) = snap.fixed_pos {
                    area = area.fixed_pos(pos);
                } else {
                    area = area.fixed_pos(egui::pos2(16.0, 16.0));
                }
            } else {
                area = area.default_pos(egui::pos2(16.0, 16.0));
            }
            area.show(ctx, |ui| {
                // SAFETY: userdata is the pointer the plugin registered; valid for process life.
                (snap.callback)(ui as *mut egui::Ui as *mut c_void, snap.userdata as *mut c_void);
            });
        } else {
            let title = display_title(&snap.id);
            let mut window = egui::Window::new(title)
                .id(egui_id)
                .default_pos(egui::pos2(32.0, 32.0))
                .resizable(true)
                .collapsible(true);
            if snap.fixed {
                window = window.movable(false);
                if let Some(pos) = snap.fixed_pos {
                    window = window.current_pos(pos);
                }
            }
            window.show(ctx, |ui| {
                // SAFETY: userdata is the pointer the plugin registered; valid for process life.
                (snap.callback)(ui as *mut egui::Ui as *mut c_void, snap.userdata as *mut c_void);
            });
        }
    }
}

fn draw_tabs(ui: &mut egui::Ui) {
    let (titles, selected, draw): (Vec<String>, usize, Option<(GuiDrawFn, usize)>) = {
        let state = STATE.lock();
        if state.tabs.is_empty() {
            return;
        }
        let selected = state.selected_tab.min(state.tabs.len().saturating_sub(1));
        let titles = state.tabs.iter().map(|t| t.title.clone()).collect();
        let draw = state.tabs.get(selected).map(|t| (t.callback, t.userdata));
        (titles, selected, draw)
    };

    ui.horizontal(|ui| {
        for (i, title) in titles.iter().enumerate() {
            if ui.selectable_label(i == selected, title).clicked() {
                STATE.lock().selected_tab = i;
            }
        }
    });
    ui.separator();
    if let Some((callback, userdata)) = draw {
        // SAFETY: userdata is the pointer the plugin registered; valid for process life.
        callback(ui as *mut egui::Ui as *mut c_void, userdata as *mut c_void);
    }
}

fn display_title(id: &str) -> String {
    id.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn push_overlay(id: String, flags: u64, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    ensure();
    let handle = next_handle();
    STATE.lock().overlays.push(OverlayEntry {
        handle,
        id,
        chromeless: flags & overlay_flags::CHROMELESS != 0,
        fixed: flags & overlay_flags::FIXED != 0,
        callback,
        userdata: userdata as usize,
    });
    handle
}

// ── Public registry API (compat Sdk names) ──

pub fn register_panel(id: &str, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    push_overlay(id.to_owned(), 0, callback, userdata)
}

pub fn register_overlay(id: &str, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    push_overlay(id.to_owned(), 0, callback, userdata)
}

pub fn register_panel_chromeless(id: &str, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    push_overlay(id.to_owned(), overlay_flags::CHROMELESS, callback, userdata)
}

pub fn register_panel_chromeless_fixed(id: &str, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    push_overlay(
        id.to_owned(),
        overlay_flags::CHROMELESS | overlay_flags::FIXED,
        callback,
        userdata,
    )
}

/// Register a top-level tab drawn inside the surface window's tab strip.
pub fn register_tab(title: &str, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    ensure();
    let handle = next_handle();
    STATE.lock().tabs.push(TabEntry {
        handle,
        title: title.to_owned(),
        callback,
        userdata: userdata as usize,
    });
    handle
}

/// Alias of [`register_tab`] for compat `register_page` (fork routes pages to menu
/// sections; here pages are surface tabs since edge has no Control Center shell).
pub fn register_page(title: &str, callback: GuiDrawFn, userdata: *mut c_void) -> u64 {
    register_tab(title, callback, userdata)
}

/// Menu section passthrough → edge-sdk wrapper (never raw get_api).
pub fn register_menu_section(mut draw: impl FnMut(&mut egui::Ui) + Send + 'static) -> bool {
    edge_sdk::register_menu_section(move |ui| draw(ui))
}

/// Menu section with icon → edge-sdk wrapper.
pub fn register_page_with_icon(
    title: &str,
    icon_uri: Option<&str>,
    icon_bytes: &[u8],
    mut draw: impl FnMut(&mut egui::Ui) + Send + 'static,
) -> bool {
    edge_sdk::register_menu_section_with_icon(title, icon_uri, icon_bytes, move |ui| draw(ui))
}

pub fn register_menu_section_with_icon(
    title: &str,
    icon_uri: Option<&str>,
    icon_bytes: &[u8],
    draw: impl FnMut(&mut egui::Ui) + Send + 'static,
) -> bool {
    register_page_with_icon(title, icon_uri, icon_bytes, draw)
}

/// Fork default: unknown id is visible (`true`).
#[must_use]
pub fn overlay_visible(id: &str) -> bool {
    STATE.lock().visible.get(id).copied().unwrap_or(true)
}

pub fn set_overlay_visible(id: &str, visible: bool) -> bool {
    STATE.lock().visible.insert(id.to_owned(), visible);
    if visible {
        // Overlays render from inside the surface window's contents callback;
        // if the user closed it, showing an overlay must bring it back.
        reopen();
    }
    true
}

/// Set visibility only if `id` has no prior entry (fork `set_overlay_visible_if_unset`).
pub fn set_overlay_visible_if_unset(id: &str, visible: bool) {
    let mut state = STATE.lock();
    state.visible.entry(id.to_owned()).or_insert(visible);
}

/// Alias — race-hud calls this name.
pub fn overlay_set_visible(id: &str, visible: bool) -> bool {
    set_overlay_visible(id, visible)
}

pub fn toggle_overlay(id: &str) -> bool {
    let next = !overlay_visible(id);
    set_overlay_visible(id, next)
}

/// Remove a registration by handle (overlay or tab).
pub fn unregister(handle: u64) -> bool {
    let mut state = STATE.lock();
    let before = state.overlays.len() + state.tabs.len();
    state.overlays.retain(|o| o.handle != handle);
    state.tabs.retain(|t| t.handle != handle);
    state.overlays.len() + state.tabs.len() != before
}

/// Set an explicit position for a fixed chromeless panel (optional helper).
pub fn set_fixed_pos(id: &str, pos: egui::Pos2) {
    STATE.lock().fixed_pos.insert(id.to_owned(), pos);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TEST_LOCK;

    extern "C" fn noop_draw(_ui: *mut c_void, _ud: *mut c_void) {}

    fn clear_registry() {
        let mut state = STATE.lock();
        state.overlays.clear();
        state.tabs.clear();
        state.visible.clear();
        state.fixed_pos.clear();
        state.selected_tab = 0;
    }

    #[test]
    fn watchdog_miss_threshold() {
        assert!(!watchdog_should_reshow(0, 100));
        assert!(!watchdog_should_reshow(10, 11));
        assert!(!watchdog_should_reshow(10, 12));
        assert!(watchdog_should_reshow(10, 13));
        assert!(watchdog_should_reshow(10, 50));
    }

    #[test]
    fn visibility_defaults_true_and_toggle() {
        let _guard = TEST_LOCK.lock();
        clear_registry();
        assert!(overlay_visible("unknown"));
        set_overlay_visible("panel_a", false);
        assert!(!overlay_visible("panel_a"));
        toggle_overlay("panel_a");
        assert!(overlay_visible("panel_a"));
        // Alias
        overlay_set_visible("panel_a", false);
        assert!(!overlay_visible("panel_a"));
        clear_registry();
    }

    #[test]
    fn register_unregister_overlay() {
        let _guard = TEST_LOCK.lock();
        clear_registry();
        // Avoid calling ensure() (needs Sdk) — push directly via register which
        // calls ensure; instead test the registry map in isolation by locking.
        let handle = next_handle();
        STATE.lock().overlays.push(OverlayEntry {
            handle,
            id: "test".into(),
            chromeless: false,
            fixed: false,
            callback: noop_draw,
            userdata: 0,
        });
        assert!(unregister(handle));
        assert!(!unregister(handle));
        clear_registry();
    }

    #[test]
    fn chromeless_flags_encoded() {
        assert_eq!(overlay_flags::CHROMELESS, 1);
        assert_eq!(overlay_flags::FIXED, 2);
        assert_eq!(overlay_flags::CHROMELESS | overlay_flags::FIXED, 3);
    }
}
