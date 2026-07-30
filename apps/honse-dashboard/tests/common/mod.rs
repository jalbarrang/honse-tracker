//! Shared test fixtures: settled-turn builders over temp databases.

use honse_dashboard::pb;

/// Build a settled turn with the identity/stat fields the grouping and view
/// logic care about.
#[must_use]
pub fn make_turn(capture_id: &str, card_id: i32, scenario_id: i32, turn: i32, captured_at_ms: u64) -> pb::SettledTurn {
    pb::SettledTurn {
        capture_id: capture_id.to_string(),
        captured_at_ms,
        snapshot: Some(pb::CareerSnapshot {
            is_playing: true,
            current_turn: turn,
            card_id,
            scenario_id,
            speed: 500 + turn,
            stamina: 400 + turn,
            power: 450 + turn,
            guts: 300 + turn,
            wiz: 350 + turn,
            total_stats: 2000 + 5 * turn,
            hp: 64,
            max_hp: 100,
            motivation: 5,
            fan_count: 1000 * turn,
            skill_point: 120,
            total_races: 4,
            win_count: 3,
            training_levels: vec![4, 2, 3, 2, 3],
            stat_caps: vec![1200, 1100, 1200, 1000, 1100],
            failure_rates: vec![2, 16, 12, 9, 0],
            stat_gains: vec![41, 19, 25, 27, 25],
            per_stat_gains: vec![
                pb::StatRow {
                    values: vec![24, 0, 11, 0, 6],
                },
                pb::StatRow {
                    values: vec![0, 14, 0, 5, 0],
                },
                pb::StatRow {
                    values: vec![0, 8, 17, 0, 0],
                },
                pb::StatRow {
                    values: vec![5, 0, 6, 16, 0],
                },
                pb::StatRow {
                    values: vec![7, 0, 0, 0, 18],
                },
            ],
            aptitudes: Some(pb::Aptitudes {
                ground_turf: 7,
                dist_mile: 7,
                dist_middle: 6,
                dist_long: 5,
                ..Default::default()
            }),
            ..Default::default()
        }),
        extras: Some(pb::CareerExtras {
            skill_points: Some(420),
            skills: vec![pb::AcquiredSkill {
                master_id: 100,
                level: 2,
                name: "Corner Adept".to_string(),
            }],
            evaluations: vec![
                pb::Evaluation {
                    target_id: 1,
                    value: 82,
                    is_appear: true,
                    name: "Kitasan Black".to_string(),
                    training_facility: Some(0),
                    bond_pressure: Some(0.9),
                    ..Default::default()
                },
                pb::Evaluation {
                    target_id: 2,
                    value: 44,
                    is_appear: true,
                    name: "Vodka".to_string(),
                    training_facility: Some(2),
                    bond_pressure: Some(0.1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }),
    }
}
