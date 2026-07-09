//! Pure reusable egui components for Honse UI surfaces.

use std::hash::Hash;
use std::ops::RangeInclusive;

use egui::{Color32, CornerRadius, Response, RichText, Stroke, StrokeKind, TextureId, Ui, Vec2};

use crate::paint;
use crate::theme::{stat_color, Tokens};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PillButtonKind {
    Primary,
    Secondary,
    Danger,
}

pub fn pill_button(ui: &mut Ui, text: impl Into<String>, kind: PillButtonKind) -> Response {
    let tokens = Tokens::DEFAULT;
    let (fill, stroke, text_color) = match kind {
        PillButtonKind::Primary => (
            tokens.accent,
            Stroke::new(1.0_f32, paint::brighten_toward_white(tokens.accent, 0.22)),
            Color32::WHITE,
        ),
        PillButtonKind::Secondary => (tokens.surface_2, Stroke::new(1.0_f32, tokens.line), tokens.fg),
        PillButtonKind::Danger => (
            tokens.crit,
            Stroke::new(1.0_f32, tokens.crit.linear_multiply(0.8)),
            Color32::WHITE,
        ),
    };

    ui.add(
        egui::Button::new(RichText::new(text.into()).color(text_color).strong())
            .fill(fill)
            .stroke(stroke)
            .corner_radius(tokens.radius_pill)
            .min_size(Vec2::new(0.0, 26.0)),
    )
}

pub fn primary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    pill_button(ui, text, PillButtonKind::Primary)
}

pub fn secondary_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    pill_button(ui, text, PillButtonKind::Secondary)
}

pub fn danger_button(ui: &mut Ui, text: impl Into<String>) -> Response {
    pill_button(ui, text, PillButtonKind::Danger)
}

pub fn card_frame(_ui: &Ui) -> egui::Frame {
    let tokens = Tokens::DEFAULT;
    egui::Frame::new()
        .fill(tokens.surface_2)
        .stroke(Stroke::new(1.0_f32, tokens.line))
        .corner_radius(tokens.radius_card)
        .inner_margin(egui::Margin::symmetric(12, 10))
}

pub fn row_frame(rainbow: bool) -> egui::Frame {
    let tokens = Tokens::DEFAULT;
    let stroke = if rainbow {
        Stroke::new(1.5_f32, Color32::from_rgb(0x9a, 0x8c, 0xff))
    } else {
        Stroke::new(1.0_f32, tokens.line)
    };
    egui::Frame::new()
        .fill(tokens.surface_2)
        .stroke(stroke)
        .corner_radius(tokens.radius_small)
        .inner_margin(egui::Margin::symmetric(10, 6))
}

pub fn window_chrome(ui: &mut Ui, title: &str, add_body: impl FnOnce(&mut Ui)) {
    let tokens = Tokens::DEFAULT;
    egui::Frame::new()
        .fill(tokens.surface_1)
        .stroke(Stroke::new(1.0_f32, tokens.line))
        .corner_radius(tokens.radius_card)
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.label(RichText::new(title).color(tokens.fg).strong());
            ui.add_space(4.0);
            add_body(ui);
        });
}

pub fn section_header(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.add_space(8.0);
    let response = ui.add(egui::Label::new(RichText::new(text.into()).strong().size(15.0)));
    ui.add_space(4.0);
    response
}

pub fn section_strip(ui: &mut Ui, label: &str, trailing: &str, width: f32, height: f32) -> Response {
    let size = Vec2::new(width, height.max(22.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint::vgrad(
            ui,
            rect,
            Color32::from_rgb(0x40, 0x9c, 0x3c),
            Color32::from_rgb(0x2c, 0x82, 0x2a),
            6,
        );
        paint::strip_stripes(ui, rect);
        ui.painter().text(
            rect.left_center() + Vec2::new(10.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(size.y * 0.55),
            Color32::WHITE,
        );
        if !trailing.is_empty() {
            ui.painter().text(
                rect.right_center() - Vec2::new(10.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                trailing,
                egui::FontId::proportional(size.y * 0.46),
                Color32::from_white_alpha(220),
            );
        }
    }
    response
}

pub fn pill<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let tokens = Tokens::DEFAULT;
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(10, 5))
        .corner_radius(tokens.radius_small)
        .fill(tokens.surface_2)
        .stroke(Stroke::new(1.0_f32, tokens.line))
        .show(ui, |ui| ui.horizontal(add).inner)
}

pub fn badge(ui: &mut Ui, text: impl Into<String>) -> Response {
    let tokens = Tokens::DEFAULT;
    ui.add(
        egui::Label::new(RichText::new(text.into()).color(tokens.fg).strong().size(12.0)).sense(egui::Sense::hover()),
    )
}

pub fn chip(ui: &mut Ui, text: impl Into<String>) -> Response {
    pill(ui, |ui| {
        ui.label(RichText::new(text.into()).color(Tokens::DEFAULT.fg_muted))
    })
    .inner
}

pub fn stat_chip(ui: &mut Ui, label: &str, value: impl ToString, delta: Option<&str>) -> Response {
    let tokens = Tokens::DEFAULT;
    let desired = Vec2::new(72.0, 48.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(rect, tokens.radius_small, tokens.surface_2);
        ui.painter().rect_stroke(
            rect,
            tokens.radius_small,
            Stroke::new(1.0_f32, tokens.line),
            StrokeKind::Inside,
        );
        ui.painter().text(
            rect.center_top() + Vec2::new(0.0, 6.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::TextStyle::Small.resolve(ui.style()),
            tokens.fg_dim,
        );
        ui.painter().text(
            rect.center_top() + Vec2::new(0.0, 20.0),
            egui::Align2::CENTER_TOP,
            value.to_string(),
            egui::TextStyle::Button.resolve(ui.style()),
            tokens.fg,
        );
        if let Some(delta) = delta {
            ui.painter().text(
                rect.center_bottom() - Vec2::new(0.0, 4.0),
                egui::Align2::CENTER_BOTTOM,
                delta,
                egui::TextStyle::Small.resolve(ui.style()),
                tokens.accent,
            );
        }
    }
    response
}

pub fn stat_chip_chrome(ui: &mut Ui, facility: usize, size: f32, sprite: Option<TextureId>) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(4), stat_color(facility));
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(4),
            Stroke::new(1.0_f32, Color32::from_black_alpha(40)),
            StrokeKind::Inside,
        );
        if let Some(texture_id) = sprite {
            let inner = rect.shrink(size * 0.12);
            ui.painter().image(
                texture_id,
                inner,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            let label = ["S", "St", "P", "G", "W"].get(facility).copied().unwrap_or("?");
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(size * 0.6),
                Color32::from_black_alpha(180),
            );
        }
    }
    response
}

pub fn empty_state(ui: &mut Ui, text: impl Into<String>) {
    card_frame(ui).show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(text.into()).color(Tokens::DEFAULT.fg_dim));
        });
    });
}

pub fn separator(ui: &mut Ui) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Tokens::DEFAULT.line);
    response
}

pub fn toggle(ui: &mut Ui, label: &str, checked: bool) -> Option<bool> {
    let mut value = checked;
    let response = ui.checkbox(&mut value, RichText::new(label).color(Tokens::DEFAULT.fg).size(14.0));
    response.changed().then_some(value)
}

pub fn combo<T: PartialEq + Copy>(ui: &mut Ui, id_salt: impl Hash, value: &mut T, choices: &[(T, &str)]) -> bool {
    let selected = choices
        .iter()
        .find(|(v, _)| v == value)
        .map_or("Unknown", |(_, label)| *label);
    let mut changed = false;
    egui::ComboBox::new(ui.id().with(id_salt), "")
        .wrap_mode(egui::TextWrapMode::Wrap)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            for (choice, label) in choices {
                changed |= ui.selectable_value(value, *choice, *label).changed();
            }
        });
    changed
}

pub fn slider_f32(ui: &mut Ui, value: &mut f32, range: RangeInclusive<f32>, step: f64) -> bool {
    ui.add(egui::Slider::new(value, range).step_by(step).trailing_fill(true))
        .changed()
}

/// A multiplier slider centered on 1.0x: leftmost `0.5x`, exact center `1.0x`,
/// rightmost `3.0x`. A piecewise-linear position mapping (the two halves cover
/// `0.5..1.0` and `1.0..3.0`) keeps `1.0` dead-center even though the endpoints
/// are not multiplicatively symmetric. Values snap to the nearest `0.05`.
pub fn slider_scale(ui: &mut Ui, value: &mut f32) -> bool {
    fn pos_to_scale(p: f32) -> f32 {
        let s = if p <= 0.5 { 0.5 + p } else { 1.0 + (p - 0.5) * 4.0 };
        (s * 20.0).round() / 20.0
    }
    fn scale_to_pos(v: f32) -> f32 {
        if v <= 1.0 {
            (v - 0.5).max(0.0)
        } else {
            0.5 + (v - 1.0) / 4.0
        }
    }
    let mut pos = scale_to_pos(*value).clamp(0.0, 1.0);
    let response = ui.add(
        egui::Slider::new(&mut pos, 0.0..=1.0)
            .trailing_fill(true)
            .custom_formatter(|p, _| format!("{:.2}", pos_to_scale(p as f32)))
            .custom_parser(|s| s.parse::<f64>().ok().map(|v| f64::from(scale_to_pos(v as f32)))),
    );
    if response.changed() {
        *value = pos_to_scale(pos);
        true
    } else {
        false
    }
}
