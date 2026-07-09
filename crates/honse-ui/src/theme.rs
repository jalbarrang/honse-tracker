//! Canonical fixed UI tokens and egui style wiring.

use egui::{Color32, CornerRadius, Margin, Stroke, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tokens {
    pub bg: Color32,
    pub surface_1: Color32,
    pub surface_2: Color32,
    pub surface_3: Color32,
    pub line: Color32,
    pub fg: Color32,
    pub fg_muted: Color32,
    pub fg_dim: Color32,
    pub accent: Color32,
    pub good: Color32,
    pub warn: Color32,
    pub bad: Color32,
    pub crit: Color32,
    pub gold: Color32,
    pub uma_300: Color32,
    pub uma_400: Color32,
    pub stat_speed: Color32,
    pub stat_stamina: Color32,
    pub stat_power: Color32,
    pub stat_guts: Color32,
    pub stat_wit: Color32,
    pub radius_small: CornerRadius,
    pub radius_card: CornerRadius,
    pub radius_pill: CornerRadius,
    pub radius_chip: CornerRadius,
}

impl Tokens {
    pub const DEFAULT: Self = Self {
        bg: Color32::from_rgb(0x0b, 0x0e, 0x13),
        surface_1: Color32::from_rgb(0x15, 0x1a, 0x23),
        surface_2: Color32::from_rgb(0x1c, 0x22, 0x30),
        surface_3: Color32::from_rgb(0x24, 0x2c, 0x3d),
        line: Color32::from_rgb(0x2c, 0x36, 0x48),
        fg: Color32::from_rgb(0xea, 0xef, 0xf6),
        fg_muted: Color32::from_rgb(0xa3, 0xb1, 0xc4),
        fg_dim: Color32::from_rgb(0x6e, 0x7d, 0x92),
        accent: Color32::from_rgb(0x5f, 0xb2, 0xff),
        good: Color32::from_rgb(0x4f, 0xbb, 0x4f),
        warn: Color32::from_rgb(0xff, 0xb0, 0x4d),
        bad: Color32::from_rgb(0xff, 0x7a, 0x6b),
        crit: Color32::from_rgb(0xff, 0x7a, 0x6b),
        gold: Color32::from_rgb(0xf0, 0xa8, 0x18),
        uma_300: Color32::from_rgb(0x8f, 0xe0, 0x8f),
        uma_400: Color32::from_rgb(0x6f, 0xd0, 0x6f),
        stat_speed: Color32::from_rgb(0x5f, 0xb2, 0xff),
        stat_stamina: Color32::from_rgb(0xff, 0x8a, 0x5c),
        stat_power: Color32::from_rgb(0xff, 0xb0, 0x4d),
        stat_guts: Color32::from_rgb(0xff, 0x7a, 0x90),
        stat_wit: Color32::from_rgb(0x4d, 0xdc, 0xb0),
        radius_small: CornerRadius::same(8),
        radius_card: CornerRadius::same(10),
        radius_pill: CornerRadius::same(u8::MAX),
        radius_chip: CornerRadius::same(4),
    };
}

#[must_use]
pub fn stat_color(facility: usize) -> Color32 {
    let tokens = Tokens::DEFAULT;
    [
        tokens.stat_speed,
        tokens.stat_stamina,
        tokens.stat_power,
        tokens.stat_guts,
        tokens.stat_wit,
    ]
    .get(facility)
    .copied()
    .unwrap_or(tokens.surface_3)
}

#[must_use]
pub fn mood_color(motivation: i32) -> Color32 {
    match motivation {
        5 => Color32::from_rgb(0xe8, 0x5f, 0x9c),
        4 => Color32::from_rgb(0xff, 0x9a, 0x3d),
        3 => Color32::from_rgb(0xc2, 0xa8, 0x3d),
        2 => Color32::from_rgb(0x4d, 0x8f, 0xd6),
        1 => Color32::from_rgb(0xa8, 0x6f, 0xd6),
        _ => Tokens::DEFAULT.fg_muted,
    }
}

pub fn apply_style(style: &mut Style, opacity: f32) {
    let tokens = Tokens::DEFAULT;
    let opacity = opacity.clamp(0.0, 1.0);
    let faded = |color: Color32| color.linear_multiply(opacity);

    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.window_margin = Margin::symmetric(14, 12);
    style.spacing.menu_margin = Margin::symmetric(8, 8);
    style.interaction.selectable_labels = false;

    style.visuals.window_fill = faded(tokens.surface_1);
    style.visuals.panel_fill = faded(tokens.surface_1);
    style.visuals.extreme_bg_color = tokens.bg;
    style.visuals.window_corner_radius = tokens.radius_card;
    style.visuals.window_stroke = Stroke::new(1.0, tokens.line);

    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, tokens.fg);

    style.visuals.widgets.inactive.weak_bg_fill = faded(tokens.surface_2);
    style.visuals.widgets.inactive.bg_fill = faded(tokens.surface_2);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, tokens.line);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, tokens.fg_dim);
    style.visuals.widgets.inactive.corner_radius = tokens.radius_small;

    style.visuals.widgets.hovered.weak_bg_fill = faded(tokens.surface_3);
    style.visuals.widgets.hovered.bg_fill = faded(tokens.surface_3);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, tokens.accent.linear_multiply(0.75));
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, tokens.fg);
    style.visuals.widgets.hovered.corner_radius = tokens.radius_small;

    style.visuals.widgets.active.weak_bg_fill = faded(tokens.accent.linear_multiply(0.72));
    style.visuals.widgets.active.bg_fill = faded(tokens.accent);
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0, tokens.accent);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0, brighten_toward_white(tokens.fg, 0.6));
    style.visuals.widgets.active.corner_radius = tokens.radius_small;

    style.visuals.widgets.open = style.visuals.widgets.hovered;
    style.visuals.selection.bg_fill = tokens.accent.linear_multiply(0.42);
    style.visuals.selection.stroke = Stroke::new(1.0, tokens.accent);
    style.visuals.override_text_color = Some(tokens.fg);
    style.visuals.hyperlink_color = tokens.accent;
}

fn brighten_toward_white(c: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8| (f32::from(x) + (255.0 - f32::from(x)) * t).round() as u8;
    Color32::from_rgb(lerp(c.r()), lerp(c.g()), lerp(c.b()))
}
