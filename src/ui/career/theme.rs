//! Visual primitives for the Career panel, backed by the shared Honse UI tokens
//! and painters.
//!
//! Game-specific layout sizing and sprite lookup stay local; palette, generic
//! chrome, and low-level painting come from `honse_ui`.

use crate::compat::egui::{self, Color32, Ui};

use super::super::textures;
use crate::career_meta;

pub use honse_ui::theme::{mood_color, stat_color};

const TOKENS: honse_ui::theme::Tokens = honse_ui::theme::Tokens::DEFAULT;

// Thin compatibility aliases for existing tracker call sites.
pub const SURFACE_1: Color32 = TOKENS.surface_1;
pub const SURFACE_2: Color32 = TOKENS.surface_2;
#[allow(dead_code)]
pub const SURFACE_3: Color32 = TOKENS.surface_3;
pub const LINE: Color32 = TOKENS.line;
pub const FG: Color32 = TOKENS.fg;
pub const FG_MUTED: Color32 = TOKENS.fg_muted;
pub const FG_DIM: Color32 = TOKENS.fg_dim;
pub const UMA_300: Color32 = TOKENS.uma_300;
pub const UMA_400: Color32 = TOKENS.uma_400;
pub const GRADE_A: Color32 = TOKENS.bad;
pub const STAT_SPEED: Color32 = TOKENS.stat_speed;
pub const STAT_POWER: Color32 = TOKENS.stat_power;
pub const STAT_GUTS: Color32 = TOKENS.stat_guts;
pub const GOLD: Color32 = TOKENS.gold;

/// The green striped section header with the `//` slash accent. `trailing` is a
/// small right-aligned caption (e.g. "1429 SP · 5"), empty for none.
pub fn section_strip(ui: &mut Ui, label: &str, trailing: &str) {
    let height = (ui.text_style_height(&egui::TextStyle::Body) + 8.0).max(22.0);
    // Deterministic width (not ui.available_width(), which inflates under the
    // host's auto_sized window) so the strip can't grow the panel.
    let width = super::super::overlay_panels::content_width();
    let _ = honse_ui::components::section_strip(ui, label, trailing, width, height);
}

/// A small raised pill chip; `add` draws its inline contents.
pub fn pill(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    let _ = honse_ui::components::pill(ui, add);
}

/// Frame for a bond row: rainbow border when `rainbow`, else the raised face.
#[allow(dead_code)]
pub fn row_frame(rainbow: bool) -> egui::Frame {
    honse_ui::components::row_frame(rainbow)
}

/// A stat-colored rounded chip with the stat glyph centered, side `size` px.
/// Falls back to a colored chip with the facility's initial when the icon sprite
/// is unavailable.
#[allow(dead_code)]
pub fn stat_chip(ui: &mut Ui, facility: usize, size: f32) {
    let icon = career_meta::stat_icon_path(facility);
    let sprite = textures::texture(ui.ctx(), &icon).map(|tex| tex.id());
    let _ = honse_ui::components::stat_chip_chrome(ui, facility, size, sprite);
}
