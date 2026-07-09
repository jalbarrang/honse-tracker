//! Career snapshot resolution for overlay tabs.

use crate::compat::egui;

use crate::memory_reader;
use crate::overlay_cache;

/// Resolve the live career snapshot, drawing a placeholder when unavailable.
pub(super) fn current_snapshot(ui: &mut egui::Ui) -> Option<memory_reader::CareerSnapshot> {
    overlay_cache::maybe_request_refresh();
    match overlay_cache::snapshot() {
        Some(s) if s.is_playing => Some(s),
        Some(_) => {
            ui.small("\u{1f3cb} No active career");
            None
        }
        None => {
            ui.small("\u{1f3cb} Loading career data…");
            None
        }
    }
}
