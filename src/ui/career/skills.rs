//! Career panel Skills section (acquired-skill cards with rarity rail + icon +
//! level) and the Conditions tag row. Mirrors the dashboard `CareerPanel` tail.

use crate::compat::egui::{self, Color32, CornerRadius, RichText, Stroke, StrokeKind, Vec2, Vec2b};
use egui_taffy::taffy::prelude::{auto, length};
use egui_taffy::{taffy, tui, TuiBuilderLogic, TuiContainerResponse};

use super::super::dimens;
use super::super::textures;
use super::theme;
use crate::chara_effects::{self, Polarity};
use crate::gametora_data;
use crate::memory_reader::CareerSnapshot;
use crate::overlay_cache;

pub(super) fn draw(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    let skills = overlay_cache::skills();
    let trailing = match overlay_cache::skill_points() {
        Some(sp) => format!("{sp} SP  \u{b7} {}", skills.len()),
        None => format!("\u{b7} {}", skills.len()),
    };
    theme::section_strip(ui, "Skills", &trailing);
    ui.add_space(4.0);

    if skills.is_empty() {
        ui.label(RichText::new("No skills acquired yet").small().color(theme::FG_DIM));
    } else {
        // Single full-width column at the overlay's narrow width.
        let w = super::super::overlay_panels::content_width();
        for (idx, s) in skills.iter().enumerate() {
            skill_card(ui, idx, s.master_id, s.level, &s.name, w);
            ui.add_space(4.0);
        }
    }

    conditions(ui, snap);
}

/// `[rarity rail | icon | name (fills, truncates) | Lv pill]`, laid out as an
/// egui_taffy flex row inside the card frame.
fn skill_card(ui: &mut egui::Ui, idx: usize, master_id: i32, level: i32, name: &str, w: f32) {
    let meta = gametora_data::skill(master_id as i64);
    let rarity = meta.and_then(|m| m.rarity).unwrap_or(1);
    let icon_id = meta.and_then(|m| m.iconid);
    let label = if name.is_empty() {
        format!("#{master_id}")
    } else {
        name.to_string()
    };

    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: 0,
            right: 8,
            top: 5,
            bottom: 5,
        })
        .corner_radius(CornerRadius::same(8))
        .fill(theme::SURFACE_2)
        .stroke(Stroke::new(1.0_f32, theme::LINE))
        .show(ui, |ui| {
            let inner = (w - dimens::z(dimens::SKILL_CARD_MARGIN)).max(40.0);
            ui.set_width(inner);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            tui(ui, ui.id().with("career_skill").with(idx))
                .reserve_width(inner)
                .style(taffy::Style {
                    display: taffy::Display::Flex,
                    flex_direction: taffy::FlexDirection::Row,
                    align_items: Some(taffy::AlignItems::Center),
                    gap: taffy::Size {
                        width: length(dimens::z(dimens::GAP_MD)),
                        height: length(0.0_f32),
                    },
                    size: taffy::Size {
                        width: length(inner),
                        height: auto(),
                    },
                    ..Default::default()
                })
                .show(|tui| {
                    // Rarity rail (rounded on its right edge).
                    tui.style(item_center()).add(|tui| {
                        tui.ui(|ui| {
                            let (rail, _) = ui.allocate_exact_size(
                                Vec2::new(dimens::z(dimens::RAIL_W), dimens::z(dimens::RAIL_H)),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect_filled(
                                rail,
                                CornerRadius {
                                    nw: 0,
                                    ne: 2,
                                    sw: 0,
                                    se: 2,
                                },
                                rarity_color(rarity),
                            );
                        });
                    });
                    if let Some(id) = icon_id {
                        tui.style(item_center()).add(|tui| {
                            tui.ui(|ui| {
                                textures::image_square(
                                    ui,
                                    &format!("{id}.png"),
                                    dimens::z(dimens::ICON_LG),
                                    Color32::WHITE,
                                )
                            });
                        });
                    }
                    // Name fills the remaining width and truncates. Report a
                    // constant size (width 0 + infinite.x) so the truncating label
                    // doesn't feed its assigned width back into layout and spin
                    // Taffy every frame (see career/bonds.rs).
                    tui.style(name_grow()).add(|tui| {
                        tui.ui_manual(|ui, _| {
                            ui.add(
                                egui::Label::new(RichText::new(&label).small().strong().color(theme::FG)).truncate(),
                            );
                            let h = ui.min_size().y;
                            TuiContainerResponse {
                                inner: (),
                                min_size: Vec2::new(0.0, h),
                                intrinsic_size: None,
                                max_size: Vec2::new(0.0, h),
                                infinite: Vec2b::new(true, false),
                            }
                        });
                    });
                    if level > 1 {
                        tui.style(item_center()).add(|tui| {
                            tui.ui(|ui| {
                                theme::pill(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("Lv {level}"))
                                            .small()
                                            .strong()
                                            .color(theme::FG_MUTED),
                                    );
                                });
                            });
                        });
                    }
                });
        });
}

/// A content-sized flex item, vertically centered.
fn item_center() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        ..Default::default()
    }
}

/// The flexible name item: grows into the remaining width, can shrink to 0.
fn name_grow() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_grow: 1.0,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Start),
        min_size: taffy::Size {
            width: length(0.0_f32),
            height: auto(),
        },
        ..Default::default()
    }
}

/// Rarity rail color (uma-sim buckets): 1 white/silver, 2 gold, 3–5 unique
/// (rainbow → representative violet), 6 evolution pink.
fn rarity_color(rarity: i64) -> Color32 {
    match rarity {
        2 => Color32::from_rgb(0xff, 0xbe, 0x28),     // gold
        3..=5 => Color32::from_rgb(0xaa, 0xaa, 0xff), // unique (rainbow)
        6 => Color32::from_rgb(0xff, 0x9b, 0xd3),     // evolution pink
        _ => Color32::from_rgb(0xb5, 0xb2, 0xc6),     // white/silver
    }
}

/// Condition tags as a wrapping flex row of colored chips.
fn conditions(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    if snap.chara_effect_ids.is_empty() {
        return;
    }
    ui.add_space(dimens::z(dimens::GAP_LG));
    let w = super::super::overlay_panels::content_width();
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    tui(ui, ui.id().with("career_conditions"))
        .reserve_width(w)
        .style(taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            flex_wrap: taffy::FlexWrap::Wrap,
            align_items: Some(taffy::AlignItems::Center),
            gap: taffy::Size {
                width: length(dimens::z(dimens::GAP_MD)),
                height: length(dimens::z(dimens::GAP_SM)),
            },
            size: taffy::Size {
                width: length(w),
                height: auto(),
            },
            ..Default::default()
        })
        .show(|tui| {
            tui.style(item_center()).add(|tui| {
                tui.ui(|ui| {
                    ui.label(RichText::new("CONDITIONS").small().strong().color(theme::FG_MUTED));
                });
            });
            for &id in &snap.chara_effect_ids {
                let (name, polarity) = chara_effects::lookup(id);
                // User convention: orange positive / blue negative.
                let color = match polarity {
                    Polarity::Positive => theme::STAT_POWER,
                    Polarity::Negative => Color32::from_rgb(0x4d, 0x9f, 0xff),
                };
                tui.style(chip_style()).add_with_background_ui(
                    move |ui, c| chip_background(ui, c, color),
                    move |tui, _| {
                        tui.ui(|ui| {
                            ui.label(RichText::new(name).small().strong().color(color));
                        });
                    },
                );
            }
        });
}

/// Padded, centered chip item style.
fn chip_style() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Center),
        padding: taffy::Rect {
            left: length(dimens::z(dimens::CHIP_PAD_X)),
            right: length(dimens::z(dimens::CHIP_PAD_X)),
            top: length(dimens::z(dimens::CHIP_PAD_Y)),
            bottom: length(dimens::z(dimens::CHIP_PAD_Y)),
        },
        ..Default::default()
    }
}

fn chip_background(ui: &mut egui::Ui, container: &egui_taffy::TaffyContainerUi, color: Color32) {
    let rect = container.full_container();
    ui.painter().rect_filled(rect, CornerRadius::same(8), theme::SURFACE_2);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0_f32, color.gamma_multiply(0.6)),
        StrokeKind::Inside,
    );
}
