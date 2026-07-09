//! Skill Shop tab: SP + filters + purchasable list (scrollable).

use crate::compat::egui::{self, Color32, RichText, Vec2, Vec2b};
use egui_taffy::taffy::prelude::{auto, length};
use egui_taffy::{taffy, tui, TuiBuilderLogic, TuiContainerResponse};

use crate::overlay_cache;
use crate::skill_shop;
use crate::skill_shop_prefs::{cycle_sort_mode, prefs, set_prefs, sort_mode_label, DistanceFilter, StyleFilter};

use std::sync::Mutex;

use super::dimens;
use super::overlay;

/// Skill id awaiting a second-click confirm before the purchase fires.
static CONFIRM: Mutex<Option<i32>> = Mutex::new(None);

pub(super) fn draw(ui: &mut egui::Ui) {
    overlay_cache::maybe_request_refresh();
    if overlay_cache::snapshot().is_none() {
        ui.small("Loading shop data…");
        return;
    }

    draw_header(ui);
    draw_controls(ui);
    ui.separator();
    overlay::scroll_list(ui, draw_list);
}

fn draw_header(ui: &mut egui::Ui) {
    if let Some(sp) = overlay_cache::skill_points() {
        ui.strong(format!("SP: {}", sp));
    }
}

fn draw_controls(ui: &mut egui::Ui) {
    let p = prefs();

    if ui
        .small_button(format!("Sort: {}", sort_mode_label(p.sort_mode)))
        .clicked()
    {
        cycle_sort_mode();
    }

    ui.horizontal_wrapped(|ui| {
        ui.small("Style:");
        for &(label, filter) in StyleFilter::LABELS {
            let selected = p.style_filter == filter;
            if ui
                .small_button(format!("{}{}", if selected { "*" } else { "" }, label))
                .clicked()
            {
                set_prefs(|prefs| prefs.style_filter = filter);
            }
        }
    });

    ui.horizontal_wrapped(|ui| {
        ui.small("Dist:");
        for &(label, filter) in DistanceFilter::LABELS {
            let selected = p.distance_filter == filter;
            if ui
                .small_button(format!("{}{}", if selected { "*" } else { "" }, label))
                .clicked()
            {
                set_prefs(|prefs| prefs.distance_filter = filter);
            }
        }
    });

    let mut show_hintless = p.show_hintless;
    if ui.checkbox(&mut show_hintless, "Show full-price (no hint)").changed() {
        set_prefs(|prefs| prefs.show_hintless = show_hintless);
    }
    if show_hintless {
        ui.small("Open the in-game skill shop once to capture purchasable rows.");
    }
}

fn draw_list(ui: &mut egui::Ui) {
    let entries = skill_shop::prepare_entries_for_display(overlay_cache::skill_shop(), &prefs());
    if entries.is_empty() {
        ui.small("No shop skills match filters");
        return;
    }

    let sp = overlay_cache::skill_points().unwrap_or(0);
    let w = overlay::content_width();
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    for (idx, entry) in entries.iter().enumerate() {
        shop_row(ui, idx, w, entry, sp);
    }
}

/// One skill as a single aligned row: `[ Name | Hint | Cost | Action ]`.
/// Owned skills render struck-through; unaffordable skills render dimmed.
fn shop_row(ui: &mut egui::Ui, idx: usize, w: f32, entry: &skill_shop::SkillShopEntry, sp: i32) {
    let icon = skill_shop::rarity_label(entry.rarity);
    let name = if entry.name.is_empty() {
        format!("#{}", entry.group_id)
    } else {
        entry.name.clone()
    };
    let name_text = format!("{icon} {name}");

    let discount = skill_shop::discount_pct(entry.hint_level, false);
    let hint_text = if discount > 0 {
        format!("-{discount}%")
    } else {
        String::new()
    };
    let cost = (entry.base_cost > 0).then(|| skill_shop::discounted_cost(entry.base_cost, entry.hint_level, false));
    let cost_text = cost.map(|c| format!("{c}pt")).unwrap_or_default();

    let learned = entry.is_learned;
    let affordable = cost.is_some_and(|c| sp >= c);

    // Triage colors: owned → grey struck-through; can't-afford → dimmed; else bright.
    let base_color = if entry.rarity >= 2 {
        Color32::from_rgb(255, 200, 50)
    } else {
        Color32::from_rgb(220, 220, 220)
    };
    let (name_color, cost_color) = if learned {
        (Color32::from_rgb(105, 105, 105), Color32::from_rgb(95, 95, 95))
    } else if !affordable {
        (Color32::from_rgb(120, 120, 120), Color32::from_rgb(135, 105, 105))
    } else {
        let cc = if discount > 0 {
            Color32::from_rgb(120, 200, 120)
        } else {
            Color32::from_rgb(175, 175, 175)
        };
        (base_color, cc)
    };
    let hint_color = if learned {
        Color32::from_rgb(95, 95, 95)
    } else {
        Color32::from_rgb(120, 200, 120)
    };

    let col = |width: f32| taffy::Style {
        display: taffy::Display::Flex,
        align_items: Some(taffy::AlignItems::Center),
        justify_content: Some(taffy::JustifyContent::End),
        size: taffy::Size {
            width: length(width),
            height: auto(),
        },
        ..Default::default()
    };

    tui(ui, ui.id().with("shop_row").with(idx))
        .reserve_width(w)
        .style(taffy::Style {
            display: taffy::Display::Flex,
            flex_direction: taffy::FlexDirection::Row,
            align_items: Some(taffy::AlignItems::Center),
            gap: taffy::Size {
                width: length(dimens::z(dimens::GAP_SM)),
                height: length(0.0),
            },
            size: taffy::Size {
                width: length(w),
                height: auto(),
            },
            ..Default::default()
        })
        .show(|tui| {
            // Name fills and truncates; constant reported size avoids the
            // relayout feedback loop (see career/bonds.rs).
            tui.style(taffy::Style {
                display: taffy::Display::Flex,
                flex_grow: 1.0,
                align_items: Some(taffy::AlignItems::Center),
                justify_content: Some(taffy::JustifyContent::Start),
                min_size: taffy::Size {
                    width: length(0.0),
                    height: auto(),
                },
                ..Default::default()
            })
            .add(|tui| {
                tui.ui_manual(|ui, _| {
                    let mut rt = RichText::new(&name_text).small().color(name_color);
                    if learned {
                        rt = rt.strikethrough();
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
            // Hint column.
            tui.style(col(dimens::z(40.0))).add(|tui| {
                tui.ui(|ui| {
                    if !hint_text.is_empty() {
                        ui.label(RichText::new(&hint_text).small().color(hint_color));
                    }
                });
            });
            // Cost column.
            tui.style(col(dimens::z(52.0))).add(|tui| {
                tui.ui(|ui| {
                    if !cost_text.is_empty() {
                        let mut rt = RichText::new(&cost_text).small().strong().color(cost_color);
                        if learned {
                            rt = rt.strikethrough();
                        }
                        ui.label(rt);
                    }
                });
            });
            // Action column.
            tui.style(col(dimens::z(74.0))).add(|tui| {
                tui.ui(|ui| {
                    draw_action(ui, entry.skill_id, &name, cost, learned, affordable);
                });
            });
        });
}

/// Action cell: owned → “✓ own”; affordable → `➕ Buy` then a `✓/✕` confirm;
/// unaffordable → a disabled `Buy`.
fn draw_action(ui: &mut egui::Ui, skill_id: i32, name: &str, cost: Option<i32>, learned: bool, affordable: bool) {
    if learned {
        ui.label(
            RichText::new("\u{2713} own")
                .small()
                .color(Color32::from_rgb(105, 105, 105)),
        );
        return;
    }
    if cost.is_none() {
        return; // unknown cost → not buyable here
    }
    if !affordable {
        ui.add_enabled(false, egui::Button::new(RichText::new("\u{2795} Buy").small()));
        return;
    }
    let confirming = *CONFIRM.lock().expect("lock poisoned") == Some(skill_id);
    ui.horizontal(|ui| {
        if confirming {
            if ui
                .small_button(
                    RichText::new("\u{2713}")
                        .strong()
                        .color(Color32::from_rgb(120, 200, 120)),
                )
                .clicked()
            {
                *CONFIRM.lock().expect("lock poisoned") = None;
                match skill_shop::buy_skill(skill_id, 1) {
                    Ok(spent) => hlog_info!("Skill shop: buying '{}' for {}pt", name, spent),
                    Err(e) => hlog_warn!("Skill shop: buy refused: {}", e),
                }
            }
            if ui
                .small_button(RichText::new("\u{2715}").color(Color32::from_rgb(200, 120, 120)))
                .clicked()
            {
                *CONFIRM.lock().expect("lock poisoned") = None;
            }
        } else if ui.small_button(RichText::new("\u{2795} Buy").small()).clicked() {
            *CONFIRM.lock().expect("lock poisoned") = Some(skill_id);
        }
    });
}
