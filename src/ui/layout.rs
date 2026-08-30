//! Layout mode — move panels without a mouse.
//!
//! The design called for dragging. Dragging needs pointer events, which need
//! the WndProc hook we have not built, so this is the keyboard form of the same
//! idea: pick a panel, send it to a corner, nudge it from there.
//!
//! # Corner first, pixels second
//!
//! Panels are positioned as a corner plus an inset, not absolute coordinates,
//! so a resolution change moves nothing. That shape makes coarse placement one
//! keypress — cycle the anchor — and leaves the arrows for fine adjustment.
//! Nudging alone would mean fifty presses to cross the screen; the hotkeys
//! auto-repeat when held, which covers the rest.
//!
//! # Persistence
//!
//! Saved to `overlayLayout.json` on every change, keyed by panel id. A panel id
//! in the file that no longer exists is ignored, and a panel with no entry
//! keeps where it registered — so adding or removing panels never corrupts a
//! saved layout.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use honse_services::overlay::{self, Anchor};
use honse_services::PluginConfig;
use serde::{Deserialize, Serialize};

use super::egui;

/// Pixels one arrow press moves a panel.
const NUDGE: f32 = 4.0;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Placement {
    anchor: String,
    x: f32,
    y: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutFile {
    #[serde(default)]
    panels: BTreeMap<String, Placement>,
}

static LAYOUT: Mutex<Option<PluginConfig<LayoutFile>>> = Mutex::new(None);
/// Index into `overlay::panel_ids()` while layout mode is on.
static SELECTED: AtomicUsize = AtomicUsize::new(0);

/// Load saved positions and apply them. Call after every panel is registered —
/// a placement for an unregistered panel is silently dropped.
pub fn load_and_apply() {
    let Some(config) = PluginConfig::<LayoutFile>::load("overlayLayout.json") else {
        hlog_warn!(target: "training-tracker", "Overlay layout: no base dir; positions will not persist");
        return;
    };
    let mut applied = 0;
    for (id, placement) in &config.value.panels {
        let Some(anchor) = Anchor::from_name(&placement.anchor) else {
            hlog_warn!(target: "training-tracker", "Overlay layout: unknown anchor {:?} for '{id}'", placement.anchor);
            continue;
        };
        // `set_placement` is a no-op for an id that was never registered.
        overlay::set_placement(id, anchor, egui::vec2(placement.x, placement.y));
        applied += 1;
    }
    if applied > 0 {
        hlog_info!(target: "training-tracker", "Overlay layout: {applied} saved position(s) applied");
    }
    *LAYOUT.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(config);
}

/// Whether layout mode is on.
#[must_use]
pub fn is_active() -> bool {
    overlay::layout_selection().is_some()
}

/// The panel currently being edited.
fn selected_id() -> Option<&'static str> {
    let ids = overlay::panel_ids();
    if ids.is_empty() {
        return None;
    }
    Some(ids[SELECTED.load(Ordering::Acquire) % ids.len()])
}

/// Enter or leave layout mode.
pub fn toggle() {
    if is_active() {
        overlay::set_layout_selection(None);
        hlog_info!(target: "training-tracker", "Layout mode: off");
        return;
    }
    SELECTED.store(0, Ordering::Release);
    let Some(id) = selected_id() else {
        hlog_warn!(target: "training-tracker", "Layout mode: no panels registered");
        return;
    };
    overlay::set_layout_selection(Some(id));
    hlog_info!(target: "training-tracker", "Layout mode: on \u{2014} editing '{id}'");
}

/// Move to the next panel, wrapping.
pub fn select_next() {
    if !is_active() {
        return;
    }
    let ids = overlay::panel_ids();
    if ids.is_empty() {
        return;
    }
    let next = (SELECTED.load(Ordering::Acquire) + 1) % ids.len();
    SELECTED.store(next, Ordering::Release);
    overlay::set_layout_selection(Some(ids[next]));
}

/// Send the selected panel to the next corner, keeping its inset.
pub fn cycle_anchor() {
    let Some(id) = active_target() else {
        return;
    };
    if let Some((anchor, offset)) = overlay::placement(id) {
        let next = anchor.next();
        overlay::set_placement(id, next, offset);
        save(id, next, offset);
        hlog_info!(target: "training-tracker", "Layout: '{id}' \u{2192} {}", next.name());
    }
}

/// Nudge the selected panel. Deltas are in inset space, so `+x` always means
/// "further from its own corner" regardless of which corner that is.
pub fn nudge(dx: f32, dy: f32) {
    let Some(id) = active_target() else {
        return;
    };
    if let Some((anchor, offset)) = overlay::placement(id) {
        let moved = egui::vec2(offset.x + dx * NUDGE, offset.y + dy * NUDGE);
        overlay::set_placement(id, anchor, moved);
        // Read back: `set_placement` clamps, so this stores what actually took.
        if let Some((_, applied)) = overlay::placement(id) {
            save(id, anchor, applied);
        }
    }
}

/// Put the selected panel back where it was registered.
pub fn reset_selected() {
    let Some(id) = active_target() else {
        return;
    };
    overlay::reset_placement(id);
    if let Some((anchor, offset)) = overlay::placement(id) {
        save(id, anchor, offset);
    }
    hlog_info!(target: "training-tracker", "Layout: '{id}' reset");
}

fn active_target() -> Option<&'static str> {
    if is_active() { selected_id() } else { None }
}

/// Persist one panel's position.
fn save(id: &str, anchor: Anchor, offset: egui::Vec2) {
    let mut guard = LAYOUT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(config) = guard.as_mut() else {
        return;
    };
    config.value.panels.insert(
        id.to_owned(),
        Placement {
            anchor: anchor.name().to_owned(),
            x: offset.x,
            y: offset.y,
        },
    );
    if let Err(e) = config.save() {
        hlog_warn!(target: "training-tracker", "Overlay layout: save failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchor_names_round_trip() {
        for anchor in Anchor::ALL {
            assert_eq!(Anchor::from_name(anchor.name()), Some(anchor), "{anchor:?}");
        }
        assert_eq!(Anchor::from_name("middle"), None);
    }

    #[test]
    fn cycling_anchors_visits_all_four_and_returns() {
        let mut anchor = Anchor::TopLeft;
        let mut seen = vec![anchor];
        for _ in 0..3 {
            anchor = anchor.next();
            seen.push(anchor);
        }
        assert_eq!(seen.len(), 4);
        assert_eq!(anchor.next(), Anchor::TopLeft, "cycles back round");
        for a in Anchor::ALL {
            assert!(seen.contains(&a), "{a:?} was skipped");
        }
    }

    #[test]
    fn a_saved_layout_round_trips_through_json() {
        let mut file = LayoutFile::default();
        file.panels.insert(
            "training".to_owned(),
            Placement {
                anchor: Anchor::BottomLeft.name().to_owned(),
                x: 24.0,
                y: 96.0,
            },
        );
        let text = serde_json::to_string(&file).expect("serialize");
        let back: LayoutFile = serde_json::from_str(&text).expect("deserialize");
        let stored = back.panels.get("training").expect("panel kept");
        assert_eq!(Anchor::from_name(&stored.anchor), Some(Anchor::BottomLeft));
        assert!((stored.y - 96.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_empty_file_parses_to_no_placements() {
        let file: LayoutFile = serde_json::from_str("{}").expect("empty object is valid");
        assert!(file.panels.is_empty());
    }
}
