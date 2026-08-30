//! What you can actually take on the Lessons tree — Grand Live only.
//!
//! The game shows three squares at a time behind a scroll, each with a row of
//! five cost icons, and leaves the arithmetic to you. This panel lists every
//! square on offer at once, in the game's own order, and says what the ones you
//! cannot take are short by.
//!
//! # Affordability is the game's answer, not ours
//!
//! `affordable` comes from `CanGetTreeSquare(squareId)`. Recomputing it by
//! comparing cost against tokens would look right and drift: the game also
//! knows about prerequisites and per-turn limits that we have not modelled. The
//! shortfall *is* ours, because nothing exposes it — but it is only ever shown
//! for squares the game has already said no to.
//!
//! # Freshness
//!
//! It renders the cached capture like every panel, but on the Techniques Shop a
//! light refresh re-reads stats, energy and the scenario slice a few times a
//! second, so a purchase shows up immediately. `refreshed_face` reports that as
//! `Live`.

use honse_services::overlay::theme;

use super::{egui, with_snapshot, Face};
use crate::memory_reader::{GrandLivePerformance, GrandLiveSquare, PerformanceTokens, ScenarioState};

/// Squares listed before the panel gives up and says how many more there are.
/// The tree can offer more than fits a readable panel.
const MAX_ROWS: usize = 8;

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
            return;
        };
        if perf.squares.is_empty() {
            return; // no tree on offer — nothing to weigh up
        }
        honse_services::overlay::chrome(ui, |ui| body(ui, perf, face));
    });
}

fn body(ui: &mut egui::Ui, perf: &GrandLivePerformance, face: Face) {
    ui.set_opacity(face.opacity());

    let affordable = perf.squares.iter().filter(|s| s.affordable).count();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("LESSONS")
                .font(theme::text::panel_title())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("{affordable}/{} affordable", perf.squares.len()))
                    .font(theme::text::meta())
                    .color(if affordable > 0 { theme::ACCENT } else { theme::TEXT_FAINT }),
            );
        });
    });
    ui.add_space(6.0);

    // The reader returns the game's own order (`GetSortId`), and it is kept:
    // the panel sits beside the list it describes, so row N here must be row N
    // there. Sorting by affordability read better in isolation and made the two
    // impossible to cross-reference.
    for square in perf.squares.iter().take(MAX_ROWS) {
        row(ui, square, &perf.tokens);
    }
    if perf.squares.len() > MAX_ROWS {
        ui.add_space(3.0);
        ui.label(
            egui::RichText::new(format!("+{} more", perf.squares.len() - MAX_ROWS))
                .font(theme::text::help())
                .color(theme::TEXT_FAINT),
        );
    }
}

fn row(ui: &mut egui::Ui, square: &GrandLiveSquare, tokens: &PerformanceTokens) {
    let name = square.name.as_deref().unwrap_or("(unnamed square)");
    let title = if square.is_music {
        format!("\u{266a} {name}")
    } else {
        name.to_string()
    };

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(theme::text::row_label())
                .color(if square.affordable {
                    theme::TEXT
                } else {
                    theme::TEXT_MUTED
                }),
        );
    });

    // Cost when you can take it; what you are missing when you cannot.
    let (text, colour) = if square.affordable {
        (cost_text(&square.cost), theme::ACCENT_VALUE)
    } else {
        let missing = tokens.shortfall(&square.cost);
        if missing.is_zero() {
            // Priced within reach but still refused — a prerequisite or a
            // per-turn limit, which we deliberately do not model.
            ("locked".to_string(), theme::TEXT_UNKNOWN)
        } else {
            (format!("need {}", cost_text(&missing)), theme::CAUTION)
        }
    };
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(egui::RichText::new(text).font(theme::text::meta()).color(colour));
    });
    ui.add_space(4.0);
}

/// Non-zero tokens as `Da32 Vi12`. Empty costs render as an em dash.
fn cost_text(tokens: &PerformanceTokens) -> String {
    super::token_vector_text(tokens.to_vector())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(d: i32, p: i32, vo: i32, vi: i32, m: i32) -> PerformanceTokens {
        PerformanceTokens {
            dance: d,
            passion: p,
            vocal: vo,
            visual: vi,
            mental: m,
        }
    }

    #[test]
    fn cost_lists_only_non_zero_tokens() {
        assert_eq!(cost_text(&tokens(32, 0, 0, 12, 0)), "Da32 Vi12");
        assert_eq!(cost_text(&tokens(0, 0, 32, 0, 12)), "Vo32 Co12");
    }

    #[test]
    fn an_empty_cost_is_a_dash_not_a_blank() {
        assert_eq!(cost_text(&tokens(0, 0, 0, 0, 0)), "\u{2014}");
    }

    #[test]
    fn shortfall_counts_only_what_is_missing() {
        let have = tokens(10, 10, 10, 10, 10);
        let cost = tokens(32, 0, 0, 12, 0);
        assert_eq!(have.shortfall(&cost), tokens(22, 0, 0, 2, 0));
    }

    #[test]
    fn covered_costs_leave_nothing_missing() {
        let have = tokens(50, 50, 50, 50, 50);
        assert!(have.shortfall(&tokens(32, 0, 0, 12, 0)).is_zero());
    }
}
