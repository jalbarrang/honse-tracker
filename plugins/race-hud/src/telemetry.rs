//! Conversions from race state to telemetry protobuf, plus publish entry points
//! called from `state.rs`. Pure data mapping — no IL2CPP access. No-op when
//! telemetry is disabled or the channel is off.

use hachimi_telemetry::pb;
use hachimi_telemetry::Channel;

use crate::sim::DecodedRace;
use crate::state::RunnerRow;

const SOURCE: &str = "race-hud";

/// Publish a live frame (~100ms cadence) from the sampled rows.
pub fn publish_live(elapsed: f32, rows: &[RunnerRow]) {
    if !hachimi_telemetry::channel_enabled(Channel::RaceLive) {
        return;
    }
    let frame = pb::RaceLiveFrame {
        elapsed,
        rows: rows.iter().map(row_to_pb).collect(),
    };
    hachimi_telemetry::publish(SOURCE, pb::envelope::Payload::RaceLiveFrame(frame));
}

/// Publish the full decoded race one-shot when a new SimData is captured.
pub fn publish_full(decoded: &DecodedRace, names: &[String], mine: &[bool]) {
    if !hachimi_telemetry::channel_enabled(Channel::RaceFull) {
        return;
    }
    let s = &decoded.summary;
    let full = pb::RaceFull {
        summary: Some(pb::RaceSummary {
            version: s.version,
            horse_num: s.horse_num,
            frame_count: s.frame_count,
            frame_size: s.frame_size,
            horse_frame_size: s.horse_frame_size,
            distance_diff_max: s.distance_diff_max,
            race_length_m: s.race_length_m,
        }),
        names: names.to_vec(),
        is_player: mine.to_vec(),
        frames: decoded
            .frames
            .iter()
            .map(|f| pb::RaceFrame {
                time: f.time,
                runners: f
                    .runners
                    .iter()
                    .map(|r| pb::RunnerSample {
                        distance: r.distance,
                        lane: u32::from(r.lane),
                        speed: u32::from(r.speed),
                        hp: u32::from(r.hp),
                        temptation: i32::from(r.temptation),
                        block_front: i32::from(r.block_front),
                    })
                    .collect(),
            })
            .collect(),
    };
    hachimi_telemetry::publish(SOURCE, pb::envelope::Payload::RaceFull(full));
}

fn row_to_pb(r: &RunnerRow) -> pb::RunnerRow {
    pb::RunnerRow {
        rank: u32::from(r.rank),
        post: u32::from(r.post),
        name: r.name.clone(),
        distance: r.distance,
        speed_raw: u32::from(r.speed),
        hp: u32::from(r.hp),
        accel: r.accel,
        kakari: r.temptation != 0,
        blocked: r.block_front >= 0,
        strategy: u32::from(r.strategy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_maps_states() {
        let row = RunnerRow {
            rank: 2,
            post: 5,
            name: "Runner".to_string(),
            distance: 120.5,
            speed: 1800,
            hp: 450,
            accel: 0.3,
            temptation: 1,
            block_front: 3,
            strategy: 3,
        };
        let pb = row_to_pb(&row);
        assert_eq!(pb.rank, 2);
        assert_eq!(pb.post, 5);
        assert_eq!(pb.speed_raw, 1800);
        assert!(pb.kakari);
        assert!(pb.blocked);
        assert_eq!(pb.strategy, 3);
    }

    #[test]
    fn row_no_states() {
        let row = RunnerRow {
            rank: 1,
            post: 1,
            name: String::new(),
            distance: 0.0,
            speed: 0,
            hp: 500,
            accel: 0.0,
            temptation: 0,
            block_front: -1,
            strategy: 0,
        };
        let pb = row_to_pb(&row);
        assert!(!pb.kakari);
        assert!(!pb.blocked);
        assert_eq!(pb.strategy, 0);
    }
}
