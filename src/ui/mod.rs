//! The overlay's panels, and the snapshot they read.
//!
//! # Why a cache
//!
//! A capture is an ~80&nbsp;ms run of IL2CPP calls that may only happen in
//! `CommandSelectActive` (see `read_gate`), so the render thread can never read
//! game memory to draw a frame — it would race asset unloading regardless of
//! how carefully the gate was checked. Instead every settled capture leaves a
//! copy here, and the panels render from that at whatever rate the game
//! presents.
//!
//! This is what makes the HOLDING face honest rather than a loading state: the
//! numbers on screen between captures are real, they are simply the last
//! settled turn's.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use honse_services::overlay::{self, Anchor};

use crate::memory_reader::{CareerSnapshot, LightRefresh};
use crate::read_gate::CareerState;

pub mod affordability;
pub mod debug;
pub mod keys;
pub mod plan;
pub mod performance;
pub mod training;

/// The most recent settled capture, or `None` outside a career.
static LATEST: Mutex<Option<CareerSnapshot>> = Mutex::new(None);

/// How a panel should present what it is showing. Derived from the career
/// lifecycle so every panel changes together and none invents its own idea of
/// freshness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Read this turn. Full opacity, filled dot.
    Live,
    /// A command is in flight, assets are moving, or a career is loading. The
    /// last settled turn, dimmed.
    Holding,
    /// A race or a cutscene. Nothing is drawn — you are not making a decision.
    Away,
    /// No career. Nothing is drawn.
    Off,
}

impl Face {
    /// Map the seven lifecycle states onto the four faces the design defines.
    #[must_use]
    pub const fn of(state: CareerState) -> Self {
        match state {
            CareerState::CommandSelectActive => Self::Live,
            CareerState::CommandInFlight
            | CareerState::AssetTransition
            | CareerState::CareerLoading
            | CareerState::CareerMenu => Self::Holding,
            CareerState::RaceActive | CareerState::CutsceneActive => Self::Away,
            CareerState::Idle => Self::Off,
        }
    }

    /// Whether a panel should paint anything at all.
    #[must_use]
    pub const fn visible(self) -> bool {
        matches!(self, Self::Live | Self::Holding)
    }

    /// Opacity multiplier for the whole panel.
    #[must_use]
    pub const fn opacity(self) -> f32 {
        match self {
            Self::Live => 1.0,
            _ => honse_services::overlay::theme::HOLDING_OPACITY,
        }
    }
}

/// The face the panels should wear right now.
#[must_use]
pub fn face() -> Face {
    Face::of(crate::career_poll::current_lifecycle_state())
}

/// Whether a light refresh is landing on the screen we are on.
static REFRESH_LIVE: AtomicBool = AtomicBool::new(false);

/// The face for panels whose content the light refresh keeps current.
///
/// On a shop screen the lifecycle is `CareerMenu`, so [`face`] says `Holding`
/// — right for the career as a whole, wrong for the stats, energy and scenario
/// state that are being re-read a few times a second. Those panels get `Live`
/// exactly while a refresh is landing.
///
/// One caveat this does not express: the training panel's projections (gains
/// and failure rates) are **not** part of the refresh, because nothing
/// re-derives them while you are in a shop. They stay last turn's, which is
/// also when they next apply.
#[must_use]
pub fn refreshed_face() -> Face {
    let face = face();
    if face.visible() && REFRESH_LIVE.load(Ordering::Acquire) {
        Face::Live
    } else {
        face
    }
}

/// Patch the fields a purchase can move — stats, energy, scenario state —
/// leaving everything else at its last settled value.
///
/// Deliberately partial: training projections and failure rates are *not*
/// touched, because nothing re-derives them while you are in a shop. Writing
/// them would mean inventing numbers rather than reading them.
pub fn patch_light(refresh: &LightRefresh) {
    if let Ok(mut guard) = LATEST.lock() {
        if let Some(snapshot) = guard.as_mut() {
            snapshot.speed = refresh.speed;
            snapshot.stamina = refresh.stamina;
            snapshot.power = refresh.power;
            snapshot.guts = refresh.guts;
            snapshot.wiz = refresh.wiz;
            snapshot.hp = refresh.hp;
            snapshot.max_hp = refresh.max_hp;
            snapshot.scenario_state = refresh.scenario_state.clone();
            REFRESH_LIVE.store(true, Ordering::Release);
        }
    }
}

/// Mark the cached scenario state as no longer known-fresh — on leaving the
/// screen that was refreshing it, or when a refresh fails.
pub fn clear_refresh_live() {
    REFRESH_LIVE.store(false, Ordering::Release);
}

/// Publish a settled capture for the panels to render. Called from the capture
/// path, never from the render thread.
pub fn publish(snapshot: &CareerSnapshot) {
    if let Ok(mut guard) = LATEST.lock() {
        *guard = Some(snapshot.clone());
    }
}

/// Drop the cached capture when the career ends, so a finished run's numbers
/// can never survive into the next one.
pub fn clear() {
    if let Ok(mut guard) = LATEST.lock() {
        *guard = None;
    }
    clear_refresh_live();
}

/// Run `f` against the latest capture, if there is one.
fn with_snapshot<R>(f: impl FnOnce(&CareerSnapshot) -> R) -> Option<R> {
    let guard = LATEST.lock().ok()?;
    guard.as_ref().map(f)
}

/// Register every panel on the overlay. Called once from plugin init.
pub fn install() {
    overlay::register_panel(
        "training",
        Anchor::TopRight,
        egui::vec2(overlay::theme::GAP, 96.0),
        overlay::theme::WIDTH_WIDE,
        training::draw,
    );
    // Top-left, opposite the training board so the two never collide. Narrow:
    // five short rows, and it only appears in Grand Live runs.
    overlay::register_panel(
        "performance",
        Anchor::TopLeft,
        egui::vec2(overlay::theme::GAP, 96.0),
        250.0,
        performance::draw,
    );
    // Bottom-right: the last free corner, and wide because square names are
    // long. Only appears in Grand Live runs with a tree on offer.
    overlay::register_panel(
        "affordability",
        Anchor::BottomRight,
        egui::vec2(overlay::theme::GAP, overlay::theme::GAP),
        overlay::theme::WIDTH_WIDE,
        affordability::draw,
    );
    // Bottom-left, clear of the training panel, and narrower — it is a readout,
    // not a board.
    overlay::register_panel(
        "debug",
        Anchor::BottomLeft,
        egui::vec2(overlay::theme::GAP, overlay::theme::GAP),
        300.0,
        debug::draw,
    );
    // Registering is not enough: the view poll only runs while something wants
    // it, and the panel is on by default.
    // Centre-left: the planner is a modal thing you open, read and close, so it
    // gets the middle of the screen rather than sharing a corner.
    overlay::register_panel(
        "plan",
        Anchor::TopLeft,
        egui::vec2(overlay::theme::GAP, 300.0),
        overlay::theme::WIDTH_WIDE,
        plan::draw,
    );
    debug::set_enabled(debug::is_enabled());
    crate::song_plan::load();
    keys::install();
    hlog_info!(target: "training-tracker", "Overlay: training + performance + lessons + debug panels registered");
}

/// Songs the current run has already learned, resolved to catalogue ids.
///
/// Empty outside a Grand Live run, and empty when nothing has been bought yet.
#[must_use]
pub fn owned_songs() -> crate::song_plan::Owned {
    with_snapshot(|snapshot| match &snapshot.scenario_state {
        Some(crate::memory_reader::ScenarioState::GrandLive(perf)) => {
            crate::song_plan::Owned::from_names(perf.owned.iter().filter_map(|s| s.name.as_deref()))
        }
        _ => crate::song_plan::Owned::default(),
    })
    .unwrap_or_default()
}

/// Short performance-token codes, in `PerformanceTokens::labelled` order.
/// Shared so the panels that print token vectors cannot disagree about them.
pub const TOKEN_CODES: [&str; 5] = ["Da", "Pa", "Vo", "Vi", "Co"];

/// Non-zero entries of a token vector as `Da32 Vi12`; an em dash when empty.
#[must_use]
pub fn token_vector_text(tokens: [i32; 5]) -> String {
    let parts: Vec<String> = tokens
        .iter()
        .enumerate()
        .filter(|(_, &v)| v > 0)
        .map(|(i, v)| format!("{}{v}", TOKEN_CODES[i]))
        .collect();
    if parts.is_empty() {
        "\u{2014}".to_string()
    } else {
        parts.join(" ")
    }
}

/// Re-export so panel modules and the plugin agree on one egui.
pub use honse_services::egui;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_lifecycle_state_maps_to_a_face() {
        assert_eq!(Face::of(CareerState::CommandSelectActive), Face::Live);
        for s in [
            CareerState::CommandInFlight,
            CareerState::AssetTransition,
            CareerState::CareerLoading,
        ] {
            assert_eq!(Face::of(s), Face::Holding, "{s:?}");
        }
        for s in [CareerState::RaceActive, CareerState::CutsceneActive] {
            assert_eq!(Face::of(s), Face::Away, "{s:?}");
        }
        assert_eq!(Face::of(CareerState::Idle), Face::Off);
    }

    #[test]
    fn only_live_and_holding_paint() {
        assert!(Face::Live.visible());
        assert!(Face::Holding.visible());
        assert!(!Face::Away.visible());
        assert!(!Face::Off.visible());
    }

    #[test]
    fn holding_is_dimmer_than_live() {
        assert!(Face::Holding.opacity() < Face::Live.opacity());
        assert!((Face::Live.opacity() - 1.0).abs() < f32::EPSILON);
    }
}
