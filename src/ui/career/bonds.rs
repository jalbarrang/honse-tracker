//! Career panel Bonds section: a table of supports/guests with their card type,
//! bond value, and the facility they trained on this turn — with a rainbow border
//! when a card can friendship-train. Rows are grouped by the facility they're
//! training on this turn (cards training nowhere sort last). The table itself is
//! authored in Dioxus (`bonds_table`); this module resolves the data + visuals.

use crate::compat::egui::{self, RichText};

use super::bonds_table::{self, BondRow};
use super::theme;
use crate::career_meta::stat_icon_path;
use crate::gametora_data;
use crate::memory_reader::CareerSnapshot;
use crate::overlay_cache;

/// One resolved bond row.
struct Bond {
    name: String,
    /// Specialty facility (0..4) when the card is a stat card; `None` for
    /// guests / pal-friend / uncatalogued.
    specialty: Option<usize>,
    /// `true` for a pal/friend card (emoji glyph), `false` for `group`.
    is_friend: bool,
    has_type: bool,
    value: i32,
    /// Facility trained on this turn (0..4), from partner placements.
    on_facility: Option<usize>,
    #[allow(dead_code)]
    is_support: bool,
    rainbow_ready: bool,
}

pub(super) fn draw(ui: &mut egui::Ui, snap: &CareerSnapshot) {
    theme::section_strip(ui, "Bonds", "");

    let mut bonds = collect(snap);
    if bonds.is_empty() {
        ui.label(RichText::new("No bond data yet").small().color(theme::FG_DIM));
        return;
    }

    // Group by the facility a card is training on this turn (0..4) so the "On"
    // column stays ordered; cards training nowhere (`None`) sort last. Within a
    // facility group, highest bond value first.
    bonds.sort_by(|a, b| {
        let fa = a.on_facility.unwrap_or(usize::MAX);
        let fb = b.on_facility.unwrap_or(usize::MAX);
        fa.cmp(&fb).then(b.value.cmp(&a.value))
    });

    let rows: Vec<BondRow> = bonds.iter().map(|b| to_row(ui.ctx(), b)).collect();
    bonds_table::render(ui, rows);
}

/// Resolve a [`Bond`] into the plain-value [`BondRow`] the table consumes:
/// icons become `icons/`-relative PNG paths, colours stay [`Color32`].
fn to_row(_ctx: &egui::Context, bond: &Bond) -> BondRow {
    let (type_icon, type_chip_bg, type_glyph) = match bond.specialty {
        Some(f) => (Some(stat_icon_path(f)), Some(theme::stat_color(f)), None),
        None if bond.has_type => (
            None,
            None,
            // 🤝 pal/friend, 👥 group
            Some(if bond.is_friend { "\u{1f91d}" } else { "\u{1f465}" }.to_string()),
        ),
        None => (None, None, None),
    };

    let (on_icon, on_chip_bg) = match bond.on_facility {
        Some(f) => (Some(stat_icon_path(f)), Some(theme::stat_color(f))),
        None => (None, None),
    };

    let value_color = if bond.value >= 80 {
        theme::STAT_POWER
    } else if bond.value >= 60 {
        theme::UMA_400
    } else {
        theme::FG
    };

    BondRow {
        name: bond.name.clone(),
        value: bond.value,
        value_color,
        type_icon,
        type_chip_bg,
        type_glyph,
        on_icon,
        on_chip_bg,
        rainbow: bond.rainbow_ready,
    }
}

fn collect(snap: &CareerSnapshot) -> Vec<Bond> {
    let evals = overlay_cache::evaluations();
    let deck = overlay_cache::equipped_support_ids();
    evals
        .iter()
        .filter(|e| e.is_appear || e.value > 0)
        .map(|e| {
            let support_id = deck
                .iter()
                .find(|(slot, _)| *slot == e.target_id)
                .map(|(_, id)| *id as i64)
                .filter(|id| *id > 0);

            let card = support_id.and_then(gametora_data::support_card);
            let type_str = card.and_then(|c| c.r#type.as_deref());
            let specialty = support_id.and_then(gametora_data::support_specialty_facility);
            let on_facility = snap.partner_placements.get(&e.target_id).map(|(f, _)| *f);

            let name = support_id
                .and_then(gametora_data::support_card_name)
                .map(str::to_owned)
                .filter(|n| !n.is_empty())
                .or_else(|| (!e.name.is_empty()).then(|| e.name.clone()))
                .or_else(|| scenario_npc_name(snap.scenario_id, e.target_id).map(str::to_owned))
                .unwrap_or_else(|| format!("#{}", e.target_id));

            let is_support = e.guest_chara_id <= 0 && (support_id.is_some() || (1..=6).contains(&e.target_id));
            // Rainbow fires only on the card's own specialty facility at bond >= 80.
            let rainbow_ready = specialty.is_some() && e.value >= 80 && on_facility == specialty;

            Bond {
                name,
                specialty,
                is_friend: type_str == Some("friend"),
                has_type: type_str.is_some(),
                value: e.value,
                on_facility,
                is_support,
                rainbow_ready,
            }
        })
        .collect()
}

/// Scenario NPC names (not real support cards), keyed by scenario + target id.
fn scenario_npc_name(scenario_id: i32, target_id: i32) -> Option<&'static str> {
    match (scenario_id, target_id) {
        (4, 102) => Some("Director Akikawa"),
        (4, 103) => Some("Etsuko Otonashi"),
        _ => None,
    }
}
