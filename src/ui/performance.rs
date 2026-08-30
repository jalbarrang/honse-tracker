//! Performance tokens — Grand Live only.
//!
//! The five tokens you bank across the run (Dance, Passion, Vocal, Visual,
//! Composure) against the ceiling each one is currently capped at.
//!
//! # Scenario-gated, not scenario-aware
//!
//! This panel draws only when the active run is Grand Live, and it decides that
//! from the snapshot's own `ScenarioState` rather than from a scenario id.
//! Dispatch already happened in the reader; re-deriving it here would be a
//! second place to get the 3-vs-4 id trap wrong.
//!
//! # The cap is not 200
//!
//! It rises as the run progresses, which is why it is read live via
//! `GetPerformanceMax` rather than hardcoded. When it reads as zero the getter
//! did not resolve, and the panel prints the token alone rather than inventing
//! a denominator — the same "unknown is not zero" rule the training panel uses
//! for failure rates.

use honse_services::overlay::theme;

use super::{egui, with_snapshot, Face};
use crate::memory_reader::{GrandLivePerformance, ScenarioState};

/// Draw the panel. Returns without painting anything — chrome included —
/// whenever this is not a Grand Live run with something to show.
pub fn draw(ui: &mut egui::Ui) {
    let face = super::refreshed_face();
    if !face.visible() {
        return;
    }
    with_snapshot(|snapshot| {
        if !snapshot.is_playing {
            return;
        }
        let Some(ScenarioState::GrandLive(perf)) = &snapshot.scenario_state else {
            return; // not Grand Live — this panel has nothing to say
        };
        honse_services::overlay::chrome(ui, |ui| body(ui, perf, face));
    });
}

fn body(ui: &mut egui::Ui, perf: &GrandLivePerformance, face: Face) {
    ui.set_opacity(face.opacity());
    ui.label(
        egui::RichText::new("PERFORMANCE")
            .font(theme::text::panel_title())
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(6.0);

    let caps = perf.caps.labelled();
    for (i, (label, value)) in perf.tokens.labelled().into_iter().enumerate() {
        row(ui, label, value, caps[i].1);
    }
}

/// One token row: name on the left, `value / cap` on the right.
///
/// A token at its ceiling is called out — further gains in it are wasted, which
/// is the one thing on this panel that changes what you do next.
fn row(ui: &mut egui::Ui, label: &str, value: i32, cap: i32) {
    let capped = cap > 0 && value >= cap;
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .font(theme::text::row_label())
                .color(if capped { theme::TEXT } else { theme::TEXT_SECONDARY }),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Unknown ceiling: print the token alone rather than "/ 0".
            if cap > 0 {
                ui.label(
                    egui::RichText::new(format!("/{cap}"))
                        .font(theme::text::unit())
                        .color(theme::TEXT_FAINT),
                );
            }
            ui.label(
                egui::RichText::new(value.to_string())
                    .font(theme::text::value())
                    .color(if capped { theme::CAUTION } else { theme::TEXT }),
            );
        });
    });
}
