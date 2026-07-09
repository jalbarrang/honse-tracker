//! Shared UI sizing and overlay identifiers.

/// Default overlay font size.
pub(super) const OVERLAY_FONT_SIZE: f32 = 16.0;
/// Default fixed overlay content width at zoom 1.0 (points). Each panel is a
/// fixed-width column with auto height; the zoom slider scales this.
pub(super) const OVERLAY_BASE_WIDTH: f32 = 500.0;
/// Standalone energy pill no longer uses a fixed base width (it sizes off the
/// game viewport via `overlay::draw_energy_standalone`); kept for reference.
#[allow(dead_code)]
pub(super) const ENERGY_BASE_WIDTH: f32 = 140.0;
pub(super) const TRAINING_BASE_WIDTH: f32 = 500.0;
pub(super) const BONDS_BASE_WIDTH: f32 = 500.0;
pub(super) const SCENARIO_BASE_WIDTH: f32 = 500.0;
pub(super) const SHOP_BASE_WIDTH: f32 = 500.0;
/// Maximum overlay height at zoom 1.0 (points). The panel caps here instead of
/// growing unbounded with content (which made the host window scroll the whole
/// overlay); tab bodies scroll internally within the remaining space.
pub(super) const OVERLAY_MAX_HEIGHT: f32 = 500.0;

/// Per-window fixed heights at zoom 1.0 (points). `None` keeps a window
/// auto-sizing (capped by [`OVERLAY_MAX_HEIGHT`]); `Some(h)` pins it to `h`.
#[allow(dead_code)]
pub(super) const ENERGY_FIXED_HEIGHT: Option<f32> = None;
pub(super) const TRAINING_FIXED_HEIGHT: Option<f32> = None;
pub(super) const BONDS_FIXED_HEIGHT: Option<f32> = None;
pub(super) const SCENARIO_FIXED_HEIGHT: Option<f32> = None;
pub(super) const SHOP_FIXED_HEIGHT: Option<f32> = None;

pub(super) const ENERGY_OVERLAY_ID: &str = "training_tracker_overlay_energy";
pub(super) const TRAINING_OVERLAY_ID: &str = "training_tracker_overlay_training";
pub(super) const BONDS_OVERLAY_ID: &str = "training_tracker_overlay_bonds";
pub(super) const SCENARIO_OVERLAY_ID: &str = "training_tracker_overlay_scenario";
pub(super) const SHOP_OVERLAY_ID: &str = "training_tracker_overlay_shop";
pub(super) const RANK_OVERLAY_ID: &str = "training_tracker_overlay_rank";

pub(super) const PANEL_IDS: [&str; 6] = [
    ENERGY_OVERLAY_ID,
    TRAINING_OVERLAY_ID,
    BONDS_OVERLAY_ID,
    SCENARIO_OVERLAY_ID,
    SHOP_OVERLAY_ID,
    RANK_OVERLAY_ID,
];
