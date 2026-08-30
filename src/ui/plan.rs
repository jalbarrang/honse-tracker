//! The song planner — pick what you are saving for, per concert.
//!
//! Opened with a hotkey and driven entirely by them, because without a WndProc
//! the overlay observes keys rather than consuming them: anything we watch also
//! reaches the game. Every chord therefore carries Ctrl+Shift, navigation
//! included. See [`super::keys`].
//!
//! # It opens where you are
//!
//! The starting window comes from the live per-token cap, so the planner is
//! already on the concert you are playing. Paging to another window is what
//! makes it a *planner* — you can commit to a window-4 song in Junior year and
//! watch the shortfall from then on.
//!
//! # Scope
//!
//! Planned is a wish list, not a ledger. Nothing here notices that you already
//! own a song: `TotalMusicIdArray` gives owned music ids, but pairing those to
//! catalogue entries needs a name↔id map we can only learn as songs appear on
//! the tree. Until that exists, a bought song stays "planned".

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use honse_services::overlay::theme;

use super::{egui, with_snapshot};
use crate::memory_reader::ScenarioState;
use crate::song_catalog::{self, Song};
use crate::song_plan;

static OPEN: AtomicBool = AtomicBool::new(false);
/// Concert window being edited, 1-4. `0` means "follow the live cap".
static WINDOW: AtomicU8 = AtomicU8::new(0);
static CURSOR: AtomicUsize = AtomicUsize::new(0);

/// Whether the planner is on screen. The navigation hotkeys no-op when closed,
/// so Ctrl+Shift+Space outside the planner does nothing rather than something
/// surprising.
#[must_use]
pub fn is_open() -> bool {
    OPEN.load(Ordering::Acquire)
}

/// Open or close the planner. Opening resets the cursor and re-follows the live
/// window, so it never reopens pointing at last session's concert.
pub fn toggle_open() {
    let open = !is_open();
    OPEN.store(open, Ordering::Release);
    if open {
        WINDOW.store(0, Ordering::Release);
        CURSOR.store(0, Ordering::Release);
    }
    hlog_info!(target: "training-tracker", "Song planner: {}", if open { "open" } else { "closed" });
}

/// The window the live cap says we are in, or 1 when it is unknown.
fn live_window() -> u8 {
    with_snapshot(|snapshot| match &snapshot.scenario_state {
        Some(ScenarioState::GrandLive(perf)) => song_catalog::window_for_cap(perf.caps.dance),
        _ => None,
    })
    .flatten()
    .unwrap_or(1)
}

/// The window on screen: an explicit choice, else whichever one is live.
fn active_window() -> u8 {
    match WINDOW.load(Ordering::Acquire) {
        0 => live_window(),
        w => w,
    }
}

fn songs() -> Vec<&'static Song> {
    song_catalog::songs_in_window(active_window()).collect()
}

/// Move the cursor, wrapping at both ends so you cannot get stuck.
pub fn move_cursor(delta: i32) {
    if !is_open() {
        return;
    }
    let len = songs().len();
    if len == 0 {
        return;
    }
    let current = CURSOR.load(Ordering::Acquire).min(len - 1) as i32;
    let next = (current + delta).rem_euclid(len as i32);
    CURSOR.store(next as usize, Ordering::Release);
}

/// Page to another concert, clamped to 1..=4. Selecting a window pins it, so
/// the planner stops following the live cap until it is closed and reopened.
pub fn change_window(delta: i32) {
    if !is_open() {
        return;
    }
    let next = (i32::from(active_window()) + delta).clamp(1, 4);
    WINDOW.store(next as u8, Ordering::Release);
    CURSOR.store(0, Ordering::Release);
}

/// Plan or skip the highlighted song.
pub fn toggle_selected() {
    if !is_open() {
        return;
    }
    let songs = songs();
    let Some(song) = songs.get(CURSOR.load(Ordering::Acquire).min(songs.len().saturating_sub(1))) else {
        return;
    };
    song_plan::toggle(song.id);
    hlog_info!(
        target: "training-tracker",
        "Song planner: {} is now {}",
        song.name,
        if song_plan::is_planned(song.id) { "planned" } else { "skipped" }
    );
}

/// Return the visible window to uma.guide's defaults.
pub fn reset_window() {
    if !is_open() {
        return;
    }
    let window = active_window();
    song_plan::reset_window(window);
    hlog_info!(target: "training-tracker", "Song planner: concert {window} reset to guide defaults");
}

pub fn draw(ui: &mut egui::Ui) {
    if !is_open() {
        return;
    }
    honse_services::overlay::chrome(ui, body);
}

fn body(ui: &mut egui::Ui) {
    let window = active_window();
    let songs = songs();
    let cursor = CURSOR.load(Ordering::Acquire).min(songs.len().saturating_sub(1));
    let cap = song_catalog::CONCERT_CAPS[usize::from(window - 1)];

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("PLAN \u{00b7} CONCERT {window}"))
                .font(theme::text::panel_title())
                .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(format!("cap {cap}"))
                    .font(theme::text::meta())
                    .color(theme::TEXT_FAINT),
            );
        });
    });
    ui.add_space(6.0);

    let owned = super::owned_songs();
    for (i, song) in songs.iter().enumerate() {
        row(ui, song, i == cursor, owned.has(song.id));
    }

    totals(ui, window, cap, &owned);
    help(ui);
}

fn row(ui: &mut egui::Ui, song: &Song, selected: bool, owned: bool) {
    let planned = song_plan::is_planned(song.id);
    // Three states, not two. A bought song is settled — it neither costs
    // anything further nor is something you can still decide about.
    let (mark, mark_colour) = if owned {
        ("\u{25cf}", theme::ACCENT_BRIGHT)
    } else if planned {
        ("\u{2713}", theme::ACCENT)
    } else {
        ("\u{2717}", theme::TEXT_UNKNOWN)
    };

    ui.horizontal(|ui| {
        // The cursor is a glyph, not a highlight bar: at this row height a
        // filled bar behind small text hurts legibility more than it helps.
        ui.label(
            egui::RichText::new(if selected { "\u{203a}" } else { " " })
                .font(theme::text::row_label())
                .color(theme::ACCENT_BRIGHT),
        );
        ui.label(
            egui::RichText::new(mark)
                .font(theme::text::meta())
                .color(mark_colour),
        );
        ui.label(
            egui::RichText::new(song.name)
                .font(theme::text::row_label())
                .color(match (owned, planned, selected) {
                    (true, _, _) => theme::TEXT_SECONDARY,
                    (false, true, _) => theme::TEXT,
                    (false, false, true) => theme::TEXT_SECONDARY,
                    (false, false, false) => theme::TEXT_MUTED,
                }),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // A bought song's cost is history; showing it invites re-adding it up.
            let (text, colour) = if owned {
                ("bought".to_string(), theme::TEXT_FAINT)
            } else if planned {
                (super::token_vector_text(song.cost), theme::TEXT_FAINT)
            } else {
                (super::token_vector_text(song.cost), theme::TEXT_UNKNOWN)
            };
            ui.label(egui::RichText::new(text).font(theme::text::meta()).color(colour));
        });
    });
}

fn totals(ui: &mut egui::Ui, window: u8, cap: i32, owned: &song_plan::Owned) {
    ui.add_space(6.0);
    let sep = ui.available_rect_before_wrap();
    ui.painter().hline(
        sep.left()..=sep.right(),
        sep.top(),
        egui::Stroke::new(1.0, theme::SEPARATOR),
    );
    ui.add_space(6.0);

    let required = song_plan::remaining_cost(window, owned);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{}/{} bought \u{00b7} still need",
                song_plan::owned_count(window, owned),
                song_plan::planned_count(window)
            ))
            .font(theme::text::meta())
            .color(theme::TEXT_MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(super::token_vector_text(required))
                    .font(theme::text::meta())
                    .color(theme::TEXT),
            );
        });
    });

    if song_catalog::exceeds_cap(required, cap) {
        ui.label(
            egui::RichText::new("this plan exceeds the concert ceiling")
                .font(theme::text::help())
                .color(theme::NEGATIVE),
        );
    }
}

fn help(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new("Ctrl+Shift  \u{2191}\u{2193} move  \u{2190}\u{2192} concert  Space plan  R reset")
            .font(theme::text::help())
            .color(theme::TEXT_FAINT),
    );
}
