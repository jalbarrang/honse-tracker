//! Floating overlay for view-transition diagnostics (egui).

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};

use edge_sdk::{egui, ui_from_ptr};
use honse_ui::components;
use honse_ui::theme::Tokens;

const OVERLAY_ID: &str = "debug_viewer";
const OVERLAY_MIN_WIDTH: f32 = 260.0;

/// Register the debug overlay panel with the host GUI.
pub fn register_ui() {
    let handle = honse_services::register_panel(OVERLAY_ID, draw_overlay, std::ptr::null_mut());

    if handle == 0 {
        hlog_warn!(target: "debug-viewer", "Overlay panel registration declined by host");
    } else {
        hlog_info!(target: "debug-viewer", "Overlay panel registered ({})", handle);
    }

    honse_services::register_hotkey(
        "debug-viewer.toggle",
        "Toggle Debug Window",
        0,
        0,
        toggle_overlay_hotkey,
        std::ptr::null_mut(),
    );
}

extern "C" fn toggle_overlay_hotkey(_userdata: *mut c_void) {
    if panic::catch_unwind(|| honse_services::toggle_overlay(OVERLAY_ID)).is_err() {
        hlog_error!(target: "debug-viewer", "toggle_overlay_hotkey panicked");
    }
}

extern "C" fn draw_overlay(ui: *mut c_void, _userdata: *mut c_void) {
    // SAFETY: the host passes a valid `&mut egui::Ui` pointer for this callback.
    let ui = unsafe { ui_from_ptr(ui) };
    if panic::catch_unwind(AssertUnwindSafe(|| draw_overlay_inner(ui))).is_err() {
        hlog_error!(target: "debug-viewer", "draw_overlay panicked");
    }
}

fn draw_overlay_inner(ui: &mut egui::Ui) {
    ui.set_min_width(OVERLAY_MIN_WIDTH);

    let snapshot = crate::state::snapshot();
    let current = format_view(snapshot.current_view_id);
    let previous = format_view(snapshot.previous_view_id);
    let sequence = snapshot.sequence.to_string();
    let tokens = Tokens::DEFAULT;

    components::window_chrome(ui, "Debug Viewer", |ui| {
        ui.monospace(format!("Current view:  {current}"));
        ui.monospace(format!("Previous view: {previous}"));
        ui.monospace(format!("Transitions:   {sequence}"));

        let _ = components::separator(ui);
        ui.label(egui::RichText::new("Recent view changes").color(tokens.fg));

        if snapshot.history.is_empty() {
            ui.label(
                egui::RichText::new("No VIEW_CHANGE events observed yet.")
                    .color(tokens.fg_dim)
                    .size(12.0),
            );
        } else {
            for entry in snapshot.history.iter().rev() {
                let row = format!(
                    "#{}  {:.1}s  {}",
                    entry.sequence,
                    entry.seconds_since_start,
                    format_view(Some(entry.view_id))
                );
                ui.label(egui::RichText::new(row).monospace().size(12.0));
            }
        }
    });
}

fn format_view(view_id: Option<i32>) -> String {
    view_id.map_or_else(
        || "—".to_owned(),
        |id| match honse_services::view_name(id) {
            Some(name) => format!("{id} ({name})"),
            None => id.to_string(),
        },
    )
}
