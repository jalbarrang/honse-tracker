//! Self-hosted egui overlay: panels and windows drawn on OUR egui context,
//! rendered by an in-plugin DX11 renderer from edge's present callback.
//!
//! This replaces the parasitic "surface window" model (`surface.rs`): panels are
//! chromeless `egui::Area`s and config windows are decorated `egui::Window`s
//! with a REAL user-respected close button — no host window is involved, so no
//! egui types ever cross the plugin↔host boundary (no ABI lockstep for UI).
//!
//! Spike findings encoded (`.taskman/plans/spike-own-egui-overlay/FINDINGS.md`):
//! 1. Backbuffer-derived resources (RTV) are created and dropped per Present —
//!    see [`stack`].
//! 2. Critical chords belong in the WndProc; the polling stack owns global
//!    hotkeys. One binding, one owner — see [`stack::register_wndproc_chord`].
//! 3. Transient render errors never disable the overlay (only ~300 consecutive
//!    failures do) — see [`stack`].
//! 4. One stack instance per DLL: these statics are per-DLL because every
//!    plugin statically links its own honse-services copy.
//! 5. The present callback fires before edge's GUI pass, so our UI draws under
//!    edge's menu (correct z-order).
//! 6. DX11 pipeline state is backed up around the render — see `d3d11_state`.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Instant;

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::next_handle;

#[cfg(windows)]
mod d3d11_state;
#[cfg(windows)]
pub mod stack;

#[cfg(windows)]
pub use stack::{register_wndproc_chord, uninstall_wndproc};

/// Debounce window for layout saves: persist this long after the last move.
const LAYOUT_SAVE_DEBOUNCE_SECS: f32 = 2.0;
/// Positions closer than this (points) count as unchanged.
const POS_EPSILON: f32 = 0.5;

/// Draw callback for a panel or window body. Receives a `Ui` from OUR context.
pub type DrawFn = Box<dyn FnMut(&mut egui::Ui) + Send>;

struct Entry {
    handle: u64,
    id: String,
    /// `Some(title)` → decorated `egui::Window` with a close button;
    /// `None` → chromeless draggable `egui::Area`.
    window_title: Option<String>,
    draw: DrawFn,
}

/// On-disk layout: overlay id → `[x, y]` position in points.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Layout {
    #[serde(default)]
    positions: BTreeMap<String, [f32; 2]>,
}

#[derive(Default)]
struct Registry {
    entries: Vec<Entry>,
    /// Visibility keyed by id. Unknown id → hidden (silent boot).
    visible: BTreeMap<String, bool>,
    layout: Layout,
    layout_path: Option<PathBuf>,
    layout_dirty: bool,
    last_layout_change: Option<Instant>,
}

static REGISTRY: Lazy<Mutex<Registry>> = Lazy::new(|| Mutex::new(Registry::default()));

/// egui memory id for an overlay entry — stable across restarts, ours (never a
/// host window id).
fn egui_area_id(id: &str) -> egui::Id {
    egui::Id::new("honse-own-overlay").with(id)
}

// ───────────────────────────── registration ─────────────────────────────

/// Register a chromeless, draggable panel (`egui::Area`) drawn on our context.
/// Hidden until [`set_visible`] / [`toggle`] shows it. Returns a handle.
pub fn register_panel(id: &str, draw: impl FnMut(&mut egui::Ui) + Send + 'static) -> u64 {
    push_entry(id, None, Box::new(draw))
}

/// Register a decorated window (`egui::Window`) with a REAL close button: the
/// user's [X] hides it and it STAYS hidden until reopened via [`set_visible`]
/// (a "Show <title>" hachimi-menu item is registered automatically) or a hotkey.
pub fn register_window(id: &str, title: &str, draw: impl FnMut(&mut egui::Ui) + Send + 'static) -> u64 {
    let handle = push_entry(id, Some(title.to_owned()), Box::new(draw));
    let reopen_id = id.to_owned();
    if !edge_sdk::register_menu_item(&format!("Show {title}"), move || {
        set_visible(&reopen_id, true);
    }) {
        log::warn!("honse-services: overlay menu item registration failed for {id:?}");
    }
    handle
}

fn push_entry(id: &str, window_title: Option<String>, draw: DrawFn) -> u64 {
    let handle = next_handle();
    let mut reg = REGISTRY.lock();
    reg.entries.push(Entry {
        handle,
        id: id.to_owned(),
        window_title,
        draw,
    });
    handle
}

/// Remove a registration by handle.
pub fn unregister(handle: u64) -> bool {
    let mut reg = REGISTRY.lock();
    let before = reg.entries.len();
    reg.entries.retain(|e| e.handle != handle);
    reg.entries.len() != before
}

// ───────────────────────────── visibility ─────────────────────────────

/// Whether `id` is currently visible. Unknown id → `false` (silent boot).
#[must_use]
pub fn is_visible(id: &str) -> bool {
    REGISTRY.lock().visible.get(id).copied().unwrap_or(false)
}

/// Show or hide an overlay entry. No window machinery involved: a visible panel
/// renders on the next present, independent of every other entry.
pub fn set_visible(id: &str, visible: bool) {
    REGISTRY.lock().visible.insert(id.to_owned(), visible);
}

/// Set visibility only when `id` has no explicit entry yet.
pub fn set_visible_if_unset(id: &str, visible: bool) {
    REGISTRY.lock().visible.entry(id.to_owned()).or_insert(visible);
}

/// Toggle and return the new visibility.
pub fn toggle(id: &str) -> bool {
    let mut reg = REGISTRY.lock();
    let next = !reg.visible.get(id).copied().unwrap_or(false);
    reg.visible.insert(id.to_owned(), next);
    next
}

/// True when at least one registered entry is visible (render gate).
#[must_use]
pub(crate) fn any_visible() -> bool {
    let reg = REGISTRY.lock();
    reg.entries
        .iter()
        .any(|e| reg.visible.get(&e.id).copied().unwrap_or(false))
}

/// True when anything is registered (lazy stack init gate).
#[must_use]
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn has_registrations() -> bool {
    !REGISTRY.lock().entries.is_empty()
}

// ───────────────────────────── drawing ─────────────────────────────

/// Draw every visible entry on `ctx`. Called by the stack inside `Context::run`.
///
/// Entries are moved out of the registry while their callbacks run so a body may
/// call back into this module (visibility toggles, new registrations) without
/// deadlocking.
pub(crate) fn draw_all(ctx: &egui::Context) {
    let (mut entries, visible, positions) = {
        let mut reg = REGISTRY.lock();
        let entries = std::mem::take(&mut reg.entries);
        (entries, reg.visible.clone(), reg.layout.positions.clone())
    };

    let mut closed: Vec<String> = Vec::new();
    for entry in &mut entries {
        if !visible.get(&entry.id).copied().unwrap_or(false) {
            continue;
        }
        let egui_id = egui_area_id(&entry.id);
        let saved = positions.get(&entry.id).map(|p| egui::pos2(p[0], p[1]));
        match &entry.window_title {
            None => {
                let mut area = egui::Area::new(egui_id).interactable(true);
                area = area.default_pos(saved.unwrap_or(egui::pos2(16.0, 16.0)));
                area.show(ctx, |ui| (entry.draw)(ui));
            }
            Some(title) => {
                let mut open = true;
                let mut window = egui::Window::new(title.as_str())
                    .id(egui_id)
                    .resizable(true)
                    .collapsible(true)
                    .open(&mut open);
                if let Some(pos) = saved {
                    window = window.default_pos(pos);
                } else {
                    window = window.default_pos(egui::pos2(60.0, 120.0));
                }
                window.show(ctx, |ui| (entry.draw)(ui));
                if !open {
                    // Real close: respected, stays closed until reopened.
                    closed.push(entry.id.clone());
                }
            }
        }
    }

    let mut reg = REGISTRY.lock();
    // Entries registered while we drew were pushed into the (empty) registry
    // vec; keep them after the originals.
    let new_entries = std::mem::take(&mut reg.entries);
    entries.extend(new_entries);
    reg.entries = entries;
    for id in closed {
        reg.visible.insert(id, false);
    }
}

// ───────────────────────────── layout persistence ─────────────────────────────

/// Point layout persistence at `<edge base dir>/<file_name>` and load any saved
/// positions. Call once from plugin init (via [`crate::InitOptions`]).
pub fn set_layout_file(file_name: &str) {
    let Some(base) = edge_sdk::Sdk::try_get().and_then(|s| s.base_dir()) else {
        log::warn!("honse-services: overlay layout file unavailable (no base dir)");
        return;
    };
    set_layout_path(base.join(file_name));
}

/// Explicit-path variant (tests + callers that already resolved the dir).
pub fn set_layout_path(path: PathBuf) {
    // Flush pending layout changes when the plugin shuts down (DllMain detach
    // dispatches SHUTDOWN before unload).
    static SHUTDOWN_HOOKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if !SHUTDOWN_HOOKED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        extern "C" fn flush_on_shutdown(_event: u32, _data: *const std::ffi::c_void, _ud: *mut std::ffi::c_void) {
            flush_layout();
        }
        crate::events::on(crate::event::SHUTDOWN, flush_on_shutdown, std::ptr::null_mut());
    }
    let layout = match std::fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Layout>(&text) {
            Ok(l) => l,
            Err(e) => {
                log::warn!(
                    "honse-services: overlay layout {} unreadable ({e}); starting fresh",
                    path.display()
                );
                Layout::default()
            }
        },
        Err(_) => Layout::default(), // missing file — first run
    };
    let mut reg = REGISTRY.lock();
    reg.layout = layout;
    reg.layout_path = Some(path);
}

/// Post-frame bookkeeping: harvest current positions from egui memory, mark the
/// layout dirty on movement, and save (debounced). Called by the stack after
/// `Context::run`.
pub(crate) fn after_frame(ctx: &egui::Context) {
    let mut reg = REGISTRY.lock();
    if reg.layout_path.is_none() {
        return;
    }
    let ids: Vec<String> = reg.entries.iter().map(|e| e.id.clone()).collect();
    for id in ids {
        let Some(rect) = ctx.memory(|m| m.area_rect(egui_area_id(&id))) else {
            continue;
        };
        let pos = [rect.min.x, rect.min.y];
        if !pos.iter().all(|v| v.is_finite()) {
            continue;
        }
        let changed = match reg.layout.positions.get(&id) {
            Some(old) => (old[0] - pos[0]).abs() > POS_EPSILON || (old[1] - pos[1]).abs() > POS_EPSILON,
            None => true,
        };
        if changed {
            reg.layout.positions.insert(id, pos);
            reg.layout_dirty = true;
            reg.last_layout_change = Some(Instant::now());
        }
    }

    let due = reg.layout_dirty
        && reg
            .last_layout_change
            .is_some_and(|t| t.elapsed().as_secs_f32() >= LAYOUT_SAVE_DEBOUNCE_SECS);
    if due {
        save_layout_locked(&mut reg);
    }
}

/// Persist the layout immediately if dirty (e.g. on shutdown).
pub fn flush_layout() {
    let mut reg = REGISTRY.lock();
    if reg.layout_dirty {
        save_layout_locked(&mut reg);
    }
}

fn save_layout_locked(reg: &mut Registry) {
    let Some(path) = reg.layout_path.clone() else {
        return;
    };
    match serde_json::to_string_pretty(&reg.layout) {
        Ok(text) => {
            if let Err(e) = std::fs::write(&path, text + "\n") {
                log::warn!("honse-services: overlay layout save failed ({}): {e}", path.display());
            } else {
                reg.layout_dirty = false;
            }
        }
        Err(e) => log::warn!("honse-services: overlay layout serialize failed: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TEST_LOCK;

    fn clear() {
        let mut reg = REGISTRY.lock();
        *reg = Registry::default();
    }

    #[test]
    fn visibility_defaults_hidden_and_toggles() {
        let _guard = TEST_LOCK.lock();
        clear();
        assert!(!is_visible("unknown"));
        assert!(toggle("p"));
        assert!(is_visible("p"));
        assert!(!toggle("p"));
        set_visible_if_unset("p", true); // explicit entry exists → no-op
        assert!(!is_visible("p"));
        set_visible_if_unset("q", true);
        assert!(is_visible("q"));
        clear();
    }

    #[test]
    fn register_unregister_and_any_visible() {
        let _guard = TEST_LOCK.lock();
        clear();
        let h = register_panel("panel_x", |_ui| {});
        assert!(has_registrations());
        assert!(!any_visible());
        set_visible("panel_x", true);
        assert!(any_visible());
        assert!(unregister(h));
        assert!(!unregister(h));
        assert!(!has_registrations());
        clear();
    }

    #[test]
    fn layout_round_trip_via_path() {
        let _guard = TEST_LOCK.lock();
        clear();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");

        {
            let mut reg = REGISTRY.lock();
            reg.layout_path = Some(path.clone());
            reg.layout.positions.insert("a".into(), [10.0, 20.0]);
            reg.layout_dirty = true;
        }
        flush_layout();
        assert!(path.is_file());
        assert!(!REGISTRY.lock().layout_dirty);

        clear();
        set_layout_path(path);
        let reg = REGISTRY.lock();
        assert_eq!(reg.layout.positions.get("a"), Some(&[10.0, 20.0]));
        drop(reg);
        clear();
    }

    #[test]
    fn corrupt_layout_starts_fresh() {
        let _guard = TEST_LOCK.lock();
        clear();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.json");
        std::fs::write(&path, "{nope").unwrap();
        set_layout_path(path);
        assert!(REGISTRY.lock().layout.positions.is_empty());
        clear();
    }
}
