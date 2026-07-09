//! Career panel Training section: the Speed/Stamina/Power/Guts/Wit table with
//! per-facility level, stat value + rank sprite, single/total gains, and failure
//! rate. Mirrors the dashboard `CareerPanel` Training grid.

use crate::compat::egui::{self, Color32, CornerRadius, RichText, Stroke, StrokeKind};
use egui_taffy::taffy::prelude::{fr, length};
use egui_taffy::taffy::style_helpers::auto;
use egui_taffy::{taffy, tui, TaffyContainerUi, TuiBuilderLogic};

use super::super::dimens;
use super::super::textures;
use super::theme;
use crate::career_meta;
use crate::memory_reader::CareerSnapshot;

const FACILITIES: [&str; 5] = ["Speed", "Stamina", "Power", "Guts", "Wit"];

/// Pin width to a definite length — never `percent(1.)`, which resolves against the
/// host auto-sized window and grows the overlay without bound.
fn fixed_width(w: f32) -> taffy::Style {
    let size = taffy::Size {
        width: length(w),
        height: auto(),
    };
    taffy::Style {
        size,
        max_size: size,
        ..Default::default()
    }
}

fn card() -> taffy::Style {
    taffy::Style {
        // Tight vertical padding (keep the wider side padding) so the stat table
        // reads as a compact block.
        padding: taffy::Rect {
            left: length(dimens::z(dimens::GAP_LG)),
            right: length(dimens::z(dimens::GAP_LG)),
            top: length(dimens::z(dimens::GAP_SM)),
            bottom: length(dimens::z(dimens::GAP_SM)),
        },
        ..Default::default()
    }
}

fn grid_6col() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Grid,
        // Grow to fill the card's main axis so the fr() columns have the full
        // width to distribute, instead of collapsing to content width on the left.
        flex_grow: 1.0,
        grid_template_columns: vec![
            length(dimens::z(dimens::STAT_LABEL_COL)),
            fr(1.0_f32),
            fr(1.0_f32),
            fr(1.0_f32),
            fr(1.0_f32),
            fr(1.0_f32),
        ],
        gap: taffy::Size {
            width: length(dimens::z(dimens::GAP_MD)),
            height: length(dimens::z(dimens::GAP_XS)),
        },
        align_items: Some(taffy::AlignItems::Stretch),
        justify_items: Some(taffy::AlignItems::Stretch),
        ..Default::default()
    }
}

fn label_cell() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Row,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Start),
        ..Default::default()
    }
}

fn data_cell() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        flex_direction: taffy::FlexDirection::Row,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Center),
        ..Default::default()
    }
}

fn card_background(ui: &mut egui::Ui, container: &TaffyContainerUi) {
    let rect = container.full_container();
    ui.painter().rect_filled(rect, CornerRadius::same(8), theme::SURFACE_2);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0_f32, theme::LINE),
        StrokeKind::Inside,
    );
}

pub(super) fn draw(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    theme::section_strip(ui, "Training", "");
    ui.add_space(4.0);

    let width = super::super::overlay::content_width();
    let stats = [snap.speed, snap.stamina, snap.power, snap.guts, snap.wiz];
    // Cells get a near-zero available width during taffy's measure pass, so egui
    // text would wrap one glyph per line. Force every label to extend instead.
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    tui(ui, ui.id().with("career_training"))
        .reserve_width(width)
        .style(taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Column,
            align_items: Some(taffy::AlignItems::Stretch),
            ..fixed_width(width)
        })
        .show(|tui| {
            tui.style(card()).add_with_background_ui(card_background, |tui, _| {
                tui.style(grid_6col()).add(|tui| {
                    draw_header_row(tui, snap);
                    draw_stat_row(tui, &stats);
                    draw_single_row(tui, snap);
                    draw_total_row(tui, snap);
                    draw_failure_row(tui, snap);
                });
            });
        });
}

fn draw_header_row(tui: &mut egui_taffy::Tui, snap: &CareerSnapshot) {
    tui.style(label_cell()).add(|tui| {
        tui.ui(|ui| ui.label(""));
    });
    for (i, name) in FACILITIES.iter().enumerate() {
        tui.style(data_cell()).add(|tui| {
            tui.ui(|ui| {
                let resp = ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = dimens::z(dimens::GAP_XS);
                    ui.label(RichText::new(*name).small().strong().color(theme::FG));
                    ui.label(
                        RichText::new(format!("L{}", snap.training_levels[i]))
                            .small()
                            .strong()
                            .color(theme::UMA_400),
                    );
                });
                resp.response.on_hover_text(*name);
            });
        });
    }
}

fn draw_stat_row(tui: &mut egui_taffy::Tui, stats: &[i32; 5]) {
    tui.style(label_cell()).add(|tui| {
        tui.ui(|ui| ui.label(RichText::new("Stat").strong().color(theme::FG)));
    });
    for &v in stats {
        tui.style(data_cell()).add(|tui| {
            tui.ui(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = dimens::z(dimens::GAP_XS);
                    textures::image_square(
                        ui,
                        &career_meta::stat_rank_sprite(v),
                        dimens::z(dimens::ICON_MD),
                        Color32::WHITE,
                    );
                    ui.label(RichText::new(v.to_string()).strong().color(theme::FG));
                });
            });
        });
    }
}

fn draw_single_row(tui: &mut egui_taffy::Tui, snap: &CareerSnapshot) {
    tui.style(label_cell()).add(|tui| {
        tui.ui(|ui| ui.label(RichText::new("Single").strong().color(theme::FG)));
    });
    for i in 0..5 {
        tui.style(data_cell()).add(|tui| {
            tui.ui(|ui| gain_cell(ui, snap.per_stat_gains[i][i]));
        });
    }
}

fn draw_total_row(tui: &mut egui_taffy::Tui, snap: &CareerSnapshot) {
    tui.style(label_cell()).add(|tui| {
        tui.ui(|ui| ui.label(RichText::new("Total").strong().color(theme::FG)));
    });
    for i in 0..5 {
        tui.style(data_cell()).add(|tui| {
            tui.ui(|ui| gain_cell(ui, snap.stat_gains[i]));
        });
    }
}

fn draw_failure_row(tui: &mut egui_taffy::Tui, snap: &CareerSnapshot) {
    tui.style(label_cell()).add(|tui| {
        tui.ui(|ui| ui.label(RichText::new("Failure").strong().color(theme::FG)));
    });
    for i in 0..5 {
        tui.style(data_cell()).add(|tui| {
            tui.ui(|ui| fail_cell(ui, snap.failure_rates[i]));
        });
    }
}

fn gain_cell(ui: &mut egui::Ui, gain: i32) {
    if gain > 0 {
        ui.label(RichText::new(format!("+{gain}")).strong().color(theme::STAT_SPEED));
    } else {
        ui.label(RichText::new("\u{2013}").color(theme::FG_DIM));
    }
}

fn fail_cell(ui: &mut egui::Ui, rate: i32) {
    if rate < 0 {
        ui.label(RichText::new("\u{2013}").color(theme::FG_MUTED));
        return;
    }
    let color = if rate < 20 {
        theme::UMA_400
    } else if rate < 40 {
        theme::STAT_POWER
    } else if rate < 60 {
        theme::STAT_GUTS
    } else {
        theme::GRADE_A
    };
    ui.label(RichText::new(format!("{rate}%")).strong().color(color));
}
