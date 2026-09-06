//! One button: hand this run to torena-hub's Skill Planner.
//!
//! # Only where it means something
//!
//! It draws on the two screens that spend skill points — the Skills Shop and
//! the pre-complete summary — and nowhere else. A planner link built anywhere
//! else would still be *correct*, but a button that is always on screen is a
//! button you stop reading; this one appears exactly when the decision it
//! helps with is in front of you.
//!
//! # Visibility is decided before the frame, not during it
//!
//! An interactive panel is one the game cannot be clicked through, so the flag
//! has to follow visibility rather than be set once. It cannot be set from
//! [`draw`], though: the overlay holds its panel list locked for the whole
//! paint, so a panel asking to become interactive mid-draw would deadlock the
//! render thread on a lock it is already inside. So [`sync`] runs as a frame
//! job — before the paint, like the hotkey poll — and [`draw`] renders what it
//! settled. The song planner reaches the same place from the other direction:
//! its flag is written by the hotkey that opens it (see [`super::plan`]).
//!
//! # What the numbers are
//!
//! The balance and stats come from the shop screen's light refresh, so they
//! move as you buy. Hints and the obtained list are only re-read when the
//! button is pressed, which is the moment they matter — see
//! [`crate::planner_export`].

use std::sync::atomic::{AtomicBool, Ordering};

use honse_services::overlay::theme;

use super::{egui, with_snapshot, Face};
use crate::read_gate::View;

/// Whether the panel is on screen this frame, settled by [`sync`].
static SHOWING: AtomicBool = AtomicBool::new(false);

/// Frame job: decide whether the panel is on screen, and give the mouse to
/// whichever of the panel and the game should have it.
pub fn sync() {
    let showing = should_show();
    if SHOWING.swap(showing, Ordering::AcqRel) != showing {
        honse_services::overlay::set_panel_interactive("planner", showing);
    }
}

pub fn draw(ui: &mut egui::Ui) {
    if !SHOWING.load(Ordering::Acquire) {
        return;
    }
    let face = super::refreshed_face();
    with_snapshot(|snapshot| {
        honse_services::overlay::chrome(ui, |ui| body(ui, snapshot.skill_point, face));
    });
}

/// The two screens where leftover skill points get spent, and only inside a
/// career the panels are allowed to paint for.
fn should_show() -> bool {
    if !super::refreshed_face().visible() {
        return false;
    }
    let view = View::from_id(honse_services::current_view_id());
    if !matches!(view, View::CareerSkillsShop | View::CareerPreComplete) {
        return false;
    }
    with_snapshot(|snapshot| snapshot.is_playing).unwrap_or(false)
}

fn body(ui: &mut egui::Ui, skill_points: i32, face: Face) {
    ui.set_opacity(face.opacity());

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("SKILL PLANNER")
                .font(theme::text::panel_title())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{skill_points} SP"))
                    .font(theme::text::meta())
                    .color(theme::ACCENT_VALUE),
            );
        });
    });
    ui.add_space(6.0);

    if ui
        .button(egui::RichText::new("Load in torena-hub Skill Planner").font(theme::text::row_label()))
        .clicked()
    {
        crate::planner_export::request();
    }

    ui.add_space(3.0);
    ui.label(
        egui::RichText::new("Opens your browser; the link is copied too.")
            .font(theme::text::help())
            .color(theme::TEXT_FAINT),
    );
}
