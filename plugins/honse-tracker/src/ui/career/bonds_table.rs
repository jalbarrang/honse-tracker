//! Bonds table for the Career overlay panel (pure egui).
//!
//! `bonds.rs` collects + sorts the bond rows (resolving icon paths and theme
//! colours), then hands them here. We render the table with `egui_taffy`: fixed-
//! width right columns so the Type / Value / On columns line up across rows, while
//! the name cell flexes. Each row is a framed card (rainbow border when the card
//! can friendship-train this turn).

use crate::compat::egui::{self, Color32, CornerRadius, Pos2, Rect, RichText, Stroke, Vec2};
use egui_taffy::taffy::prelude::length;
use egui_taffy::{taffy, tui, TuiBuilderLogic};

use std::cell::RefCell;

use super::super::dimens;
use super::super::textures;
use super::theme;

/// A fully-resolved bond row: every visual is a plain value (colours as
/// [`Color32`], icons as `icons/`-relative PNG paths) so the table needs no
/// game/IL2CPP access.
#[derive(Clone, Default)]
pub(super) struct BondRow {
    pub name: String,
    pub value: i32,
    /// Bond value colour.
    pub value_color: Color32,
    /// Specialty stat chip: icon path + chip background colour. `None` for
    /// pal/friend/group/guest cards.
    pub type_icon: Option<String>,
    pub type_chip_bg: Option<Color32>,
    /// Glyph for friend/group cards when there's no specialty chip.
    pub type_glyph: Option<String>,
    /// Facility trained on this turn: icon path + chip background colour.
    pub on_icon: Option<String>,
    pub on_chip_bg: Option<Color32>,
    /// Rainbow-ready (specialty bond >= 80 on its own facility this turn).
    pub rainbow: bool,
}

thread_local! {
    /// Last logged row signature, so the per-frame render only logs when the
    /// row set actually changes (avoids flooding `hachimi.log` every frame).
    static LAST_SIG: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Diagnostic: dump what the table layer *receives* (name + bond value + resolved
/// "On" facility) whenever the row set changes. The "On" facility is recovered
/// from the chip colour (`on_chip_bg`); `-` means the card trained nowhere.
fn log_rows_on_change(rows: &[BondRow]) {
    let sig = rows
        .iter()
        .map(|r| {
            let on = r.on_chip_bg.map(|c| format!("{:?}", c)).unwrap_or_else(|| "-".into());
            format!("{}={}@{}", r.name, r.value, on)
        })
        .collect::<Vec<_>>()
        .join(",");
    let changed = LAST_SIG.with(|s| {
        let mut s = s.borrow_mut();
        if *s == sig {
            false
        } else {
            *s = sig.clone();
            true
        }
    });
    if changed {
        hlog_info!(
            target: "training-tracker",
            "bonds_table rows ({}): {}",
            rows.len(),
            sig
        );
    }
}

const C_RAINBOW: Color32 = Color32::from_rgb(0x9a, 0x8c, 0xff);

/// Render the table into `ui` (filling the available width).
pub(super) fn render(ui: &mut egui::Ui, rows: Vec<BondRow>) {
    log_rows_on_change(&rows);

    let w = super::super::overlay::content_width();
    let font = dimens::z(dimens::FONT_SM);
    let chip = dimens::z(dimens::ICON_LG);
    let row_gap = dimens::z(dimens::GAP_XS);

    header_row(ui, w, font);
    for (idx, row) in rows.iter().enumerate() {
        ui.add_space(row_gap);
        bond_row(ui, idx, row, w, font, chip);
    }
}

fn header_row(ui: &mut egui::Ui, w: f32, font: f32) {
    let pad = dimens::z(dimens::GAP_SM);
    egui::Frame::new()
        .inner_margin(egui::Margin::same(pad as i8))
        .show(ui, |ui| {
            ui.set_width(w);
            let name_w = name_width(w);
            flex_row(ui, ui.id().with("bonds_header"), w, |tui| {
                tui.style(name_col(name_w)).add(|tui| {
                    tui.ui(|ui| {
                        ui.set_max_width(name_w);
                        ui.label(RichText::new("Name").color(theme::FG_MUTED).size(font));
                    });
                });
                header_col(tui, "Type", col_type(), font);
                header_col(tui, "Value", col_bond(), font);
                header_col(tui, "On", col_on(), font);
            });
        });
}

fn header_col(tui: &mut egui_taffy::Tui, text: &str, width: f32, font: f32) {
    tui.style(fixed_col(width)).add(|tui| {
        tui.label(RichText::new(text).color(theme::FG_MUTED).size(font));
    });
}

fn bond_row(ui: &mut egui::Ui, idx: usize, row: &BondRow, w: f32, font: f32, chip: f32) {
    // Wider side padding, tight top/bottom so each row reads as a compact band.
    let pad_x = dimens::z(dimens::GAP_SM);
    let pad_y = dimens::z(dimens::GAP_XS);
    let border = if row.rainbow { C_RAINBOW } else { theme::LINE };
    let border_w = if row.rainbow { 1.5 } else { 1.0 };

    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(pad_x as i8, pad_y as i8))
        .corner_radius(CornerRadius::same(8))
        .fill(theme::SURFACE_2)
        .stroke(Stroke::new(border_w, border))
        .show(ui, |ui| {
            let inner = (w - 2.0 * pad_x).max(40.0);
            ui.set_width(inner);
            let name_w = name_width(inner);
            flex_row(ui, ui.id().with("bond_row").with(idx), inner, |tui| {
                // Name (fixed-width column, truncates to fit).
                tui.style(name_col(name_w)).add(|tui| {
                    tui.ui(|ui| {
                        ui.set_max_width(name_w);
                        ui.add(
                            egui::Label::new(RichText::new(&row.name).strong().color(theme::FG).size(font)).truncate(),
                        );
                    });
                });
                // Type.
                tui.style(fixed_col(col_type())).add(|tui| {
                    tui.ui(|ui| type_cell(ui, row, chip, font));
                });
                // Value.
                tui.style(fixed_col(col_bond())).add(|tui| {
                    tui.ui(|ui| {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(
                                RichText::new(row.value.to_string())
                                    .strong()
                                    .color(row.value_color)
                                    .size(font),
                            );
                            ui.label(RichText::new("/100").color(theme::FG_DIM).size(font));
                        });
                    });
                });
                // On.
                tui.style(fixed_col(col_on())).add(|tui| {
                    tui.ui(|ui| chip_cell(ui, row.on_icon.as_deref(), row.on_chip_bg, chip));
                });
            });
        });
}

/// Type cell: specialty stat chip, pal/friend/group glyph, or a dash.
fn type_cell(ui: &mut egui::Ui, row: &BondRow, chip: f32, font: f32) {
    if row.type_icon.is_some() {
        chip_cell(ui, row.type_icon.as_deref(), row.type_chip_bg, chip);
    } else if let Some(glyph) = &row.type_glyph {
        ui.label(RichText::new(glyph).size(font));
    } else {
        ui.label(RichText::new("\u{2013}").color(theme::FG_DIM).size(font));
    }
}

/// A coloured rounded chip wrapping a stat icon, or a dash when absent.
fn chip_cell(ui: &mut egui::Ui, icon: Option<&str>, bg: Option<Color32>, chip: f32) {
    match (icon, bg) {
        (Some(rel), Some(bg)) => {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(chip), egui::Sense::hover());
            ui.painter().rect_filled(rect, CornerRadius::same(4), bg);
            if let Some(tex) = textures::texture(ui.ctx(), rel) {
                let s = chip * 0.82;
                let ir = Rect::from_center_size(rect.center(), Vec2::splat(s));
                ui.painter().image(
                    tex.id(),
                    ir,
                    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
        }
        _ => {
            ui.label(RichText::new("\u{2013}").color(theme::FG_DIM));
        }
    }
}

// ── Column widths (base values, zoom-scaled) ──
fn col_type() -> f32 {
    dimens::z(32.0)
}
fn col_on() -> f32 {
    dimens::z(32.0)
}
fn col_bond() -> f32 {
    dimens::z(56.0)
}

/// Run a horizontal taffy flex row of the given width with centered, gapped items.
fn flex_row(ui: &mut egui::Ui, id: egui::Id, width: f32, f: impl FnOnce(&mut egui_taffy::Tui)) {
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    tui(ui, id)
        .reserve_width(width)
        .style(taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            align_items: Some(taffy::AlignItems::Center),
            gap: taffy::Size {
                width: length(dimens::z(dimens::GAP_MD)),
                height: length(0.0),
            },
            size: taffy::Size {
                width: length(width),
                height: taffy::prelude::auto(),
            },
            ..Default::default()
        })
        .show(f);
}

/// A fixed-width, horizontally-centered column.
fn fixed_col(width: f32) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Center),
        size: taffy::Size {
            width: length(width),
            height: taffy::prelude::auto(),
        },
        ..Default::default()
    }
}

/// Remaining width for the name column = total minus the three fixed right
/// columns and the inter-column gaps. Computed explicitly so the name cell has a
/// concrete width: a `flex_grow` item measures a `.truncate()` label as ~0 during
/// taffy's layout pass, collapsing the name to just the ellipsis.
fn name_width(total: f32) -> f32 {
    let gap = dimens::z(dimens::GAP_MD);
    (total - col_type() - col_bond() - col_on() - 3.0 * gap).max(40.0)
}

/// The name item: a fixed-width, left-justified column.
fn name_col(width: f32) -> taffy::Style {
    taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::Start),
        size: taffy::Size {
            width: length(width),
            height: taffy::prelude::auto(),
        },
        ..Default::default()
    }
}
