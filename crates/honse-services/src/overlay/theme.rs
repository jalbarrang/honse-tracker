//! Design tokens for the self-hosted overlay.
//!
//! Every colour here was authored in OKLCH and converted once to sRGB; the
//! OKLCH original is kept in the doc comment so the relationships stay legible
//! (the three semantic accents share L=0.78 C=0.14 and differ only in hue; the
//! five stat hues share L=0.72 C=0.15). Do not "tidy" a value without
//! re-deriving it — hand-nudged hex drifts the set apart.
//!
//! Two rules this module exists to enforce:
//!
//! 1. **Stat hues are identity, never meaning.** They appear only as the 3&nbsp;px
//!    bar at the left of a row — never as text, never as a fill. Colouring a
//!    number by stat *and* by meaning at once is how a HUD stops being readable.
//! 2. **No panel uses a light surface.** The panels are translucent over a very
//!    bright game; that only works while the surface stays dark.

use egui::{Color32, FontFamily, FontId, TextStyle};

// ── surfaces ────────────────────────────────────────────────────────────────

/// Panel body. Translucent so the game reads through it (blur is not available
/// in a plain D3D11 pass, so the alpha does the work alone).
pub const PANEL: Color32 = Color32::from_rgba_premultiplied(10, 13, 17, 235);
/// Panel edge.
pub const BORDER: Color32 = Color32::from_rgba_premultiplied(26, 26, 26, 26);
/// Divider inside a panel.
pub const SEPARATOR: Color32 = Color32::from_rgba_premultiplied(18, 18, 18, 18);
/// Highlighted row wash (the best-gain row).
pub const ROW_HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(0, 21, 21, 26);
/// Outline on the highlighted row.
pub const ROW_HIGHLIGHT_EDGE: Color32 = Color32::from_rgba_premultiplied(0, 50, 50, 61);

// ── text ────────────────────────────────────────────────────────────────────

/// The number you came for.
pub const TEXT: Color32 = Color32::from_rgb(0xe9, 0xed, 0xf3);
/// Row labels.
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xc0, 0xc9, 0xd4);
/// Panel and section titles.
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x7d, 0x88, 0x95);
/// Denominators, units, ids.
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x59, 0x63, 0x6f);
/// Values while the panel is holding.
pub const TEXT_HOLDING: Color32 = Color32::from_rgb(0xa8, 0xb2, 0xbd);
/// An em dash standing in for a value that was never read.
pub const TEXT_UNKNOWN: Color32 = Color32::from_rgb(0x4d, 0x57, 0x65);

// ── semantic — three, and only three ────────────────────────────────────────

/// `oklch(0.78 0.14 195)` — read this turn, and good.
pub const ACCENT: Color32 = Color32::from_rgb(0, 210, 211);
/// `oklch(0.82 0.13 195)` — the live dot.
pub const ACCENT_BRIGHT: Color32 = Color32::from_rgb(54, 222, 222);
/// `oklch(0.86 0.12 195)` — an accented value.
pub const ACCENT_VALUE: Color32 = Color32::from_rgb(93, 233, 233);
/// `oklch(0.78 0.14 65)` — layout mode, and anything with a deadline.
pub const CAUTION: Color32 = Color32::from_rgb(244, 163, 75);
/// `oklch(0.80 0.14 35)` — a failure rate worth looking at twice.
pub const WARN_RATE: Color32 = Color32::from_rgb(255, 155, 127);
/// `oklch(0.78 0.14 25)` — negative conditions, a cap you have hit, a trap.
pub const NEGATIVE: Color32 = Color32::from_rgb(255, 145, 137);

// ── stat identity — the 3px bar, and nowhere else ───────────────────────────

/// Speed · `oklch(0.72 0.15 250)`.
pub const STAT_SPEED: Color32 = Color32::from_rgb(82, 169, 254);
/// Stamina · `oklch(0.72 0.15 15)`.
pub const STAT_STAMINA: Color32 = Color32::from_rgb(243, 121, 134);
/// Power · `oklch(0.72 0.15 60)`.
pub const STAT_POWER: Color32 = Color32::from_rgb(231, 139, 48);
/// Guts · `oklch(0.72 0.15 340)`.
pub const STAT_GUTS: Color32 = Color32::from_rgb(225, 124, 194);
/// Wit · `oklch(0.72 0.15 155)`.
pub const STAT_WIT: Color32 = Color32::from_rgb(67, 192, 122);

/// Identity bar colour for a facility slot `0..=4` (Speed, Stamina, Power,
/// Guts, Wit) — the order `CareerSnapshot` uses throughout.
#[must_use]
pub const fn stat_hue(slot: usize) -> Color32 {
    match slot {
        0 => STAT_SPEED,
        1 => STAT_STAMINA,
        2 => STAT_POWER,
        3 => STAT_GUTS,
        _ => STAT_WIT,
    }
}

// ── metrics ─────────────────────────────────────────────────────────────────

/// Panel corner radius.
pub const RADIUS_PANEL: u8 = 7;
/// Row corner radius.
pub const RADIUS_ROW: u8 = 4;
/// The only two panel widths in the system.
pub const WIDTH_NARROW: f32 = 356.0;
/// See [`WIDTH_NARROW`].
pub const WIDTH_WIDE: f32 = 380.0;
/// Gap between stacked panels, and the screen-edge safe margin.
pub const GAP: f32 = 24.0;
/// Opacity multiplier applied to the whole panel while holding.
pub const HOLDING_OPACITY: f32 = 0.62;

/// Minimum height a panel is given in layout mode, so one with nothing to draw
/// still has a box you can select and move.
pub const LAYOUT_GHOST_HEIGHT: f32 = 26.0;

// ── type ────────────────────────────────────────────────────────────────────

/// The type ramp, as roles rather than sizes.
///
/// Call sites name a role and never a size or a family, so retuning the ramp —
/// or swapping in the real Space Grotesk / Spline Sans Mono faces — happens
/// here and nowhere else. Words go in the proportional family, every number in
/// the mono one so columns line up and digits do not reflow as they tick.
pub mod text {
    use super::{FontFamily, FontId};

    /// `TRAINING` — panel title.
    #[must_use]
    pub fn panel_title() -> FontId {
        FontId::new(10.5, FontFamily::Proportional)
    }
    /// `ENERGY` — section label.
    #[must_use]
    pub fn section() -> FontId {
        FontId::new(9.0, FontFamily::Proportional)
    }
    /// `SPD` — row label.
    #[must_use]
    pub fn row_label() -> FontId {
        FontId::new(11.0, FontFamily::Proportional)
    }
    /// `+12 spd +3 pow` — the value.
    #[must_use]
    pub fn value() -> FontId {
        FontId::new(12.5, FontFamily::Monospace)
    }
    /// `180` — the one lead number a panel is allowed.
    #[must_use]
    pub fn value_lead() -> FontId {
        FontId::new(15.0, FontFamily::Monospace)
    }
    /// `/1620` — denominators and units.
    #[must_use]
    pub fn unit() -> FontId {
        FontId::new(10.0, FontFamily::Monospace)
    }
    /// `Lv2`, `4%`.
    #[must_use]
    pub fn meta() -> FontId {
        FontId::new(10.5, FontFamily::Monospace)
    }
    /// Empty-state prose.
    #[must_use]
    pub fn help() -> FontId {
        FontId::new(11.5, FontFamily::Proportional)
    }
}

/// Install the overlay theme on our own egui context.
///
/// Only ever called on the context this crate owns — the host's egui is never
/// touched, which is the whole point of rendering ourselves.
///
/// # Fonts
///
/// The design calls for Space Grotesk + Spline Sans Mono. This installs egui's
/// bundled faces instead so the DLL carries no font payload; swapping in the
/// real ones is a `FontDefinitions` change here and nothing else, because every
/// call site refers to a [`text`] role rather than a family.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    // Anything that forgets to name a role still lands on the value size rather
    // than egui's 14pt default, which is far too large over the game.
    for role in [TextStyle::Body, TextStyle::Button, TextStyle::Small] {
        style
            .text_styles
            .insert(role, FontId::new(12.5, FontFamily::Proportional));
    }

    let v = &mut style.visuals;
    v.dark_mode = true;
    v.override_text_color = Some(TEXT);
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL;
    v.window_stroke = egui::Stroke::new(1.0, BORDER);
    v.window_corner_radius = egui::CornerRadius::same(RADIUS_PANEL);
    // The overlay draws no shadow: a D3D11 blur pass is not worth a frame, and
    // the border plus the dark fill already separate a panel from the game.
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;

    style.spacing.item_spacing = egui::vec2(9.0, 1.0);
    style.spacing.window_margin = egui::Margin::symmetric(14, 10);

    ctx.set_style(style);
}
