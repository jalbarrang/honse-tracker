//! Debug windows on the plugin's self-hosted egui context.
//!
//! Two own-context windows, both with real close buttons and hachimi-menu
//! reopen items:
//! - "Debug Viewer": view-transition diagnostics (unbound polling hotkey).
//! - "Overlay Demo": interactive widgets exercising the productized stack
//!   (input, text entry, frame cost). Alt+9 toggles it via a WndProc-owned
//!   chord — the chord lives ONLY there (one binding, one owner), proving
//!   guaranteed delivery at the head of the subclass chain.

use std::ffi::c_void;
use std::panic::{self, AssertUnwindSafe};
use std::time::Instant;

use edge_sdk::egui;
use honse_services::overlay;
use honse_ui::theme::Tokens;
use parking_lot::Mutex;

const VIEWER_ID: &str = "debug_viewer";
const DEMO_ID: &str = "debug_overlay_demo";
const OVERLAY_MIN_WIDTH: f32 = 260.0;

/// Register the debug windows + toggles.
pub fn register_ui() {
    overlay::register_window(VIEWER_ID, "Debug Viewer", |ui| {
        if panic::catch_unwind(AssertUnwindSafe(|| draw_viewer_inner(ui))).is_err() {
            hlog_error!(target: "debug-viewer", "draw_viewer panicked");
        }
    });

    honse_services::register_hotkey(
        "debug-viewer.toggle",
        "Toggle Debug Window",
        0,
        0,
        toggle_viewer_hotkey,
        std::ptr::null_mut(),
    );

    register_demo_window();
}

extern "C" fn toggle_viewer_hotkey(_userdata: *mut c_void) {
    if panic::catch_unwind(|| overlay::toggle(VIEWER_ID)).is_err() {
        hlog_error!(target: "debug-viewer", "toggle_viewer_hotkey panicked");
    }
}

fn draw_viewer_inner(ui: &mut egui::Ui) {
    ui.set_min_width(OVERLAY_MIN_WIDTH);

    let snapshot = crate::state::snapshot();
    let current = format_view(snapshot.current_view_id);
    let previous = format_view(snapshot.previous_view_id);
    let sequence = snapshot.sequence.to_string();
    let tokens = Tokens::DEFAULT;

    ui.monospace(format!("Current view:  {current}"));
    ui.monospace(format!("Previous view: {previous}"));
    ui.monospace(format!("Transitions:   {sequence}"));

    ui.separator();
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

// ───────────────────────── overlay demo (stack exerciser) ─────────────────────────

#[derive(Default)]
struct DemoState {
    text: String,
    clicks: u32,
    checked: bool,
}

static DEMO: Mutex<DemoState> = Mutex::new(DemoState {
    text: String::new(),
    clicks: 0,
    checked: false,
});

fn register_demo_window() {
    let mut last_frame: Option<Instant> = None;
    overlay::register_window(DEMO_ID, "Overlay Demo", move |ui| {
        let dt = last_frame.map(|t| t.elapsed().as_secs_f32() * 1000.0);
        last_frame = Some(Instant::now());
        let mut demo = DEMO.lock();
        ui.label("Rendered by honse-debug's OWN egui —");
        ui.label("no host egui, no ABI lockstep, real close semantics.");
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Click me").clicked() {
                demo.clicks += 1;
            }
            let clicks = demo.clicks;
            ui.label(format!("clicks: {clicks}"));
        });
        ui.checkbox(&mut demo.checked, "A checkbox");
        ui.horizontal(|ui| {
            ui.label("Text input:");
            ui.text_edit_singleline(&mut demo.text);
        });
        ui.separator();
        if let Some(dt) = dt {
            ui.small(format!("frame-to-frame: {dt:.1} ms — Alt+9 hides me"));
        }
    });

    // Alt+9, WndProc-owned (guaranteed delivery even when egui has keyboard
    // focus). NOT registered with the polling stack — one binding, one owner.
    #[cfg(windows)]
    overlay::register_wndproc_chord(honse_services::MOD_ALT, 0x39, || {
        let now = overlay::toggle(DEMO_ID);
        hlog_info!(target: "debug-viewer", "overlay demo visible={now}");
    });
}
