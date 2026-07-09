//! Trackblazer RaceCoin shop rendering.

use crate::compat::egui::{self, Color32, RichText, Vec2, Vec2b};
use egui_taffy::taffy::prelude::{auto, fr, length};
use egui_taffy::{taffy, tui, Tui, TuiBuilderLogic, TuiContainerResponse};

use crate::memory_reader;

use crate::ui::dimens;
use crate::ui::overlay;
use crate::ui::util::worth_color;

pub(super) fn draw(ui: &mut egui::Ui, shop: &memory_reader::TrackblazerShop) {
    draw_header(ui, shop);
    ui.separator();
    if shop.items.is_empty() {
        ui.small("Shop lineup unavailable (open the shop in-game first).");
        return;
    }
    overlay::scroll_list(ui, |ui| {
        draw_lineup(ui, &shop.items);
        draw_owned(ui, &shop.owned);
    });
}

fn draw_header(ui: &mut egui::Ui, shop: &memory_reader::TrackblazerShop) {
    ui.horizontal(|ui| {
        ui.strong(format!("\u{1f3c5} RaceCoins: {}", shop.coins));
        if shop.sale_value > 0 {
            ui.colored_label(Color32::from_rgb(220, 120, 60), format!("Sale {}%", shop.sale_value));
        }
        if shop.win_points > 0 {
            ui.small(format!("WinPt: {}", shop.win_points));
        }
    });
}

fn draw_lineup(ui: &mut egui::Ui, items: &[memory_reader::TrackblazerShopItem]) {
    let w = overlay::content_width();
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    tui(ui, ui.id().with("tb_lineup"))
        .reserve_width(w)
        .style(grid_style(vec![fr(1.0_f32), fr(1.3_f32), auto(), auto(), auto()], w))
        .show(|tui| {
            for h in ["Item", "Effect", "Price", "Avail", "Worth"] {
                header_cell(tui, h);
            }
            for item in items {
                lineup_row(tui, item);
            }
        });
}

fn draw_owned(ui: &mut egui::Ui, owned: &[memory_reader::TrackblazerOwnedItem]) {
    if owned.is_empty() {
        return;
    }
    ui.add_space(8.0);
    ui.separator();
    ui.strong("Owned items");
    let w = overlay::content_width();
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    tui(ui, ui.id().with("tb_owned"))
        .reserve_width(w)
        .style(grid_style(vec![fr(1.0_f32), fr(1.2_f32), auto()], w))
        .show(|tui| {
            for o in owned {
                let name = if o.name.is_empty() {
                    format!("#{}", o.item_id)
                } else {
                    o.name.clone()
                };
                trunc_cell(tui, name, Color32::from_rgb(230, 230, 230), o.name.is_empty());
                let effect = if o.effect.is_empty() {
                    "\u{2014}".to_string()
                } else {
                    o.effect.clone()
                };
                trunc_cell(tui, effect, Color32::from_rgb(180, 180, 180), true);
                plain_cell(tui, |tui| {
                    tui.ui(|ui| {
                        ui.label(format!("\u{00d7}{}", o.count));
                    });
                });
            }
        });
}

/// One shop lineup row: Item | Effect | Price | Avail | Worth.
fn lineup_row(tui: &mut Tui, item: &memory_reader::TrackblazerShopItem) {
    let dim = Color32::from_rgb(140, 140, 140);

    let name = if item.name.is_empty() {
        format!("#{}", item.item_id)
    } else {
        item.name.clone()
    };
    trunc_cell(tui, name, Color32::from_rgb(235, 235, 235), item.name.is_empty());

    let effect = if item.effect.is_empty() {
        "\u{2014}".to_string()
    } else {
        item.effect.clone()
    };
    trunc_cell(tui, effect, Color32::from_rgb(200, 200, 200), true);

    // Price (current coin cost + optional strikethrough original).
    plain_cell(tui, |tui| {
        tui.ui(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let price_color = if item.sold_out() {
                    dim
                } else if item.discounted() {
                    Color32::from_rgb(220, 120, 60)
                } else {
                    Color32::from_rgb(230, 200, 90)
                };
                ui.colored_label(price_color, format!("{} \u{1fa99}", item.coin_num));
                if item.discounted() {
                    ui.colored_label(
                        dim,
                        RichText::new(format!("{}", item.original_coin_num)).strikethrough(),
                    );
                }
            });
        });
    });

    // Availability (turns left).
    plain_cell(tui, |tui| {
        tui.ui(|ui| {
            if item.turns_left > 0 {
                ui.colored_label(Color32::from_rgb(220, 120, 60), format!("{} turn(s)", item.turns_left));
            } else {
                ui.small("\u{2014}");
            }
        });
    });

    // Worth rating.
    plain_cell(tui, |tui| {
        tui.ui(|ui| match item.worth {
            Some(w) => {
                ui.colored_label(worth_color(w), w.label());
            }
            None => {
                ui.small("\u{2014}");
            }
        });
    });
}

/// Shared grid container style: `columns` tracks, pinned to `width`.
fn grid_style(columns: Vec<taffy::TrackSizingFunction>, width: f32) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Grid,
        grid_template_columns: columns,
        gap: taffy::Size {
            width: length(dimens::z(dimens::GRID_GAP_X)),
            height: length(dimens::z(dimens::GAP_SM)),
        },
        align_items: Some(taffy::AlignItems::Center),
        size: taffy::Size {
            width: length(width),
            height: auto(),
        },
        ..Default::default()
    }
}

/// A grid cell that can shrink to 0 (so a truncating label doesn't force the
/// track wider than its fr share).
fn cell_style() -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Start),
        min_size: taffy::Size {
            width: length(0.0_f32),
            height: auto(),
        },
        ..Default::default()
    }
}

fn header_cell(tui: &mut Tui, text: &str) {
    tui.style(cell_style()).add(|tui| {
        tui.ui(|ui| {
            ui.strong(text);
        });
    });
}

/// A truncating text cell. Reports a constant width-independent size so the
/// truncating label doesn't feed its assigned width back into fr-track sizing
/// (which spins Taffy every frame — see career/bonds.rs).
fn trunc_cell(tui: &mut Tui, text: String, color: Color32, small: bool) {
    tui.style(cell_style()).add(|tui| {
        tui.ui_manual(|ui, _| {
            let mut rt = RichText::new(&text).color(color);
            if small {
                rt = rt.small();
            }
            ui.add(egui::Label::new(rt).truncate());
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
}

/// A content-sized (`auto` track) cell holding fixed-size widgets.
fn plain_cell(tui: &mut Tui, content: impl FnOnce(&mut Tui)) {
    tui.style(taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        ..Default::default()
    })
    .add(content);
}
