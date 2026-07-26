//! Unified "Career" overlay panel — an egui port of the honse-tracker dashboard
//! `CareerPanel`: a single scrolling view stacking the trainee header, the
//! Training table, Bonds, Skills, and Conditions, styled with [`theme`].
//!
//! Sections are added incrementally (career-overlay-port t-005..t-008); for now
//! only the theme primitives exist.

mod bonds;
mod bonds_table;
mod header;
// Skills section is hidden for now (its draw call is disabled below); the module
// is kept compiling until it's re-enabled.
#[allow(dead_code)]
mod skills;
mod theme;
mod training;

use crate::compat::egui;

use super::overlay_panels as overlay;
use crate::memory_reader::CareerSnapshot;

pub(super) fn draw_energy_panel(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    header::energy_standalone(ui, snap);
}

pub(super) fn draw_rank_panel(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    header::rank_standalone(ui, snap);
}

pub(super) fn draw_training_panel(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    overlay::scroll_list(ui, |ui| {
        training::draw(ui, snap);
    });
}

pub(super) fn draw_bonds_panel(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    overlay::scroll_list(ui, |ui| bonds::draw(ui, snap));
}
