//! Conversions from the tracker's in-memory career state to telemetry protobuf,
//! plus the publish entry point called from `overlay_cache::refresh_cache_cb`.
//!
//! Pure data mapping over already-read structs — no IL2CPP access here. Every
//! call is a cheap no-op when telemetry is disabled or the channel is off.

use hachimi_telemetry::pb;
use hachimi_telemetry::Channel;

use crate::evaluation::Aptitudes;
use crate::memory_reader::{
    AcquiredSkillInfo, CareerSnapshot, EvaluationInfo, ReservedRace, ScenarioState, TrackblazerShop,
};
use crate::skill_shop::SkillShopEntry;

const SOURCE: &str = "training-tracker";

/// Publish the career snapshot and extras. Called once per cache refresh after
/// `CACHE` is populated. Each channel is gated independently.
pub fn publish(
    snapshot: Option<&CareerSnapshot>,
    skills: &[AcquiredSkillInfo],
    evaluations: &[EvaluationInfo],
    skill_shop: &[SkillShopEntry],
    skill_points: Option<i32>,
    support_ids: &[(i32, i32)],
    reserved_races: &[ReservedRace],
) {
    if !hachimi_telemetry::is_enabled() {
        return;
    }

    if hachimi_telemetry::channel_enabled(Channel::Career) {
        if let Some(snap) = snapshot {
            if snap.is_playing {
                hachimi_telemetry::publish(SOURCE, pb::envelope::Payload::CareerSnapshot(career_to_pb(snap)));
            }
        }
    }

    if hachimi_telemetry::channel_enabled(Channel::CareerExtras) {
        let placements = snapshot.map(|s| &s.partner_placements);
        let extras = extras_to_pb(
            skills,
            evaluations,
            skill_shop,
            skill_points,
            support_ids,
            placements,
            reserved_races,
        );
        hachimi_telemetry::publish(SOURCE, pb::envelope::Payload::CareerExtras(extras));
    }
}

fn aptitudes_to_pb(a: &Aptitudes) -> pb::Aptitudes {
    pb::Aptitudes {
        dist_short: a.dist_short,
        dist_mile: a.dist_mile,
        dist_middle: a.dist_middle,
        dist_long: a.dist_long,
        style_nige: a.style_nige,
        style_senko: a.style_senko,
        style_sashi: a.style_sashi,
        style_oikomi: a.style_oikomi,
        ground_turf: a.ground_turf,
        ground_dirt: a.ground_dirt,
    }
}

fn career_to_pb(s: &CareerSnapshot) -> pb::CareerSnapshot {
    pb::CareerSnapshot {
        is_playing: s.is_playing,
        current_turn: s.current_turn,
        month: s.month,
        speed: s.speed,
        stamina: s.stamina,
        power: s.power,
        guts: s.guts,
        wiz: s.wiz,
        total_stats: s.total_stats,
        hp: s.hp,
        max_hp: s.max_hp,
        motivation: s.motivation,
        fan_count: s.fan_count,
        card_id: s.card_id,
        skill_point: s.skill_point,
        total_races: s.total_races,
        win_count: s.win_count,
        training_levels: s.training_levels.to_vec(),
        stat_caps: s.stat_caps.to_vec(),
        aptitudes: Some(aptitudes_to_pb(&s.aptitudes)),
        star: s.star,
        evaluation_value: s.evaluation_value,
        failure_rates: s.failure_rates.to_vec(),
        stat_gains: s.stat_gains.to_vec(),
        per_stat_gains: s
            .per_stat_gains
            .iter()
            .map(|row| pb::StatRow { values: row.to_vec() })
            .collect(),
        per_facility_bond_pressure: s.per_facility_bond_pressure.to_vec(),
        scenario_command_base: s.scenario_command_base,
        scenario_id: s.scenario_id,
        scenario_state_json: scenario_state_json(s.scenario_state.as_ref()),
        chara_effect_ids: s.chara_effect_ids.clone(),
    }
}

/// Serialize the scenario-specific state to JSON. Empty string when none.
/// The scenario types don't derive Serialize, so build the JSON explicitly.
fn scenario_state_json(state: Option<&ScenarioState>) -> String {
    match state {
        None => String::new(),
        Some(ScenarioState::Trackblazer(shop)) => trackblazer_json(shop).to_string(),
    }
}

#[allow(clippy::disallowed_methods)] // serde_json::json! expands to internal unwrap()
fn trackblazer_json(shop: &TrackblazerShop) -> serde_json::Value {
    let items: Vec<serde_json::Value> = shop
        .items
        .iter()
        .map(|i| {
            serde_json::json!({
                "item_id": i.item_id,
                "name": i.name,
                "effect": i.effect,
                "worth": i.worth.map(|w| format!("{w:?}")),
                "coin_num": i.coin_num,
                "original_coin_num": i.original_coin_num,
                "bought": i.bought,
                "limit": i.limit,
                "turns_left": i.turns_left,
            })
        })
        .collect();
    let owned: Vec<serde_json::Value> = shop
        .owned
        .iter()
        .map(|o| {
            serde_json::json!({
                "item_id": o.item_id,
                "name": o.name,
                "effect": o.effect,
                "count": o.count,
            })
        })
        .collect();
    serde_json::json!({
        "scenario": "trackblazer",
        "coins": shop.coins,
        "sale_value": shop.sale_value,
        "win_points": shop.win_points,
        "items": items,
        "owned": owned,
    })
}

fn extras_to_pb(
    skills: &[AcquiredSkillInfo],
    evaluations: &[EvaluationInfo],
    skill_shop: &[SkillShopEntry],
    skill_points: Option<i32>,
    support_ids: &[(i32, i32)],
    partner_placements: Option<&std::collections::HashMap<i32, (usize, f32)>>,
    reserved_races: &[ReservedRace],
) -> pb::CareerExtras {
    pb::CareerExtras {
        skills: skills
            .iter()
            .map(|s| pb::AcquiredSkill {
                master_id: s.master_id,
                level: s.level,
                name: s.name.clone(),
            })
            .collect(),
        evaluations: evaluations
            .iter()
            .map(|e| {
                let (training_facility, bond_pressure) = partner_placements
                    .and_then(|m| m.get(&e.target_id))
                    .map(|(fac, p)| (Some(*fac as i32), Some(*p)))
                    .unwrap_or((None, None));
                pb::Evaluation {
                    target_id: e.target_id,
                    value: e.value,
                    is_appear: e.is_appear,
                    name: e.name.clone(),
                    story_step: e.story_step,
                    guest_chara_id: e.guest_chara_id,
                    training_facility,
                    bond_pressure,
                }
            })
            .collect(),
        skill_shop: skill_shop
            .iter()
            .map(|e| pb::SkillShopEntry {
                skill_id: e.skill_id,
                group_id: e.group_id,
                rarity: e.rarity,
                hint_level: e.hint_level,
                name: e.name.clone(),
                base_cost: e.base_cost,
                is_learned: e.is_learned,
                has_hint: e.has_hint,
                tags: e.tags.clone(),
                filter_switch: e.filter_switch,
            })
            .collect(),
        skill_points,
        deck: support_ids
            .iter()
            .map(|&(slot, support_card_id)| pb::SupportSlot { slot, support_card_id })
            .collect(),
        reserved_races: reserved_races
            .iter()
            .map(|r| pb::ReservedRace {
                year: r.year,
                program_id: r.program_id,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn career_snapshot_maps_core_fields() {
        let mut snap = CareerSnapshot {
            is_playing: true,
            current_turn: 12,
            speed: 800,
            stamina: 400,
            ..Default::default()
        };
        snap.training_levels = [1, 2, 3, 4, 5];
        snap.per_stat_gains[0] = [10, 0, 2, 0, 0];
        snap.evaluation_value = Some(15000);
        let pb = career_to_pb(&snap);
        assert!(pb.is_playing);
        assert_eq!(pb.current_turn, 12);
        assert_eq!(pb.speed, 800);
        assert_eq!(pb.training_levels, vec![1, 2, 3, 4, 5]);
        assert_eq!(pb.per_stat_gains.len(), 5);
        assert_eq!(pb.per_stat_gains[0].values, vec![10, 0, 2, 0, 0]);
        assert_eq!(pb.evaluation_value, Some(15000));
        assert!(pb.scenario_state_json.is_empty());
    }

    #[test]
    fn extras_maps_deck_and_skills() {
        let skills = [AcquiredSkillInfo {
            master_id: 100,
            level: 2,
            name: "Test".to_string(),
        }];
        let reserved = [
            ReservedRace {
                year: 2,
                program_id: 1001,
            },
            ReservedRace {
                year: 3,
                program_id: 1002,
            },
        ];
        let extras = extras_to_pb(&skills, &[], &[], Some(500), &[(1, 30001), (2, 30002)], None, &reserved);
        assert_eq!(extras.skills.len(), 1);
        assert_eq!(extras.skills[0].master_id, 100);
        assert_eq!(extras.skill_points, Some(500));
        assert_eq!(extras.deck.len(), 2);
        assert_eq!(extras.deck[1].support_card_id, 30002);
        assert_eq!(extras.reserved_races.len(), 2);
        assert_eq!(extras.reserved_races[1].program_id, 1002);
    }
}
