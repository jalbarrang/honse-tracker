//! Diagnostic panel: which screen are we on, and what is that doing to the HUD.
//!
//! Built to answer one question that the log answers badly — *why did a panel
//! just disappear?* It shows the raw view id, the name we have for it (if any),
//! and the whole chain that turns that id into a visibility decision:
//!
//! ```text
//! view id -> View -> CareerState -> Face -> painting or not
//! ```
//!
//! # It never hides
//!
//! Unlike every real panel this one ignores [`Face::visible`]. A panel that
//! vanished exactly when you needed to know why it vanished would be useless,
//! and the interesting screens are precisely the ones that hide the others.
//!
//! # Uncatalogued screens are the point
//!
//! A view id with no row in `scene_views` becomes [`View::Unknown`], whose
//! policy fails closed and hides every panel. So an unknown screen is not a
//! cosmetic gap — it is a silent HUD blackout. Those are called out in the
//! caution colour, and that is the to-do list for the `VIEWS` table.

use std::sync::atomic::{AtomicBool, Ordering};

use honse_services::overlay::theme;

use super::{egui, Face};
use crate::read_gate::View;

/// Whether the panel paints. Also drives the view poll, so that turning the
/// panel on is enough to start observing ids outside a career.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Show or hide the panel, and hold the view poll open to match.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
    honse_services::set_view_poll_hold(enabled);
    hlog_info!(target: "training-tracker", "Debug overlay: {}", if enabled { "on" } else { "off" });
}

/// Whether the panel is painting.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Flip the panel, for the menu item.
pub fn toggle() {
    set_enabled(!is_enabled());
}

pub fn draw(ui: &mut egui::Ui) {
    if !is_enabled() {
        return; // no chrome either — see `overlay::chrome`
    }
    honse_services::overlay::chrome(ui, body);
}

fn body(ui: &mut egui::Ui) {
    let view_id = honse_services::current_view_id();
    let view = View::from_id(view_id);
    let state = crate::career_poll::current_lifecycle_state();
    let face = Face::of(state);

    ui.label(
        egui::RichText::new("SCREEN \u{00b7} DEBUG")
            .font(theme::text::panel_title())
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(6.0);

    // The view id, and whether we have a name for it. `-1` means the poll has
    // not run: nothing is tracking and nothing is holding it open.
    if view_id < 0 {
        row(ui, "view", "\u{2014} (poll idle)", theme::TEXT_UNKNOWN);
    } else {
        row(ui, "view", &view_id.to_string(), theme::TEXT);
        match view.label() {
            Some(label) => row(ui, "screen", label, theme::TEXT_SECONDARY),
            None => row(ui, "screen", "UNKNOWN - add to scene_views", theme::CAUTION),
        }
    }

    ui.add_space(4.0);
    separator(ui);
    ui.add_space(4.0);

    row(ui, "state", &format!("{state:?}"), theme::TEXT_SECONDARY);

    let (face_text, face_colour) = match face {
        Face::Live => ("Live \u{00b7} panels painting".to_string(), theme::ACCENT_VALUE),
        Face::Holding => ("Holding \u{00b7} panels dimmed".to_string(), theme::TEXT_HOLDING),
        Face::Away | Face::Off => (format!("{face:?} \u{00b7} panels hidden"), theme::NEGATIVE),
    };
    row(ui, "face", &face_text, face_colour);

    match super::with_snapshot(|s| s.current_turn) {
        Some(turn) => row(ui, "snapshot", &format!("turn {turn}"), theme::TEXT_SECONDARY),
        None => row(ui, "snapshot", "none cached", theme::TEXT_UNKNOWN),
    }

    // Whether overlay chords are being consumed or leaking to the game. The
    // whole point of the subclass, and invisible without saying so.
    let hooked = honse_services::input_block::is_installed();
    row(
        ui,
        "keys",
        if hooked { "consumed" } else { "LEAKING to game" },
        if hooked { theme::TEXT_SECONDARY } else { theme::NEGATIVE },
    );
}

/// One `label  ······  value` line, value right-aligned so the column scans.
fn row(ui: &mut egui::Ui, label: &str, value: &str, colour: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .font(theme::text::meta())
                .color(theme::TEXT_FAINT),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).font(theme::text::meta()).color(colour));
        });
    });
}

fn separator(ui: &mut egui::Ui) {
    let r = ui.available_rect_before_wrap();
    ui.painter()
        .hline(r.left()..=r.right(), r.top(), egui::Stroke::new(1.0, theme::SEPARATOR));
}
