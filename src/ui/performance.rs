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
use crate::song_catalog;
use crate::song_plan::Scope;

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

    concert_plan(ui, perf);
}

/// What your plan for this concert still costs.
///
/// Reads [`crate::song_plan`], so it reports the songs you chose in the planner
/// — falling back to uma.guide's defaults for anything you have not decided.
fn concert_plan(ui: &mut egui::Ui, perf: &GrandLivePerformance) {
    // The window comes from the live per-token ceiling, not a turn table —
    // the cap rises between concerts and the game already tells us where we are.
    let Some(window) = song_catalog::window_for_cap(perf.caps.dance) else {
        return; // unknown ceiling: no window, nothing honest to say
    };
    // Remaining, not total: a song already bought is paid for.
    let owned = crate::song_plan::owned_from(perf.owned.iter().filter_map(|s| s.name.as_deref()));

    ui.add_space(8.0);
    let sep = ui.available_rect_before_wrap();
    ui.painter().hline(
        sep.left()..=sep.right(),
        sep.top(),
        egui::Stroke::new(1.0, theme::SEPARATOR),
    );
    ui.add_space(7.0);

    let tokens = perf.tokens.to_vector();
    let settled = plan_block(ui, &concert_name(window), window, &owned, tokens);

    // Everything planned so far is bought, so this concert has nothing left to
    // say and the useful question becomes the next one — start saving now
    // rather than finding out when the cap rises. Only look ahead to a concert
    // that offers songs: the closing Grand Concert raises the cap but adds
    // none, so pointing at it would repeat the block above under a new heading.
    if settled {
        let next = window + 1;
        if song_catalog::has_songs(next) {
            ui.add_space(6.0);
            plan_block(ui, &format!("SAVING FOR {}", concert_name(next)), next, &owned, tokens);
        }
    }
}

/// What to call a concert. The last one has a name rather than a number, and
/// showing "CONCERT 5" there would not match anything on screen.
fn concert_name(window: u8) -> String {
    if song_catalog::has_songs(window) {
        format!("CONCERT {window}")
    } else {
        "GRAND CONCERT".to_string()
    }
}

/// One concert's outstanding plan: heading, the songs, the totals.
///
/// Scoped `Through`, so it carries anything unbought from earlier concerts.
/// Returns whether nothing is outstanding — the caller uses that to decide
/// whether to look ahead.
fn plan_block(
    ui: &mut egui::Ui,
    title: &str,
    window: u8,
    owned: &crate::song_plan::Owned,
    tokens: song_catalog::TokenVector,
) -> bool {
    let scope = Scope::Through(window);
    let outstanding = crate::song_plan::outstanding(scope, owned);
    let required = crate::song_plan::remaining_cost(scope, owned);
    let missing = song_catalog::shortfall(required, tokens);

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .font(theme::text::section())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{}/{} bought",
                    crate::song_plan::owned_count(scope, owned),
                    crate::song_plan::planned_count(scope)
                ))
                .font(theme::text::meta())
                .color(theme::TEXT_FAINT),
            );
        });
    });

    if outstanding.is_empty() {
        summary_row(ui, "", "all bought".to_string(), theme::ACCENT_VALUE);
        return true;
    }

    // The songs the shortfall is *for*. Without these the panel says how much
    // you are missing but not what you are missing it towards, which meant
    // keeping the planner open just to read three lines off it.
    for song in outstanding.iter().take(MAX_OUTSTANDING) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(song.name)
                    .font(theme::text::meta())
                    .color(theme::TEXT_SECONDARY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(super::token_vector_text(song.cost))
                        .font(theme::text::meta())
                        .color(theme::TEXT_FAINT),
                );
            });
        });
    }
    if outstanding.len() > MAX_OUTSTANDING {
        ui.label(
            egui::RichText::new(format!("+{} more", outstanding.len() - MAX_OUTSTANDING))
                .font(theme::text::help())
                .color(theme::TEXT_FAINT),
        );
    }

    // One song's cost is already on the line above it; a total would just
    // repeat it.
    if outstanding.len() > 1 {
        summary_row(ui, "total", super::token_vector_text(required), theme::TEXT);
    }

    if missing.iter().all(|&v| v == 0) {
        summary_row(ui, "", "affordable now".to_string(), theme::ACCENT_VALUE);
    } else {
        summary_row(ui, "short", super::token_vector_text(missing), theme::CAUTION);
    }

    // A single token needing more than that concert's ceiling allows is
    // unreachable however you train — worth saying rather than showing an
    // impossible shortfall.
    if song_catalog::exceeds_cap(required, song_catalog::CONCERT_CAPS[usize::from(window - 1)]) {
        ui.label(
            egui::RichText::new("exceeds that concert's ceiling")
                .font(theme::text::help())
                .color(theme::NEGATIVE),
        );
    }
    false
}

/// Outstanding songs listed before the panel stops and counts the rest.
/// Beyond this the footer is taller than the token list it hangs off.
const MAX_OUTSTANDING: usize = 5;


/// A label on the left and a token vector on the right, aligned with the song
/// rows above so the columns read as one block.
fn summary_row(ui: &mut egui::Ui, label: &str, value: String, colour: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(label)
                .font(theme::text::meta())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).font(theme::text::meta()).color(colour));
        });
    });
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
