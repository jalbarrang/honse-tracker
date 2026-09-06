//! One button: hand this run to torena-hub's Skill Planner.
//!
//! Draws on the Skills Shop and pre-complete screens, which is where leftover
//! skill points get spent.
//!
//! # It reads for itself
//!
//! The capture cache is empty here. A career finished through Independent
//! Training reaches these screens from the home screen without ever passing
//! through command select, so no capture is ever published for it — this panel
//! rendering from the cache is what made the button invisible on the first run
//! that tried it. So [`sync`] schedules its own read on the way in and drops it
//! on the way out. `None` means no career, and no button: one that can only
//! report failure when clicked is worse than none.
//!
//! # The mouse claim cannot be set while drawing
//!
//! `overlay::draw_panels` holds the panel list locked for the whole paint, and
//! `set_panel_interactive` wants that same lock — so calling it from [`draw`]
//! deadlocks the render thread. [`sync`] is a frame job, which runs before the
//! paint.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use honse_services::overlay::theme;

use super::egui;
use crate::compat::Sdk;
use crate::memory_reader::{read_planner_basics, PlannerBasics};
use crate::read_gate::View;

/// What the last read found, or `None` off the screen / outside a career.
static BASICS: Mutex<Option<PlannerBasics>> = Mutex::new(None);
/// Whether a read has been asked for on this visit to the screen.
static PROBED: AtomicBool = AtomicBool::new(false);
/// When this visit to the screen began, for [`SETTLE`].
static ARRIVED: Mutex<Option<Instant>> = Mutex::new(None);

/// How long the screen has to have been up before the trainee is read.
///
/// The view id changes while the screen is still being built, and reading at
/// that edge is what the whole read gate exists to avoid — the first version
/// of this probe fired 20&nbsp;ms after the change and took the game down. The
/// scenario refresh, the only other read outside a settled turn, paces itself
/// at 400 ms for the same reason.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(600);
/// Whether the panel is on screen, so the interactive flag is only written
/// when it changes rather than every frame.
static INTERACTIVE: AtomicBool = AtomicBool::new(false);

/// Frame job: keep the trainee read and the mouse claim in step with the
/// screen. Runs before the paint; see the module header.
pub fn sync() {
    if on_planner_screen() {
        if !settled() {
            return;
        }
        // One read per visit. The balance moves as you buy, so it is read
        // again on the click that actually uses it.
        if !PROBED.swap(true, Ordering::AcqRel) && !Sdk::get().schedule_on_main_thread(probe_cb) {
            PROBED.store(false, Ordering::Release);
        }
    } else {
        *arrived() = None;
        // Cleared on every off-screen frame, not just the first: a probe
        // queued before the screen closed can land after it, and a result
        // nobody clears leaves the panel drawing over the game.
        PROBED.store(false, Ordering::Release);
        let mut basics = lock();
        if basics.is_some() {
            *basics = None;
        }
    }
    let showing = lock().is_some();
    if INTERACTIVE.swap(showing, Ordering::AcqRel) != showing {
        honse_services::overlay::set_panel_interactive("planner", showing);
    }
}

/// Whether the screen has been up long enough to read from, starting the
/// clock on the first frame of a visit.
fn settled() -> bool {
    let mut arrived = arrived();
    let since = arrived.get_or_insert_with(Instant::now);
    since.elapsed() >= SETTLE
}

fn arrived() -> std::sync::MutexGuard<'static, Option<Instant>> {
    ARRIVED.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Unity main thread: read the trainee, or record that there is no career.
///
/// Nothing may escape this: it is called through a C ABI, so a panic crossing
/// it aborts the process rather than unwinding.
extern "C" fn probe_cb() {
    let read = std::panic::catch_unwind(|| {
        // Re-checked because this runs a frame or more after it was asked for,
        // and the player may have left in between.
        if !on_planner_screen() {
            return None;
        }
        hlog_info!(target: "training-tracker", "Skill planner: reading the trainee");
        let basics = read_planner_basics();
        hlog_info!(
            target: "training-tracker",
            "Skill planner: trainee read {}",
            if basics.is_some() { "ok" } else { "found no career" }
        );
        basics
    });
    match read {
        Ok(basics) => *lock() = basics,
        Err(_) => hlog_error!(target: "training-tracker", "Skill planner: probe PANICKED"),
    }
}

pub fn draw(ui: &mut egui::Ui) {
    let Some(skill_points) = lock().as_ref().map(|b| b.skill_point) else {
        return;
    };
    honse_services::overlay::chrome(ui, |ui| body(ui, skill_points));
}

fn lock() -> std::sync::MutexGuard<'static, Option<PlannerBasics>> {
    BASICS.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The two screens where leftover skill points get spent, while the overlay is
/// painting a career at all.
fn on_planner_screen() -> bool {
    super::face().visible()
        && matches!(
            View::from_id(honse_services::current_view_id()),
            View::CareerSkillsShop | View::CareerPreComplete
        )
}

/// Never dimmed, unlike the other panels. `Face::Holding` means "this is the
/// last settled turn's numbers"; these are ones this panel read for itself on
/// the way in, so there is nothing to apologise for.
fn body(ui: &mut egui::Ui, skill_points: i32) {
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
