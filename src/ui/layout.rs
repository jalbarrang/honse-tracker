//! Where each panel sits, and where that is remembered.
//!
//! # Corner first, pixels second
//!
//! Panels are positioned as a corner plus an inset, not absolute coordinates,
//! so a resolution change moves nothing. The corner is not chosen once and
//! kept: the overlay re-picks the nearest corner every frame of a drag, which
//! is what lets a panel be dragged to any part of the screen while the stored
//! position stays in the form a resolution change can survive.
//!
//! # Persistence
//!
//! Saved to the `layout` section of `honse-tracker.json` once a drag ends, keyed
//! by panel id. A panel id in the file that no longer exists is ignored, and a
//! panel with no entry keeps where it registered — so adding or removing panels
//! never corrupts a saved layout.

use std::collections::BTreeMap;

use honse_services::overlay::{self, Anchor};
use serde::{Deserialize, Serialize};

use super::egui;

/// One panel's saved position: a corner plus an inset from it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Placement {
    pub anchor: String,
    pub x: f32,
    pub y: f32,
}

/// The `layout` section of `honse-tracker.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LayoutSection {
    #[serde(default)]
    pub panels: BTreeMap<String, Placement>,
}

/// Load saved positions and apply them. Call after every panel is registered —
/// a placement for an unregistered panel is silently dropped.
pub fn load_and_apply() {
    let mut applied = 0;
    for (id, placement) in crate::config::read(|file| file.layout.panels.clone()) {
        let Some(anchor) = Anchor::from_name(&placement.anchor) else {
            hlog_warn!(target: "training-tracker", "Overlay layout: unknown anchor {:?} for '{id}'", placement.anchor);
            continue;
        };
        // `set_placement` is a no-op for an id that was never registered.
        overlay::set_placement(&id, anchor, egui::vec2(placement.x, placement.y));
        applied += 1;
    }
    if applied > 0 {
        hlog_info!(target: "training-tracker", "Overlay layout: {applied} saved position(s) applied");
    }
}

/// Persist a panel the mouse just finished dragging.
///
/// Runs every frame: the overlay moves the panel while the button is down and
/// hands the id over once it is released, because saving belongs to whoever
/// owns the config file.
pub fn flush_drag() {
    let Some(id) = overlay::take_moved_panel() else {
        return;
    };
    if let Some((anchor, offset)) = overlay::placement(id) {
        save(id, anchor, offset);
        hlog_info!(target: "training-tracker", "Layout: '{id}' dragged to {} +{},{}", anchor.name(), offset.x, offset.y);
    }
}

/// Put every panel back where it registered, on screen and on disk.
///
/// The recovery keybinding. Dragging is clamped to the screen, but a saved
/// position can still outlive the screen it was saved on — a smaller window, a
/// different resolution — and nothing on the keyboard moves a panel otherwise.
pub fn reset_all() {
    overlay::reset_all_placements();
    // Drop the entries rather than writing the registered defaults back, so the
    // file returns to meaning "nothing has been moved" — the state a fresh
    // install is in, and the one `load_and_apply` already treats as a no-op.
    crate::config::edit(|file| file.layout.panels.clear());
    hlog_info!(target: "training-tracker", "Layout: every panel reset to its registered position");
}

/// Persist one panel's position.
fn save(id: &str, anchor: Anchor, offset: egui::Vec2) {
    crate::config::edit(|file| {
        file.layout.panels.insert(
            id.to_owned(),
            Placement {
                anchor: anchor.name().to_owned(),
                x: offset.x,
                y: offset.y,
            },
        );
    });
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
    fn a_saved_layout_round_trips_through_json() {
        // The section alone: `config` covers the whole document round-tripping.
        let mut file = LayoutSection::default();
        file.panels.insert(
            "training".to_owned(),
            Placement {
                anchor: Anchor::BottomLeft.name().to_owned(),
                x: 24.0,
                y: 96.0,
            },
        );
        let text = serde_json::to_string(&file).expect("serialize");
        let back: LayoutSection = serde_json::from_str(&text).expect("deserialize");
        let stored = back.panels.get("training").expect("panel kept");
        assert_eq!(Anchor::from_name(&stored.anchor), Some(Anchor::BottomLeft));
        assert!((stored.y - 96.0).abs() < f32::EPSILON);
    }

    #[test]
    fn an_empty_file_parses_to_no_placements() {
        let file: LayoutSection = serde_json::from_str("{}").expect("empty object is valid");
        assert!(file.panels.is_empty());
    }
}
