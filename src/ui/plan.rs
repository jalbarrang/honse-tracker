//! The song planner — pick what you are saving for, per concert.
//!
//! Opened with a hotkey, and drivable entirely by them — every chord carries
//! Ctrl+Shift, navigation included. See [`super::keys`].
//!
//! It is also the one panel that takes the mouse, and only while it is open:
//! left-click plans a song, right-click marks it bought. Clicks landing on it
//! do not reach the game, which is why that is switched off the moment the
//! planner closes.
//!
//! # It opens where you are
//!
//! The starting window comes from the live per-token cap, so the planner is
//! already on the concert you are playing. Paging to another window is what
//! makes it a *planner* — you can commit to a window-4 song in Junior year and
//! watch the shortfall from then on.
//!
//! # Three states, not two
//!
//! A row is bought, planned, or skipped, and totals count what you still need
//! rather than what the plan costs.
//!
//! Bought is normally *read* — `TotalMusicIdArray` gives the songs this run has
//! learned — but that read has proven unreliable, so `B` marks a song bought by
//! hand. Marks are unioned with what the reader finds, never subtracted, so a
//! detection fix later can only add and your record cannot be contradicted.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use honse_services::overlay::theme;

use super::{egui, with_snapshot};
use crate::memory_reader::ScenarioState;
use crate::song_catalog::{self, Song};
use crate::song_plan::{self, Scope};

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
    // The overlay only swallows clicks over a panel that asked for them, so
    // this is what decides whether the game or the planner gets the mouse.
    honse_services::overlay::set_panel_interactive("plan", open);
    if open {
        WINDOW.store(0, Ordering::Release);
        CURSOR.store(0, Ordering::Release);
    }
    hlog_info!(target: "training-tracker", "Song planner: {}", if open { "open" } else { "closed" });
}

/// The window the live cap says we are in, or 1 when it is unknown.
///
/// Clamped to a concert that has songs: during the closing Grand Concert the
/// cap says 5, but there is nothing there to plan, so the planner opens on the
/// last concert that offers anything.
fn live_window() -> u8 {
    with_snapshot(|snapshot| match &snapshot.scenario_state {
        Some(ScenarioState::GrandLive(perf)) => song_catalog::window_for_cap(perf.caps.dance),
        _ => None,
    })
    .flatten()
    .unwrap_or(1)
    .min(song_catalog::LAST_SONG_WINDOW)
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

/// The cursor, clamped into `len`. The stored index can outlive the list it
/// indexes — paging to a window with fewer songs leaves it past the end.
fn cursor_in(len: usize) -> usize {
    CURSOR.load(Ordering::Acquire).min(len.saturating_sub(1))
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
    let next = (cursor_in(len) as i32 + delta).rem_euclid(len as i32);
    CURSOR.store(next as usize, Ordering::Release);
}

/// Page to another concert. Selecting a window pins it, so the planner stops
/// following the live cap until it is closed and reopened.
pub fn change_window(delta: i32) {
    if !is_open() {
        return;
    }
    let next = (i32::from(active_window()) + delta).clamp(1, i32::from(song_catalog::LAST_SONG_WINDOW));
    WINDOW.store(next as u8, Ordering::Release);
    CURSOR.store(0, Ordering::Release);
}

/// Plan or skip the highlighted song.
pub fn toggle_selected() {
    if !is_open() {
        return;
    }
    let songs = songs();
    let Some(song) = songs.get(cursor_in(songs.len())) else {
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

/// Mark the highlighted song bought, or un-mark it.
///
/// An escape hatch for when the game's own owned-song list cannot be read.
/// Marks are unioned with what the reader detects, so this can only ever add a
/// song — turning detection back on later cannot contradict what you recorded.
pub fn toggle_bought_selected() {
    if !is_open() {
        return;
    }
    let songs = songs();
    let Some(song) = songs.get(cursor_in(songs.len())) else {
        return;
    };
    song_plan::toggle_bought(song.id);
    hlog_info!(
        target: "training-tracker",
        "Song planner: {} marked {}",
        song.name,
        if song_plan::is_marked_bought(song.id) { "bought" } else { "not bought" }
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
    let cursor = cursor_in(songs.len());
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
        // Clicking a row moves the cursor there first, so the keys carry on
        // from wherever the mouse left off rather than from where they were.
        let response = row(ui, song, i == cursor, owned.has(song.id));
        if response.clicked() {
            CURSOR.store(i, Ordering::Release);
            toggle_selected();
        } else if response.secondary_clicked() {
            CURSOR.store(i, Ordering::Release);
            toggle_bought_selected();
        }
    }

    totals(ui, window, cap, &owned);
    help(ui);
}

/// One song row. Returns its click response: left plans, right marks bought.
fn row(ui: &mut egui::Ui, song: &Song, selected: bool, owned: bool) -> egui::Response {
    let planned = song_plan::is_planned(song.id);
    // Reserved now, painted after the row is laid out: a hover highlight has to
    // go behind the text, and the rect is not known until the text is placed.
    let highlight = ui.painter().add(egui::Shape::Noop);
    // Three states, not two. A bought song is settled — it neither costs
    // anything further nor is something you can still decide about.
    // Geometric Shapes and Latin-1 only. egui's default fonts have no Dingbats
    // or Arrows blocks, so a check mark or a cross renders as a tofu box —
    // which made planned and skipped rows indistinguishable on screen.
    let (mark, mark_colour) = if owned {
        ("\u{25cf}", theme::ACCENT_BRIGHT) // filled: bought
    } else if planned {
        ("\u{25cb}", theme::ACCENT) // hollow: still to buy
    } else {
        ("\u{00d7}", theme::TEXT_UNKNOWN) // times: skipped
    };

    let laid_out = ui.horizontal(|ui| {
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

    // Hit the whole row, not the words in it: a two-pixel target is not a
    // target. Widened a little so the highlight reads as a band.
    let rect = laid_out.response.rect.expand2(egui::vec2(4.0, 1.0));
    let response = ui.interact(rect, ui.id().with(song.id), egui::Sense::click());
    if response.hovered() {
        ui.painter().set(
            highlight,
            egui::epaint::RectShape::filled(
                rect,
                egui::CornerRadius::same(theme::RADIUS_ROW),
                theme::ROW_HIGHLIGHT,
            ),
        );
    }
    response
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

    let required = song_plan::remaining_cost(Scope::Concert(window), owned);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "{}/{} bought \u{00b7} still need",
                song_plan::owned_count(Scope::Concert(window), owned),
                song_plan::planned_count(Scope::Concert(window))
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
        egui::RichText::new("click plan  \u{00b7}  right-click bought  \u{00b7}  Ctrl+Shift arrows, Space, B, R")
            .font(theme::text::help())
            .color(theme::TEXT_FAINT),
    );
}
