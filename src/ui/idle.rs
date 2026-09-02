//! Independent Training: how long is left, and when it lands.
//!
//! # The one panel that is not about the career
//!
//! Every other panel draws from a settled turn and wears a `Face` derived
//! from the career lifecycle. This one ignores both. Independent Training runs
//! on a wall clock while you are on the home screen, in Team Trials, anywhere —
//! all of which are `Face::Off`, which is exactly when you most want to know
//! how long is left. So visibility here is decided by one question only: is
//! there a session to report?
//!
//! That also makes it self-hiding. There is no chrome and no empty box when
//! nothing is running, so leaving it switched on costs nothing.
//!
//! # Why it prints a clock time as well as a countdown
//!
//! "26 minutes" answers a different question from "14:32". The first tells you
//! whether to wait; the second tells you what else you can fit in. The game
//! only offers the first.

use std::sync::atomic::{AtomicBool, Ordering};

use honse_services::overlay::theme;

use super::egui;
use crate::idle_training::{Countdown, IdleState};

/// Whether the panel paints. On by default — it draws nothing at all unless a
/// session is actually running, so there is nothing to opt out of until there
/// is something to see. `Ctrl+Shift+I` turns it off.
static ENABLED: AtomicBool = AtomicBool::new(true);

/// Show or hide the panel.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Release);
    hlog_info!(target: "training-tracker", "Idle training panel: {}", if enabled { "on" } else { "off" });
}

/// Whether the panel is painting.
#[must_use]
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Acquire)
}

/// Flip the panel, for the hotkey and the menu item.
pub fn toggle() {
    set_enabled(!is_enabled());
}

/// What the panel has to say about a session, if anything.
///
/// Split out from drawing so the visibility rule is a value rather than a
/// tangle of early returns inside a paint function.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Report {
    /// Counting down. `remaining` seconds, `progress` of the way through.
    Running { remaining: i64, progress: f32 },
    /// Landed. The run is sitting in the game waiting to be collected.
    Ready,
    /// Nothing worth a panel: no session, or one already collected.
    Nothing,
}

/// Decide what to draw from the countdown alone. Pure.
///
/// `LogChecked` deliberately reads as nothing: the result has been seen, so the
/// panel has stopped being a timer and would just be clutter until the next
/// session starts.
fn report(countdown: Option<Countdown>) -> Report {
    let Some(c) = countdown else {
        return Report::Nothing; // before the first poll
    };
    match c.state {
        IdleState::Playing if c.remaining > 0 => Report::Running {
            remaining: c.remaining,
            progress: c.progress,
        },
        // Time is up. The server may not have settled the result yet, so
        // `Playing` with nothing left means the same thing to a reader as
        // `Finished` does.
        IdleState::Playing | IdleState::Finished => Report::Ready,
        IdleState::Idle | IdleState::LogChecked | IdleState::Unrecognised(_) => Report::Nothing,
    }
}

pub fn draw(ui: &mut egui::Ui) {
    if !is_enabled() {
        return;
    }
    match report(crate::idle_training::countdown()) {
        Report::Nothing => (), // no chrome either — nothing is running
        r => honse_services::overlay::chrome(ui, |ui| body(ui, r)),
    }
}

fn body(ui: &mut egui::Ui, report: Report) {
    ui.label(
        egui::RichText::new("INDEPENDENT TRAINING")
            .font(theme::text::panel_title())
            .color(theme::TEXT_MUTED),
    );
    ui.add_space(6.0);

    match report {
        Report::Running { remaining, progress } => {
            ui.label(
                egui::RichText::new(clock(remaining))
                    .font(theme::text::value_lead())
                    .color(theme::TEXT),
            );
            ui.add_space(5.0);
            gauge(ui, progress, theme::ACCENT);
            ui.add_space(5.0);
            footer(ui, &lands_at(remaining), theme::TEXT_MUTED);
        }
        Report::Ready => {
            ui.label(
                egui::RichText::new("Ready")
                    .font(theme::text::value_lead())
                    .color(theme::ACCENT_VALUE),
            );
            ui.add_space(5.0);
            gauge(ui, 1.0, theme::ACCENT_BRIGHT);
            ui.add_space(5.0);
            footer(ui, "waiting to be collected", theme::TEXT_SECONDARY);
        }
        // `draw` never calls us with this; matching it keeps the arm honest
        // rather than reaching for an unreachable panic.
        Report::Nothing => (),
    }

    // Nothing will announce this one. Said here because the countdown looks
    // exactly the same either way, and silence is not something you notice
    // until you have already missed it.
    if matches!(report, Report::Running { .. }) && !crate::idle_training::is_armed() {
        ui.add_space(2.0);
        footer(ui, "no alert armed \u{2014} see the log", theme::CAUTION);
    }
}

/// The filled bar, matching the game's own gauge for the same session.
fn gauge(ui: &mut egui::Ui, progress: f32, colour: egui::Color32) {
    let height = 6.0;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::hover());
    let radius = egui::CornerRadius::same(3);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, theme::ROW_HIGHLIGHT_EDGE);

    let filled = rect.width() * progress.clamp(0.0, 1.0);
    if filled > 0.5 {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.min, egui::vec2(filled, height)),
            radius,
            colour,
        );
    }
}

fn footer(ui: &mut egui::Ui, text: &str, colour: egui::Color32) {
    ui.label(egui::RichText::new(text).font(theme::text::meta()).color(colour));
}

/// `h:mm:ss`, the same shape the game's own "time left" uses. Shared with the
/// debug readout so the two can never drift into different formats.
pub(super) fn clock(secs: i64) -> String {
    let secs = secs.max(0);
    format!("{}:{:02}:{:02}", secs / 3600, (secs / 60) % 60, secs % 60)
}

/// "lands 14:32", in the player's own local time.
///
/// Computed from the countdown rather than from the raw end timestamp so the
/// two lines can never disagree: whatever the clock above says is left, this is
/// that far from now.
fn lands_at(remaining: i64) -> String {
    let Some(at) = chrono::Local::now().checked_add_signed(chrono::TimeDelta::seconds(remaining)) else {
        return "lands \u{2014}".to_string();
    };
    format!("lands {}", at.format("%H:%M"))
}

#[cfg(test)]
mod tests {
    use super::{clock, report, Countdown, IdleState, Report};

    fn countdown(state: IdleState, remaining: i64, progress: f32) -> Option<Countdown> {
        Some(Countdown {
            state,
            remaining,
            progress,
        })
    }

    #[test]
    fn a_running_session_counts_down() {
        assert_eq!(
            report(countdown(IdleState::Playing, 900, 0.75)),
            Report::Running {
                remaining: 900,
                progress: 0.75
            }
        );
    }

    /// The server can leave the state at `Playing` past the end time until the
    /// player opens the result. To a reader that is finished, not a timer
    /// stuck on zero.
    #[test]
    fn a_finished_session_reads_ready_whichever_state_it_reports() {
        assert_eq!(report(countdown(IdleState::Playing, 0, 1.0)), Report::Ready);
        assert_eq!(report(countdown(IdleState::Finished, 0, 1.0)), Report::Ready);
    }

    /// The panel is self-hiding: with nothing running there is no chrome, so
    /// leaving it switched on costs nothing.
    #[test]
    fn nothing_to_report_draws_nothing() {
        assert_eq!(report(None), Report::Nothing);
        for state in [IdleState::Idle, IdleState::LogChecked, IdleState::Unrecognised(7)] {
            assert_eq!(report(countdown(state, 0, 1.0)), Report::Nothing, "{state:?}");
        }
    }

    #[test]
    fn the_clock_matches_the_games_own_shape() {
        assert_eq!(clock(2777), "0:46:17");
        assert_eq!(clock(3600), "1:00:00");
        assert_eq!(clock(0), "0:00:00");
        assert_eq!(clock(-5), "0:00:00", "an overrun never prints a negative");
    }
}
