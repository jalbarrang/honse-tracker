//! The training board — five facilities, and the trade each one is asking for.
//!
//! Everything on it comes from one `CareerSnapshot`: `training_levels`,
//! `per_stat_gains`, `failure_rates`, and `hp`. It does not recommend. It
//! highlights the largest total gain and puts the cost beside it; the trade
//! stays the player's.
//!
//! # Freshness is not uniform
//!
//! On a shop screen the light refresh keeps `hp` current, so the panel paints
//! at full opacity there. The projections — gains and failure rates — are not
//! part of that refresh, because nothing re-derives them while you are
//! shopping. They stay last turn's, which is also the turn they next apply to.
//!
//! # Unknown is not zero
//!
//! `failure_rates[i] == -1` means the turn carried no command info — not that
//! training is free. Those rows render an em dash. Printing `0%` there would be
//! a lie that reads as "safe to train", which is the exact opposite of what the
//! panel is for.

use honse_services::overlay::theme;

use super::{egui, with_snapshot, Face};
use crate::memory_reader::CareerSnapshot;

/// Facility order, shared by every `[_; 5]` on the snapshot.
const FACILITIES: [(&str, &str); 5] = [
    ("SPD", "spd"),
    ("STA", "sta"),
    ("POW", "pow"),
    ("GUT", "gut"),
    ("WIT", "wit"),
];

/// A failure rate at or above this gets the warm treatment — it is the number
/// the highlight is asking you to accept.
const WARN_FAILURE_PCT: i32 = 10;

/// Draw the panel. Called every present; returns without painting whenever the
/// overlay has nothing true to say.
pub fn draw(ui: &mut egui::Ui) {
    let face = super::refreshed_face();
    if !face.visible() {
        return; // no chrome either — see `overlay::chrome`
    }
    with_snapshot(|snapshot| {
        if snapshot.is_playing {
            honse_services::overlay::chrome(ui, |ui| body(ui, snapshot, face));
        }
    });
}

fn body(ui: &mut egui::Ui, snapshot: &CareerSnapshot, face: Face) {
    ui.set_opacity(face.opacity());
    header(ui, face);
    ui.add_space(6.0);

    // A turn with no command info at all: levels are still true, so the rows
    // stay and every projection becomes a dash.
    let has_command_info = snapshot.failure_rates.iter().any(|&r| r >= 0);
    let best = has_command_info.then(|| best_slot(snapshot)).flatten();

    for slot in 0..5 {
        if snapshot.training_levels[slot] <= 0 {
            continue; // facility unavailable this turn — the row drops out
        }
        row(ui, snapshot, slot, Some(slot) == best);
    }

    if !has_command_info {
        ui.add_space(6.0);
        ui.label(
            egui::RichText::new("Projections appear once you are back on the command screen.")
                .font(theme::text::help())
                .color(theme::TEXT_MUTED),
        );
    }

    energy(ui, snapshot);
}

fn header(ui: &mut egui::Ui, face: Face) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("TRAINING")
                .font(theme::text::panel_title())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            freshness_dot(ui, face);
        });
    });
}

/// Live is a filled dot, holding a hollow one. No turn number: the game screen
/// behind the panel already says which turn you are on.
fn freshness_dot(ui: &mut egui::Ui, face: Face) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(9.0, 9.0), egui::Sense::hover());
    let centre = rect.center();
    if face == Face::Live {
        ui.painter().circle_filled(centre, 3.5, theme::ACCENT_BRIGHT);
    } else {
        ui.painter()
            .circle_stroke(centre, 3.5, egui::Stroke::new(1.0, theme::TEXT_MUTED));
    }
}

fn row(ui: &mut egui::Ui, snapshot: &CareerSnapshot, slot: usize, is_best: bool) {
    let (label, _) = FACILITIES[slot];
    let level = snapshot.training_levels[slot];
    let failure = snapshot.failure_rates[slot];

    let row_height = 23.0;
    let full = egui::vec2(ui.available_width(), row_height);
    let (rect, _) = ui.allocate_exact_size(full, egui::Sense::hover());

    if is_best {
        ui.painter().rect_filled(
            rect.expand2(egui::vec2(4.0, 1.0)),
            egui::CornerRadius::same(theme::RADIUS_ROW),
            theme::ROW_HIGHLIGHT,
        );
        ui.painter().rect_stroke(
            rect.expand2(egui::vec2(4.0, 1.0)),
            egui::CornerRadius::same(theme::RADIUS_ROW),
            egui::Stroke::new(1.0, theme::ROW_HIGHLIGHT_EDGE),
            egui::StrokeKind::Inside,
        );
    }

    let painter = ui.painter();
    let mid = rect.center().y;

    // Identity bar — the stat hue appears here and nowhere else.
    painter.rect_filled(
        egui::Rect::from_min_size(egui::pos2(rect.left(), mid - 8.5), egui::vec2(3.0, 17.0)),
        egui::CornerRadius::same(2),
        theme::stat_hue(slot),
    );

    let label_colour = if is_best { theme::TEXT } else { theme::TEXT_SECONDARY };
    text(
        painter,
        egui::pos2(rect.left() + 12.0, mid),
        theme::text::row_label(),
        label,
        label_colour,
        egui::Align::LEFT,
    );
    text(
        painter,
        egui::pos2(rect.left() + 45.0, mid),
        theme::text::meta(),
        &format!("Lv{level}"),
        theme::TEXT_FAINT,
        egui::Align::LEFT,
    );

    // Failure, right-aligned so the column scans as one number.
    let (failure_text, failure_colour) = match failure {
        r if r < 0 => ("\u{2014}".to_string(), theme::TEXT_UNKNOWN),
        r if r >= WARN_FAILURE_PCT => (format!("{r}%"), theme::WARN_RATE),
        r => (format!("{r}%"), theme::TEXT_FAINT),
    };
    text(
        painter,
        egui::pos2(rect.right(), mid),
        theme::text::meta(),
        &failure_text,
        failure_colour,
        egui::Align::RIGHT,
    );

    // Gains, between the level and the failure column.
    let gains = gains_text(snapshot, slot);
    let gains_colour = if gains == "\u{2014}" {
        theme::TEXT_UNKNOWN
    } else if is_best {
        theme::ACCENT_VALUE
    } else {
        theme::TEXT
    };
    text(
        painter,
        egui::pos2(rect.left() + 76.0, mid),
        theme::text::value(),
        &gains,
        gains_colour,
        egui::Align::LEFT,
    );
}

fn energy(ui: &mut egui::Ui, snapshot: &CareerSnapshot) {
    if snapshot.max_hp <= 0 {
        return; // unknown cap — no denominator to print
    }
    ui.add_space(8.0);
    let sep = ui.available_rect_before_wrap();
    ui.painter().hline(
        sep.left()..=sep.right(),
        sep.top(),
        egui::Stroke::new(1.0, theme::SEPARATOR),
    );
    ui.add_space(7.0);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("ENERGY")
                .font(theme::text::section())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("/{}", snapshot.max_hp))
                    .font(theme::text::unit())
                    .color(theme::TEXT_FAINT),
            );
            ui.label(
                egui::RichText::new(snapshot.hp.to_string())
                    .font(theme::text::value())
                    .color(theme::TEXT),
            );
        });
    });
}

fn text(
    painter: &egui::Painter,
    pos: egui::Pos2,
    font: egui::FontId,
    s: &str,
    colour: egui::Color32,
    align: egui::Align,
) {
    let anchor = match align {
        egui::Align::RIGHT => egui::Align2::RIGHT_CENTER,
        _ => egui::Align2::LEFT_CENTER,
    };
    painter.text(pos, anchor, s, font, colour);
}

/// The facility with the largest total stat gain, or `None` when nothing gains.
///
/// Ties keep the earlier facility so the highlight does not flicker between two
/// equal rows as unrelated numbers move.
#[must_use]
pub fn best_slot(snapshot: &CareerSnapshot) -> Option<usize> {
    (0..5)
        .filter(|&i| snapshot.training_levels[i] > 0 && snapshot.stat_gains[i] > 0)
        .max_by_key(|&i| (snapshot.stat_gains[i], std::cmp::Reverse(i)))
}

/// Non-zero per-stat deltas for a facility, largest first, capped at three
/// before collapsing to a total — a fourth entry does not fit the row and does
/// not change the decision.
#[must_use]
pub fn gains_text(snapshot: &CareerSnapshot, slot: usize) -> String {
    if snapshot.failure_rates[slot] < 0 {
        return "\u{2014}".to_string();
    }
    let per_stat = snapshot.per_stat_gains[slot];
    let mut parts: Vec<(usize, i32)> = (0..5).map(|i| (i, per_stat[i])).filter(|&(_, v)| v > 0).collect();
    if parts.is_empty() {
        return "\u{2014}".to_string();
    }
    parts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    if parts.len() > 3 {
        let total: i32 = parts.iter().map(|&(_, v)| v).sum();
        return format!("+{total} total");
    }
    parts
        .iter()
        .map(|&(i, v)| format!("+{v} {}", FACILITIES[i].1))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_with(levels: [i32; 5], gains: [[i32; 5]; 5], rates: [i32; 5]) -> CareerSnapshot {
        let mut s = CareerSnapshot {
            is_playing: true,
            training_levels: levels,
            per_stat_gains: gains,
            failure_rates: rates,
            ..Default::default()
        };
        s.stat_gains = std::array::from_fn(|i| gains[i].iter().sum());
        s
    }

    #[test]
    fn best_row_is_the_largest_total_gain() {
        let s = snapshot_with(
            [1; 5],
            [
                [12, 0, 3, 0, 0], // 15
                [0, 9, 0, 0, 0],  // 9
                [0, 4, 15, 0, 0], // 19
                [0, 0, 0, 8, 0],  // 8
                [0, 0, 0, 0, 7],  // 7
            ],
            [4, 0, 11, 0, 0],
        );
        assert_eq!(best_slot(&s), Some(2));
    }

    #[test]
    fn a_tie_keeps_the_earlier_facility() {
        let s = snapshot_with(
            [1; 5],
            [[10, 0, 0, 0, 0], [10, 0, 0, 0, 0], [0; 5], [0; 5], [0; 5]],
            [0; 5],
        );
        assert_eq!(best_slot(&s), Some(0));
    }

    #[test]
    fn nothing_gaining_highlights_nothing() {
        let s = snapshot_with([1; 5], [[0; 5]; 5], [0; 5]);
        assert_eq!(best_slot(&s), None);
    }

    #[test]
    fn unavailable_facilities_never_win() {
        let s = snapshot_with(
            [0, 1, 1, 1, 1],
            [[99, 0, 0, 0, 0], [0, 5, 0, 0, 0], [0; 5], [0; 5], [0; 5]],
            [0; 5],
        );
        assert_eq!(best_slot(&s), Some(1));
    }

    #[test]
    fn missing_command_info_is_a_dash_not_a_zero() {
        let s = snapshot_with([1; 5], [[0; 5]; 5], [-1; 5]);
        assert_eq!(gains_text(&s, 0), "\u{2014}");
    }

    #[test]
    fn gains_are_largest_first() {
        let s = snapshot_with([1; 5], [[3, 0, 15, 0, 0], [0; 5], [0; 5], [0; 5], [0; 5]], [0; 5]);
        assert_eq!(gains_text(&s, 0), "+15 pow +3 spd");
    }

    #[test]
    fn four_or_more_stats_collapse_to_a_total() {
        let s = snapshot_with([1; 5], [[2, 3, 4, 5, 6], [0; 5], [0; 5], [0; 5], [0; 5]], [0; 5]);
        assert_eq!(gains_text(&s, 0), "+20 total");
    }
}
