//! Scenario tab: scenario-specific readout.

mod trackblazer;

use crate::compat::egui;

use crate::memory_reader::{self, CareerSnapshot};

pub(super) fn draw(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    match &snap.scenario_state {
        Some(memory_reader::ScenarioState::Trackblazer(shop)) => trackblazer::draw(ui, shop),
        None => {
            ui.small("No scenario-specific data for this run.");
            ui.small("(Supported: Trackblazer / Make a New Track.)");
        }
    }
}
