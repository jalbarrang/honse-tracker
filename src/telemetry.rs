//! Conversions from the tracker's in-memory career state to telemetry protobuf,
//! plus the publish entry point called from `career_poll`'s settled-turn
//! capture callback.
//!
//! Pure data mapping over already-read structs — no IL2CPP access here. Every
//! call is a cheap no-op when telemetry is disabled.

use hachimi_telemetry::pb;

use crate::evaluation::Aptitudes;
use crate::memory_reader::{
    AcquiredSkillInfo, CareerSnapshot, EvaluationInfo, ReservedRace, ScenarioState, TrackblazerShop,
};

const SOURCE: &str = "training-tracker";

/// Borrowed inputs for one atomic settled-turn capture. The caller creates the
/// idempotency key (`capture_id`) and timestamp once (`career_poll` derives
/// the id from the career epoch plus a content fingerprint); the encoded body
/// built from them is retained unchanged across delivery retries, so the
/// sidecar can dedupe replays on `capture_id`.
pub struct SettledTurnInput<'a> {
    pub capture_id: &'a str,
    pub captured_at_ms: u64,
    pub snapshot: Option<&'a CareerSnapshot>,
    pub skills: &'a [AcquiredSkillInfo],
    pub evaluations: &'a [EvaluationInfo],
    pub skill_points: Option<i32>,
    pub support_ids: &'a [(i32, i32)],
    pub reserved_races: &'a [ReservedRace],
}

/// Build one atomic `SettledTurn` payload from already-read career state. Pure
/// mapping — no IL2CPP access, no I/O — so it is fully unit-testable.
#[must_use]
pub fn settled_turn_to_pb(input: &SettledTurnInput<'_>) -> pb::SettledTurn {
    let placements = input.snapshot.map(|s| &s.partner_placements);
    pb::SettledTurn {
        capture_id: input.capture_id.to_string(),
        captured_at_ms: input.captured_at_ms,
        snapshot: input.snapshot.map(career_to_pb),
        extras: Some(extras_to_pb(
            input.skills,
            input.evaluations,
            input.skill_points,
            input.support_ids,
            placements,
            input.reserved_races,
        )),
    }
}

/// Publish one already-built atomic settled turn (snapshot + extras in a
/// single envelope). Never blocks; a cheap no-op when telemetry is disabled.
/// The capture callback builds the payload first so it can fingerprint the
/// content for deduplication before deciding to publish. The legacy split
/// `CareerSnapshot`/`CareerExtras` envelopes are no longer emitted (the
/// sidecar only accepts `settled_turn`); the proto variants remain decodable.
pub fn publish_settled_turn(turn: pb::SettledTurn) {
    if !hachimi_telemetry::is_enabled() {
        return;
    }
    hachimi_telemetry::publish(SOURCE, pb::envelope::Payload::SettledTurn(turn));
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
        let extras = extras_to_pb(&skills, &[], Some(500), &[(1, 30001), (2, 30002)], None, &reserved);
        assert_eq!(extras.skills.len(), 1);
        assert_eq!(extras.skills[0].master_id, 100);
        assert_eq!(extras.skill_points, Some(500));
        assert_eq!(extras.deck.len(), 2);
        assert_eq!(extras.deck[1].support_card_id, 30002);
        assert_eq!(extras.reserved_races.len(), 2);
        assert_eq!(extras.reserved_races[1].program_id, 1002);
    }

    /// One atomic mapping must carry every field that previously crossed two
    /// independent envelopes: the full snapshot channel, the full extras
    /// channel, and the snapshot->extras `partner_placements` coupling.
    #[test]
    fn settled_turn_binds_snapshot_and_extras_atomically() {
        let mut snap = CareerSnapshot {
            is_playing: true,
            current_turn: 24,
            month: 7,
            speed: 900,
            stamina: 500,
            power: 600,
            guts: 400,
            wiz: 450,
            total_stats: 2850,
            hp: 80,
            max_hp: 100,
            motivation: 4,
            fan_count: 12345,
            card_id: 100101,
            skill_point: 321,
            total_races: 9,
            win_count: 6,
            star: 3,
            scenario_command_base: 101,
            scenario_id: 1,
            ..Default::default()
        };
        snap.training_levels = [1, 2, 3, 4, 5];
        snap.stat_caps = [1200, 1100, 1000, 900, 800];
        snap.failure_rates = [0, 5, 10, 15, 20];
        snap.stat_gains = [12, 0, 4, 0, 0];
        snap.per_stat_gains[0] = [10, 0, 2, 0, 0];
        snap.per_facility_bond_pressure = [0.1, 0.0, 0.5, 0.0, 0.0];
        snap.evaluation_value = Some(15000);
        snap.chara_effect_ids = vec![7, 8];
        // Coupling that used to cross the two envelopes: extras evaluations pull
        // facility/pressure from the snapshot's partner placements.
        snap.partner_placements.insert(9, (2, 0.75));

        let skills = [AcquiredSkillInfo {
            master_id: 200,
            level: 1,
            name: "Skill".to_string(),
        }];
        let evaluations = [EvaluationInfo {
            target_id: 9,
            value: 80,
            is_appear: true,
            name: "Partner".to_string(),
            story_step: 2,
            guest_chara_id: 0,
        }];
        let reserved = [ReservedRace {
            year: 2,
            program_id: 1001,
        }];

        let turn = settled_turn_to_pb(&SettledTurnInput {
            capture_id: "career-1-turn-24",
            captured_at_ms: 1_700_000_000_000,
            snapshot: Some(&snap),
            skills: &skills,
            evaluations: &evaluations,
            skill_points: Some(777),
            support_ids: &[(1, 30001), (2, 30002)],
            reserved_races: &reserved,
        });

        // Caller-supplied identity is passed through untouched.
        assert_eq!(turn.capture_id, "career-1-turn-24");
        assert_eq!(turn.captured_at_ms, 1_700_000_000_000);

        // Snapshot channel fields.
        let s = turn.snapshot.as_ref().expect("snapshot present");
        assert!(s.is_playing);
        assert_eq!(s.current_turn, 24);
        assert_eq!(s.month, 7);
        assert_eq!(
            (s.speed, s.stamina, s.power, s.guts, s.wiz, s.total_stats),
            (900, 500, 600, 400, 450, 2850)
        );
        assert_eq!((s.hp, s.max_hp, s.motivation), (80, 100, 4));
        assert_eq!((s.fan_count, s.card_id, s.skill_point), (12345, 100101, 321));
        assert_eq!((s.total_races, s.win_count, s.star), (9, 6, 3));
        assert_eq!(s.training_levels, vec![1, 2, 3, 4, 5]);
        assert_eq!(s.stat_caps, vec![1200, 1100, 1000, 900, 800]);
        assert!(s.aptitudes.is_some());
        assert_eq!(s.evaluation_value, Some(15000));
        assert_eq!(s.failure_rates, vec![0, 5, 10, 15, 20]);
        assert_eq!(s.stat_gains, vec![12, 0, 4, 0, 0]);
        assert_eq!(s.per_stat_gains[0].values, vec![10, 0, 2, 0, 0]);
        assert_eq!(s.per_facility_bond_pressure, vec![0.1, 0.0, 0.5, 0.0, 0.0]);
        assert_eq!((s.scenario_command_base, s.scenario_id), (101, 1));
        assert!(s.scenario_state_json.is_empty());
        assert_eq!(s.chara_effect_ids, vec![7, 8]);

        // Extras channel fields, in the same payload.
        let e = turn.extras.as_ref().expect("extras present");
        assert_eq!(e.skills.len(), 1);
        assert_eq!((e.skills[0].master_id, e.skills[0].level), (200, 1));
        assert_eq!(e.skill_points, Some(777));
        assert_eq!(e.deck.len(), 2);
        assert_eq!(e.deck[1].support_card_id, 30002);
        assert_eq!(e.reserved_races.len(), 1);
        assert_eq!(e.reserved_races[0].program_id, 1001);
        // The cross-envelope coupling now resolves inside one payload.
        assert_eq!(e.evaluations.len(), 1);
        assert_eq!(e.evaluations[0].target_id, 9);
        assert_eq!(e.evaluations[0].training_facility, Some(2));
        assert_eq!(e.evaluations[0].bond_pressure, Some(0.75));
    }

    /// Without a snapshot the payload still forms, with extras standalone.
    #[test]
    fn settled_turn_without_snapshot_keeps_extras() {
        let turn = settled_turn_to_pb(&SettledTurnInput {
            capture_id: "career-1-turn-0",
            captured_at_ms: 42,
            snapshot: None,
            skills: &[],
            evaluations: &[],
            skill_points: None,
            support_ids: &[],
            reserved_races: &[],
        });
        assert!(turn.snapshot.is_none());
        let extras = turn.extras.expect("extras present");
        assert!(extras.skills.is_empty());
        assert_eq!(extras.skill_points, None);
    }
}
