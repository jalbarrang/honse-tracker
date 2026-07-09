//! Shared plugin state: decoded race + names + per-uma player flags + live snapshot.

use std::sync::{Mutex, OnceLock};

use crate::sim::DecodedRace;

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

/// One runner row in the live feed (sorted by distance, leader first).
#[derive(Clone, Debug)]
pub struct RunnerRow {
    pub rank: u8,
    pub post: u8,
    #[allow(dead_code)]
    pub name: String,
    pub distance: f32,
    pub speed: u16,
    pub hp: u16,
    /// Acceleration in m/s², derived from the speed delta to the previous frame.
    pub accel: f32,
    /// `RaceSimulateHorseFrameData.TemptationMode` (≠0 → kakari / rushing).
    pub temptation: i8,
    /// `RaceSimulateHorseFrameData.BlockFrontHorseIndex` (≥0 → blocked in front).
    pub block_front: i8,
    /// Game `RunningStyle` enum (0 unknown, 1 Front, 2 Pace, 3 Late, 4 End).
    /// Constant per runner across the race; sourced from `HorseData`, not frames.
    pub strategy: u8,
}

/// Latest sampled race state for the overlay.
#[derive(Clone, Debug)]
pub struct LiveSnapshot {
    pub elapsed: f32,
    #[allow(dead_code)]
    pub frame_index: usize,
    #[allow(dead_code)]
    pub frame_count: usize,
    pub rows: Vec<RunnerRow>,
}

/// One player-owned uma's live state, surfaced to its dedicated widget.
#[derive(Clone, Debug)]
pub struct UmaRow {
    /// Character name (may be empty if unavailable).
    pub name: String,
    /// 1-based post number (horse index + 1), stable across the race.
    pub post: u8,
    /// Current stamina (HP).
    pub hp: u16,
    /// Starting stamina (frame 0), used as the HP-bar reference.
    pub initial_hp: u16,
    /// Raw speed (×100 m/s).
    pub speed: u16,
    /// Acceleration in m/s² (speed delta vs the previous frame).
    pub accel: f32,
    /// Kakari / rushing state active (`TemptationMode ≠ 0`).
    pub kakari: bool,
    /// Blocked by a horse in front (`BlockFrontHorseIndex ≥ 0`).
    pub blocked: bool,
    /// Recovery skills triggered so far this race (count of HP rising edges up to
    /// the current frame).
    pub recoveries: u16,
    /// Whether the race is currently being sampled (false = pre-start / idle).
    pub live: bool,
}

/// Read-only view assembled for the timer overlay.
#[derive(Clone, Debug, Default)]
pub struct UiState {
    pub live: Option<LiveSnapshot>,
}

#[derive(Debug, Default)]
struct State {
    /// `(race_info_addr, simdata_len)` of the last capture (dedupe).
    signature: Option<(usize, i32)>,
    frames: Vec<crate::sim::FrameData>,
    /// Character names by horse index (may be empty if unavailable).
    names: Vec<String>,
    /// Player-owned flag by horse index (`HorseData.IsUser`).
    mine: Vec<bool>,
    /// Running style (`HorseData.RunningStyle`) by horse index; 0 when unknown.
    styles: Vec<u8>,
    /// Starting stamina by horse index (frame 0), HP-bar reference.
    initial_hp: Vec<u16>,
    /// Frame indices where a recovery (HP rising edge) fired, per horse index.
    /// Precomputed once per race; used to count recoveries up to the live frame.
    recovery_frames: Vec<Vec<usize>>,
    live: Option<LiveSnapshot>,
}

fn cell() -> &'static Mutex<State> {
    STATE.get_or_init(|| Mutex::new(State::default()))
}

pub fn init() {
    let _ = cell();
}

/// Whether `(addr, len)` differs from the last capture (cheap pre-decode check).
#[must_use]
pub fn is_new_signature(race_info_addr: usize, simdata_len: i32) -> bool {
    cell()
        .lock()
        .expect("race-hud state lock poisoned")
        .signature
        .is_none_or(|s| s != (race_info_addr, simdata_len))
}

/// 0-based horse indices the player controls, in stable (post) order.
fn owned_indices(state: &State) -> Vec<usize> {
    state
        .mine
        .iter()
        .enumerate()
        .filter_map(|(i, &m)| m.then_some(i))
        .collect()
}

/// Store a freshly decoded race (frames + per-runner names + player-owned flags).
///
/// `mine[i] == true` marks a horse the player controls (`HorseData.IsUser`).
pub fn set_decoded(
    race_info_addr: usize,
    simdata_len: i32,
    decoded: Option<DecodedRace>,
    names: Vec<String>,
    mine: Vec<bool>,
    styles: Vec<u8>,
) {
    // Side-channel telemetry: publish the full decoded race one-shot (no-op when
    // disabled). Done before moving the data into the locked state.
    if let Some(d) = decoded.as_ref() {
        crate::telemetry::publish_full(d, &names, &mine);
    }

    let mut state = cell().lock().expect("race-hud state lock poisoned");
    state.signature = Some((race_info_addr, simdata_len));
    state.live = None;
    state.names = names;
    state.mine = mine;
    state.styles = styles;
    match decoded {
        Some(d) => {
            // Starting stamina per horse index from the first frame (HP-bar reference).
            state.initial_hp = d
                .frames
                .first()
                .map_or_else(Vec::new, |f| f.runners.iter().map(|r| r.hp).collect());
            state.recovery_frames = compute_recovery_frames(&d.frames);
            state.frames = d.frames;
        }
        None => {
            state.frames.clear();
            state.initial_hp.clear();
            state.recovery_frames.clear();
        }
    }
}

/// Per horse index, the frame indices at which a recovery fired.
///
/// HP drains monotonically while running, so any upward step is a recovery
/// skill. Contiguous rising frames are collapsed to one event (rising edge), so
/// a recovery spread over several frames still counts once.
fn compute_recovery_frames(frames: &[crate::sim::FrameData]) -> Vec<Vec<usize>> {
    let horse_num = frames.first().map_or(0, |f| f.runners.len());
    let mut out = vec![Vec::new(); horse_num];
    let mut rising = vec![false; horse_num];
    for f in 1..frames.len() {
        for i in 0..horse_num {
            let (Some(hp), Some(prev)) = (
                frames[f].runners.get(i).map(|r| r.hp),
                frames[f - 1].runners.get(i).map(|r| r.hp),
            ) else {
                continue;
            };
            if hp > prev {
                if !rising[i] {
                    out[i].push(f);
                    rising[i] = true;
                }
            } else {
                rising[i] = false;
            }
        }
    }
    out
}

/// State for the `slot`-th player-owned uma (0-based), or `None` if there is no
/// such owned uma in the current race.
#[must_use]
pub fn uma_row(slot: usize) -> Option<UmaRow> {
    let state = cell().lock().expect("race-hud state lock poisoned");
    let idx = *owned_indices(&state).get(slot)?;
    let post = (idx + 1) as u8;
    let name = state.names.get(idx).cloned().unwrap_or_default();
    let initial_hp = state.initial_hp.get(idx).copied().unwrap_or(0);

    let frame_index = state.live.as_ref().map(|snap| snap.frame_index);
    let recoveries = match frame_index {
        Some(fi) => state
            .recovery_frames
            .get(idx)
            .map_or(0, |edges| edges.iter().filter(|&&f| f <= fi).count() as u16),
        None => 0,
    };

    let sampled = state
        .live
        .as_ref()
        .and_then(|snap| snap.rows.iter().find(|r| r.post == post));

    Some(match sampled {
        Some(r) => UmaRow {
            name,
            post,
            hp: r.hp,
            initial_hp,
            speed: r.speed,
            accel: r.accel,
            kakari: r.temptation != 0,
            blocked: r.block_front >= 0,
            recoveries,
            live: true,
        },
        None => UmaRow {
            name,
            post,
            hp: initial_hp,
            initial_hp,
            speed: 0,
            accel: 0.0,
            kakari: false,
            blocked: false,
            recoveries: 0,
            live: false,
        },
    })
}

/// Sample the decoded frames at race time `elapsed`, refreshing the live snapshot.
pub fn sample_live(elapsed: f32) {
    let mut state = cell().lock().expect("race-hud state lock poisoned");
    if state.frames.is_empty() {
        return;
    }

    let idx = state.frames.partition_point(|f| f.time <= elapsed).saturating_sub(1);

    // Previous frame + dt for the acceleration estimate (0 at the first frame).
    let prev = state.frames.get(idx.wrapping_sub(1)).filter(|_| idx > 0);
    let dt = prev.map_or(0.0, |p| state.frames[idx].time - p.time);

    let mut rows: Vec<RunnerRow> = state.frames[idx]
        .runners
        .iter()
        .enumerate()
        .map(|(i, r)| {
            // Δspeed/Δt in m/s² (raw speed is ×100 m/s).
            let accel = match prev {
                Some(p) if dt > 0.0 => {
                    let prev_speed = p.runners.get(i).map_or(r.speed, |pr| pr.speed);
                    (f32::from(r.speed) - f32::from(prev_speed)) / 100.0 / dt
                }
                _ => 0.0,
            };
            RunnerRow {
                rank: 0,
                post: (i + 1) as u8,
                name: state.names.get(i).cloned().unwrap_or_default(),
                distance: r.distance,
                speed: r.speed,
                hp: r.hp,
                accel,
                temptation: r.temptation,
                block_front: r.block_front,
                strategy: state.styles.get(i).copied().unwrap_or(0),
            }
        })
        .collect();

    rows.sort_by(|a, b| b.distance.total_cmp(&a.distance));
    for (i, row) in rows.iter_mut().enumerate() {
        row.rank = (i + 1) as u8;
    }

    // Side-channel telemetry: publish the live frame (no-op when disabled).
    crate::telemetry::publish_live(elapsed, &rows);

    let frame_count = state.frames.len();
    state.live = Some(LiveSnapshot {
        elapsed,
        frame_index: idx,
        frame_count,
        rows,
    });
}

#[must_use]
pub fn ui_state() -> UiState {
    let state = cell().lock().expect("race-hud state lock poisoned");
    UiState {
        live: state.live.clone(),
    }
}

/// Reset everything (manual Reset button or shutdown).
pub fn clear_all() {
    let mut state = cell().lock().expect("race-hud state lock poisoned");
    *state = State::default();
}
