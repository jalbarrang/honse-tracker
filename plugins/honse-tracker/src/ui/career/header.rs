//! Career panel header: trainee identity (portrait + rank badge + eval value,
//! name, outfit, stars) and the condition cluster (year·date·turn, energy, mood).
//! Mirrors the top row of the dashboard `CareerPanel`.

use crate::compat::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Stroke, StrokeKind, Vec2, Vec2b};
use egui_taffy::taffy::prelude::{fr, length};
use egui_taffy::taffy::style_helpers::auto;
use egui_taffy::{taffy, tui, TuiBuilderLogic, TuiContainerResponse};

use super::super::dimens;
use super::super::textures;
use super::theme;
use crate::career_meta;
use crate::memory_reader::{self, CareerSnapshot};
use crate::rank_table;

/// A flex column / row style with a gap (the two layouts the header needs).
#[allow(dead_code)]
fn col(gap: f32) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Column,
        gap: length(gap),
        ..Default::default()
    }
}
fn row(gap: f32) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Row,
        align_items: Some(taffy::AlignItems::Center),
        gap: length(gap),
        ..Default::default()
    }
}
#[allow(dead_code)]
fn grid_2col(gap: f32, width: f32) -> taffy::Style {
    let size = taffy::Size {
        width: length(width),
        height: auto(),
    };
    taffy::Style {
        display: taffy::Display::Grid,
        grid_template_columns: vec![fr(1.0_f32), fr(1.0_f32)],
        gap: length(gap),
        size,
        max_size: size,
        align_items: Some(taffy::AlignItems::Center),
        ..Default::default()
    }
}
#[allow(dead_code)]
fn col_end(gap: f32) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Column,
        align_items: Some(taffy::AlignItems::End),
        gap: length(gap),
        ..Default::default()
    }
}

/// Header laid out with egui_taffy (flexbox): a single row with the rank badge
/// (rank sprite medallion) and the evaluation value side by side. Portrait,
/// name, outfit, stars, and the energy pill have been removed from this panel —
/// energy is shown via the standalone HUD pill instead. No longer drawn in the
/// Training panel — the rank badge + eval live in the standalone Rank HUD pill
/// ([`rank_standalone`]); kept for potential reuse.
#[allow(dead_code)]
pub(super) fn draw(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    // Reserve the deterministic column width, not the (auto_sized-inflated)
    // available width — egui_taffy's reserve_*_width does set_min_width(), so a
    // huge value would force the header and the window wide.
    let width = super::super::overlay_panels::content_width();
    tui(ui, ui.id().with("career_header"))
        .reserve_width(width)
        .style(taffy::Style {
            align_items: Some(taffy::AlignItems::Center),
            ..row(dimens::z(dimens::GAP_MD))
        })
        .show(|tui| {
            // Rank badge (sprite medallion) and eval value, arranged horizontally.
            tui.ui(|ui| rank_badge(ui, snap));
            if let Some(ev) = snap.evaluation_value {
                tui.ui(|ui| {
                    ui.label(RichText::new(group_thousands(ev)).strong().color(theme::FG_MUTED));
                });
            }
        });
}

/// A truncating label as a taffy leaf that reports a constant, width-independent
/// size (so the truncated-to-available width is never fed back into layout, which
/// would keep the node dirty and flicker the panel every repaint).
#[allow(dead_code)]
fn truncating_label(tui: &mut egui_taffy::Tui, text: RichText) {
    tui.ui_manual(|ui, _| {
        ui.add(egui::Label::new(text).truncate());
        let h = ui.min_size().y;
        TuiContainerResponse {
            inner: (),
            min_size: Vec2::new(0.0, h),
            intrinsic_size: None,
            max_size: Vec2::new(0.0, h),
            infinite: Vec2b::new(true, false),
        }
    });
}

/// Portrait square with a rounded border.
#[allow(dead_code)]
fn portrait(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let portrait = dimens::z(dimens::PORTRAIT);
    let (p_rect, _) = ui.allocate_exact_size(Vec2::splat(portrait), egui::Sense::hover());

    // Portrait image (or placeholder), with a rounded border.
    let drawn = career_meta::trainee_portrait_path(snap.card_id)
        .and_then(|path| textures::texture(ui.ctx(), &path))
        .map(|tex| {
            ui.painter().image(
                tex.id(),
                p_rect,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        })
        .is_some();
    if !drawn {
        ui.painter()
            .rect_filled(p_rect, CornerRadius::same(10), theme::SURFACE_3);
    }
    ui.painter().rect_stroke(
        p_rect,
        CornerRadius::same(10),
        Stroke::new(1.0_f32, theme::LINE),
        StrokeKind::Inside,
    );
}

/// Rank badge: gold-ringed dark medallion with the rank sprite, beside the portrait.
fn rank_badge(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let Some(ev) = snap.evaluation_value else { return };
    let badge = dimens::z(dimens::RANK_BADGE);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(badge), egui::Sense::hover());

    let label = rank_table::rank_label(ev);
    let center = rect.center();
    let r = badge * 0.5;
    ui.painter().circle_filled(center, r, theme::SURFACE_1);
    ui.painter().circle_stroke(center, r, Stroke::new(2.0_f32, theme::GOLD));
    let drew = career_meta::rank_label_sprite(label)
        .and_then(|path| textures::texture(ui.ctx(), &path))
        .map(|tex| {
            let s = badge * 0.74;
            let ir = Rect::from_center_size(center, Vec2::splat(s));
            ui.painter().image(
                tex.id(),
                ir,
                Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                Color32::WHITE,
            );
        })
        .is_some();
    if !drew {
        ui.painter().text(
            center,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(badge * 0.5),
            theme::GOLD,
        );
    }
}

/// Star-rating row. Removed from the training header but kept for later reuse.
#[allow(dead_code)]
fn stars(ui: &mut egui::Ui, value: i32) {
    let mut s = String::new();
    for i in 0..5 {
        s.push(if i < value { '\u{2605}' } else { '\u{2606}' }); // ★ / ☆
    }
    ui.label(RichText::new(s).size(dimens::z(dimens::FONT_XS)).color(theme::GOLD));
}

#[allow(dead_code)]
fn date_pill(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let (year, date) = career_meta::turn_date(snap.current_turn, snap.scenario_id);
    theme::pill(ui, |ui| {
        ui.spacing_mut().item_spacing.x = dimens::z(dimens::GAP_SM);
        ui.label(RichText::new(year).strong().color(theme::UMA_300));
        ui.label(RichText::new("·").color(theme::FG_DIM));
        ui.label(RichText::new(date).strong().color(theme::FG));
        ui.label(RichText::new("·").color(theme::FG_DIM));
        ui.label(
            RichText::new(format!("T{}", snap.current_turn))
                .strong()
                .color(theme::FG_MUTED),
        );
    });
}

#[allow(dead_code)]
pub(super) fn energy_pill(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let pct = if snap.max_hp > 0 {
        (snap.hp as f32 / snap.max_hp as f32 * 100.0).round() as i32
    } else {
        0
    };
    let hp_color = if pct <= 25 {
        theme::GRADE_A
    } else if pct <= 50 {
        theme::STAT_POWER
    } else {
        theme::UMA_300
    };
    theme::pill(ui, |ui| {
        ui.label(RichText::new("Energy").strong().color(theme::FG_MUTED));
        ui.label(RichText::new(snap.hp.to_string()).strong().color(hp_color));
        ui.label(RichText::new(format!("/{}", snap.max_hp)).color(theme::FG_DIM));
    });
}

/// Standalone energy HUD pill: no "Energy" caption, no background, and the value
/// is drawn as bright outlined text. It sizes off the game viewport (not the
/// overlay zoom) so it behaves like a native HUD element.
pub(super) fn energy_standalone(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let pct = if snap.max_hp > 0 {
        (snap.hp as f32 / snap.max_hp as f32 * 100.0).round() as i32
    } else {
        0
    };
    let hp_color = if pct <= 25 {
        theme::GRADE_A
    } else if pct <= 50 {
        theme::STAT_POWER
    } else {
        theme::UMA_300
    };
    // Brighten the inner fill so it pops against the dark outline.
    let value_color = brighten(hp_color, 0.45);
    let max_color = brighten(theme::FG_MUTED, 0.25);

    // Game-viewport-driven size (independent of the overlay zoom slider).
    let vp = super::super::overlay_panels::viewport_scale(ui);
    let font_size = (28.0 * vp).round();

    outlined_text(
        ui,
        &[
            (snap.hp.to_string(), value_color),
            (format!("/{}", snap.max_hp), max_color),
        ],
        font_size,
    );
}

/// Standalone career HUD element: the rank sprite medallion beside the
/// evaluation value, drawn with no frame/background and outlined bright text so
/// it reads on the game canvas. Sizes off the game viewport (not the overlay
/// zoom), like [`energy_standalone`], so it behaves like a native HUD element.
pub(super) fn rank_standalone(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let Some(ev) = snap.evaluation_value else { return };

    // Game-viewport-driven sizing (independent of the overlay zoom slider).
    let vp = super::super::overlay_panels::viewport_scale(ui);
    let badge = (40.0 * vp).round();
    let font_size = (28.0 * vp).round();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = (8.0 * vp).round();

        // Rank sprite medallion (gold-ringed dark disc + rank sprite), matching
        // the in-panel `rank_badge` but viewport-scaled.
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(badge), egui::Sense::hover());
        let label = rank_table::rank_label(ev);
        let center = rect.center();
        let r = badge * 0.5;
        ui.painter().circle_filled(center, r, theme::SURFACE_1);
        ui.painter().circle_stroke(center, r, Stroke::new(2.0_f32, theme::GOLD));
        let drew = career_meta::rank_label_sprite(label)
            .and_then(|path| textures::texture(ui.ctx(), &path))
            .map(|tex| {
                let s = badge * 0.74;
                let ir = Rect::from_center_size(center, Vec2::splat(s));
                ui.painter().image(
                    tex.id(),
                    ir,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            })
            .is_some();
        if !drew {
            ui.painter().text(
                center,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(badge * 0.5),
                theme::GOLD,
            );
        }

        // Evaluation value as outlined bright text.
        let value_color = brighten(theme::FG, 0.25);
        outlined_text(ui, &[(group_thousands(ev), value_color)], font_size);
    });
}

/// Lighten `c` toward white by `t` (0.0 = unchanged, 1.0 = white).
fn brighten(c: Color32, t: f32) -> Color32 {
    let lerp = |x: u8| (x as f32 + (255.0 - x as f32) * t).round() as u8;
    Color32::from_rgb(lerp(c.r()), lerp(c.g()), lerp(c.b()))
}

/// Paint `segments` as a single line with a dark outline behind the colored fill.
/// The outline is drawn by stamping a darkened copy of the galley at 8 offsets.
fn outlined_text(ui: &mut egui::Ui, segments: &[(String, Color32)], font_size: f32) {
    use egui::text::{LayoutJob, TextFormat};

    let font = egui::FontId::proportional(font_size);
    let mut fill_job = LayoutJob::default();
    let mut outline_job = LayoutJob::default();
    let outline_color = Color32::from_rgb(0x08, 0x0a, 0x0e);
    for (text, color) in segments {
        fill_job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: *color,
                ..Default::default()
            },
        );
        outline_job.append(
            text,
            0.0,
            TextFormat {
                font_id: font.clone(),
                color: outline_color,
                ..Default::default()
            },
        );
    }

    let fill = ui.painter().layout_job(fill_job);
    let outline = ui.painter().layout_job(outline_job);

    let thickness = (font_size / 14.0).clamp(1.0, 4.0);
    let size = fill.size();
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(size.x + thickness * 2.0, size.y + thickness * 2.0),
        egui::Sense::hover(),
    );
    let origin = rect.min + Vec2::new(thickness, thickness);

    let painter = ui.painter();
    for dx in [-thickness, 0.0, thickness] {
        for dy in [-thickness, 0.0, thickness] {
            if dx == 0.0 && dy == 0.0 {
                continue;
            }
            painter.galley(origin + Vec2::new(dx, dy), outline.clone(), outline_color);
        }
    }
    painter.galley(origin, fill, Color32::WHITE);
}

#[allow(dead_code)]
fn mood_pill(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let label = memory_reader::mood_label(snap.motivation);
    theme::pill(ui, |ui| {
        ui.label(
            RichText::new(label.to_uppercase())
                .strong()
                .color(theme::mood_color(snap.motivation)),
        );
    });
}

/// Thousands-separated integer ("7,002").
fn group_thousands(n: i32) -> String {
    let neg = n < 0;
    let digits: Vec<char> = n.unsigned_abs().to_string().chars().collect();
    let mut out = String::new();
    for (i, c) in digits.iter().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*c);
    }
    if neg {
        format!("-{out}")
    } else {
        out
    }
}
