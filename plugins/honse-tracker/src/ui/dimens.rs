//! Centralized UI dimensions and explicit font sizes (base values at zoom 1.0),
//! plus the zoom-scaling helper [`z`].
//!
//! Route every layout dimension and explicit font size through [`z`] so the whole
//! overlay scales uniformly with the user's zoom setting. Text set via egui text
//! styles (`.small()`, `.strong()`, `ui.label`) scales automatically because
//! `overlay::apply_scale` scales the `text_styles` map; only explicit `.size(..)`
//! and taffy `length(..)`/painted dimensions need `z`.
//!
//! Note: widths derived from [`super::overlay::content_width`] are already scaled
//! — do not wrap those in [`z`] again.

/// Scale a base dimension / font size by the current overlay zoom.
pub(super) fn z(base: f32) -> f32 {
    base * super::overlay::scale()
}

// ── Gaps & spacing ────────────────────────────────────────────────────────
pub(super) const GAP_XS: f32 = 2.0;
pub(super) const GAP_SM: f32 = 4.0;
pub(super) const GAP_MD: f32 = 6.0;
pub(super) const GAP_LG: f32 = 8.0;

// ── Chips / pills ─────────────────────────────────────────────────────────
#[allow(dead_code)]
pub(super) const CHIP_PAD_X: f32 = 8.0;
#[allow(dead_code)]
pub(super) const CHIP_PAD_Y: f32 = 3.0;

// ── Cards / rows ──────────────────────────────────────────────────────────
/// Horizontal inner margin a row frame eats (used to derive inner content width).
#[allow(dead_code)]
pub(super) const ROW_FRAME_MARGIN: f32 = 20.0;
/// Skill-card right inner margin.
#[allow(dead_code)]
pub(super) const SKILL_CARD_MARGIN: f32 = 8.0;

// ── Grids ─────────────────────────────────────────────────────────────────
/// Training table rotated stat-label column width.
pub(super) const STAT_LABEL_COL: f32 = 52.0;
/// Inter-column gap in data grids/tables.
pub(super) const GRID_GAP_X: f32 = 10.0;

// ── Icons / sprites ───────────────────────────────────────────────────────
pub(super) const ICON_MD: f32 = 16.0;
pub(super) const ICON_LG: f32 = 24.0;
/// Rarity rail width on skill cards.
#[allow(dead_code)]
pub(super) const RAIL_W: f32 = 4.0;
#[allow(dead_code)]
pub(super) const RAIL_H: f32 = 24.0;

// ── Header ────────────────────────────────────────────────────────────────
#[allow(dead_code)]
pub(super) const PORTRAIT: f32 = 56.0;
pub(super) const RANK_BADGE: f32 = 30.0;

// ── Type scale (Tailwind / shadcn-style) ───────────────────────────────────
// One coherent ramp for the whole overlay, anchored at base = 16px. egui's
// built-in text styles map onto this scale in `overlay::apply_scale` (Small→sm,
// Body/Button→base, Heading→xl), and explicit `.size(..)` calls should use these
// tokens (scaled via `z`) rather than magic numbers.
pub(super) const FONT_XS: f32 = 12.0;
pub(super) const FONT_SM: f32 = 14.0;
pub(super) const FONT_BASE: f32 = 16.0;
#[allow(dead_code)]
pub(super) const FONT_LG: f32 = 18.0;
#[allow(dead_code)]
pub(super) const FONT_XL: f32 = 20.0;

// ── L1 menu (settings) page ───────────────────────────────────────────────
// The menu is a host-embedded page, NOT the overlay, so these are used raw
// (never through `z`) — they must not follow the overlay zoom.
pub(super) const MENU_GAP_X: f32 = 8.0;
pub(super) const MENU_GAP_Y: f32 = 5.0;
pub(super) const MENU_ROW_GAP_Y: f32 = 4.0;
pub(super) const MENU_WIDTH_MIN: f32 = 120.0;
pub(super) const MENU_WIDTH_MAX: f32 = 560.0;
