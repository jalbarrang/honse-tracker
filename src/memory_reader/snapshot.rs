//! Career state snapshot: core stats, turn info, and training facility levels.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

use crate::compat::Sdk;

use super::chain::{ResolvedChain, CHAIN};
use super::command_info::{read_command_infos, CommandInfo};
use super::il2cpp::{
    call_bool, call_i32, call_i32_with_i32, call_obj, read_obscured_int_array, read_obscured_int_field,
};
use super::scenario::{read_scenario_state, ScenarioState};
use crate::evaluation::Aptitudes;

/// Snapshot of career state read from game memory.
#[derive(Debug, Clone, Default)]
pub struct CareerSnapshot {
    pub is_playing: bool,
    pub current_turn: i32,
    pub month: i32,

    // Core stats (decrypted from ObscuredInt by the C# getters)
    pub speed: i32,
    pub stamina: i32,
    pub power: i32,
    pub guts: i32,
    pub wiz: i32,
    pub total_stats: i32,

    pub hp: i32,
    pub max_hp: i32,
    pub motivation: i32, // RaceDefine.Motivation enum (1-5)
    #[allow(dead_code)] // read from memory; no longer shown in the redesigned UI
    pub fan_count: i32,
    /// Trained outfit/card id (matches gametora `card_id`); `0` if unknown.
    /// Used to detect the trainee's built-in (unique/innate/awakening) recoveries.
    pub card_id: i32,
    // NOTE: get_SkillPoint returns ObscuredInt (struct), not i32.
    // Needs special decryption handling — skipped for now.
    #[allow(dead_code)]
    pub skill_point: i32,

    #[allow(dead_code)] // read from memory; no longer shown in the redesigned UI
    pub total_races: i32,
    #[allow(dead_code)] // read from memory; no longer shown in the redesigned UI
    pub win_count: i32,

    /// Training facility levels [Speed, Stamina, Power, Guts, Wisdom].
    /// Read via `GetTrainingLevel(commandId)`. 0 means not available.
    pub training_levels: [i32; 5],

    /// Per-stat caps [Speed, Stamina, Power, Guts, Wisdom] (live MaxSpeed/etc.,
    /// including scenario raises). 0 means unknown. Decrypted from ObscuredInt.
    pub stat_caps: [i32; 5],

    /// Race aptitude grades (ProperGrade ints) — for the evaluation estimate.
    pub aptitudes: Aptitudes,
    /// Card rarity / star (1–5); drives the unique-skill bonus multiplier.
    pub star: i32,

    /// Self-computed overall evaluation estimate (評価点). Filled by overlay_cache.
    /// Mapped to a rank-badge label via `crate::rank_table::rank_label`.
    pub evaluation_value: Option<i32>,

    /// Per-facility training failure % [Speed, Stamina, Power, Guts, Wisdom].
    /// `-1` means unknown (no live command info this turn).
    pub failure_rates: [i32; 5],

    /// Per-facility total stat gain [Speed, Stamina, Power, Guts, Wisdom].
    /// Sum of the 5 main-stat deltas for that facility. `0` means unknown/none.
    pub stat_gains: [i32; 5],

    /// Per-facility, per-stat gain: outer = facility slot, inner = [Speed, Stamina,
    /// Power, Guts, Wisdom] delta. Drives the projected-評価点 recommendation.
    pub per_stat_gains: [[i32; 5]; 5],

    /// Per-facility near-rainbow bond pressure `0..=1` from the supports present on
    /// each facility this turn (`TrainingHorseList` bond values →
    /// `planner::near_rainbow_pressure`). Feeds the multi-turn bond lookahead.
    pub per_facility_bond_pressure: [f32; 5],

    /// `Evaluation.target_id` → (facility slot 0..4, per-character bond pressure).
    pub partner_placements: HashMap<i32, (usize, f32)>,

    /// Speed-slot training command id of the active scenario (e.g. 101 URA, 601
    /// Unity Cup, 1101 Trackblazer). Identifies the scenario for the rest-vs-race
    /// suggestion. `0` means unknown.
    pub scenario_command_base: i32,

    /// Active scenario id (`get_ScenarioId`). e.g. 4 = Trackblazer. `0` = unknown.
    pub scenario_id: i32,

    /// Live scenario-specific state (e.g. Trackblazer shop). `None` for the base
    /// URA Finale scenario or when not yet read.
    pub scenario_state: Option<ScenarioState>,

    /// Active career conditions/状態 as raw chara-effect ids (decrypted from the
    /// `CharaEffectIdArray` ObscuredInt[]). Mapped to display names via
    /// `crate::chara_effects`. Empty when none are active or unread.
    pub chara_effect_ids: Vec<i32>,
}

/// Read a snapshot of the current career state from game memory.
/// Returns `None` if the chain isn't resolved or the singleton is unavailable.
pub fn read_snapshot() -> Option<CareerSnapshot> {
    // Catch panics from bad IL2CPP pointers so they don't take down the render thread.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(read_snapshot_inner)) {
        Ok(result) => result,
        Err(_) => {
            hlog_error!("read_snapshot PANICKED — IL2CPP call likely hit a bad pointer");
            None
        }
    }
}

fn read_snapshot_inner() -> Option<CareerSnapshot> {
    let chain = CHAIN.get()?;
    let sdk = Sdk::get();

    hlog_trace!("snapshot: step 1 — get singleton");
    let singleton = sdk.get_singleton(chain.wdm_klass.cast())?.cast::<c_void>();
    hlog_trace!("snapshot: step 2 — get_SingleMode (singleton={:?})", singleton);
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let wsmd = unsafe { call_obj(singleton, chain.m_get_single_mode) };
    if wsmd.is_null() {
        return Some(CareerSnapshot::default());
    }

    // Step 3: Check if a career is active
    hlog_trace!("snapshot: step 3 — get_IsPlaying (wsmd={:?})", wsmd);
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let is_playing = unsafe { call_bool(wsmd, chain.m_get_is_playing) };

    if !is_playing {
        return Some(CareerSnapshot {
            is_playing: false,
            ..Default::default()
        });
    }

    // Step 4: Read turn/career info from WorkSingleModeData
    // NOTE: Only simple `get_` property accessors are safe here.
    // `GetFinalTurn`/`GetRemainTurnNum` do master-data lookups and crash
    // when called from the render thread.
    hlog_trace!("snapshot: step 4 — turn/career info");
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let month = unsafe { call_i32(wsmd, chain.m_get_month) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let current_turn = unsafe { call_i32(wsmd, chain.m_get_current_turn) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let total_races = unsafe { call_i32(wsmd, chain.m_get_total_races) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let win_count = unsafe { call_i32(wsmd, chain.m_get_win_count) };

    // Step 5: WorkSingleModeData → WorkSingleModeCharaData
    hlog_trace!("snapshot: step 5 — get_Character");
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let chara = unsafe { call_obj(wsmd, chain.m_get_character) };
    if chara.is_null() {
        hlog_warn!("read_snapshot: get_Character returned null");
        return Some(CareerSnapshot {
            is_playing: true,
            current_turn,
            month,
            total_races,
            win_count,
            ..Default::default()
        });
    }

    // Step 6: Read all stats from WorkSingleModeCharaData
    hlog_trace!("snapshot: step 6 — stats (chara={:?})", chara);
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let speed = unsafe { call_i32(chara, chain.m_get_speed) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let stamina = unsafe { call_i32(chara, chain.m_get_stamina) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let power = unsafe { call_i32(chara, chain.m_get_power) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let guts = unsafe { call_i32(chara, chain.m_get_guts) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let wiz = unsafe { call_i32(chara, chain.m_get_wiz) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let total_stats = unsafe { call_i32(chara, chain.m_get_all_total) };
    hlog_trace!("snapshot: step 6b — hp/motivation/fans");
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let hp = unsafe { call_i32(chara, chain.m_get_hp) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let max_hp = unsafe { call_i32(chara, chain.m_get_max_hp) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let motivation = unsafe { call_i32(chara, chain.m_get_motivation) };
    // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
    let fan_count = unsafe { call_i32(chara, chain.m_get_fan_count) };
    // Trained outfit/card id (matches gametora `card_id`; drives built-in skill
    // lookup). SAFETY: getter on a non-null IL2CPP chara object.
    let card_id = unsafe { call_i32(chara, chain.m_get_card_id) };

    // Step 7: Read training levels per facility
    hlog_trace!("snapshot: step 7 — training levels");
    let training_levels = read_training_levels(chara, chain);

    // Step 8: Aptitudes + star (for the evaluation estimate)
    hlog_trace!("snapshot: step 8 — aptitudes/star");
    let aptitudes = read_aptitudes(chara, chain);
    let star = read_star(chara, chain);

    // Step 9: Per-stat caps (live MaxSpeed/etc., ObscuredInt)
    hlog_trace!("snapshot: step 9 — stat caps");
    let stat_caps = read_stat_caps(chara);

    // Step 10: Per-facility failure rate + stat-gain preview (live command info)
    hlog_trace!("snapshot: step 10 — command info");
    let command_infos = read_command_infos(wsmd);
    let partner_placements = partner_placements_from(&command_infos);
    {
        // Turn-change diagnostic: dump raw per-facility partner ids so we can tell
        // a stale game-side HomeInfo arrangement apart from a mapping bug in
        // `partner_placements_from`. Fires once per turn (logs on turn change).
        static LAST_PLACEMENT_TURN: AtomicI32 = AtomicI32::new(-1);
        if LAST_PLACEMENT_TURN.swap(current_turn, Ordering::Relaxed) != current_turn {
            for info in &command_infos {
                let facility = facility_index_of(info.command_id);
                let ids: Vec<i32> = info.partners.iter().map(|&(id, _, _)| id).collect();
                hlog_info!(
                    "placements turn={current_turn} command_id={} facility={facility:?} partners={ids:?}",
                    info.command_id
                );
            }
        }
    }
    let (failure_rates, stat_gains, per_stat_gains, per_facility_bond_pressure, scenario_command_base) =
        align_command_infos(&command_infos);

    // Step 11: Active scenario id (logged) + command base (rest-vs-race suggestion)
    // SAFETY: Reading a getter on a non-null IL2CPP chara object.
    let scenario_id = unsafe { call_i32(chara, chain.m_get_scenario_id) };

    // Step 11b: Active career conditions/状態 (CharaEffectIdArray).
    // SAFETY: getter on a valid chara object returns an ObscuredInt[] (or null).
    let effect_array = unsafe { call_obj(chara, chain.m_get_chara_effect_id_array) };
    // SAFETY: value-type ObscuredInt[] with the standard IL2CPP array layout.
    let chara_effect_ids = unsafe { read_obscured_int_array(effect_array) };
    {
        // One-shot diagnostic: capture the live effect ids so we can map them
        // to display names (see crate::chara_effects).
        static EFFECTS_LOGGED: AtomicBool = AtomicBool::new(false);
        if !chara_effect_ids.is_empty() && !EFFECTS_LOGGED.swap(true, Ordering::Relaxed) {
            hlog_info!("Active chara effect ids (状態): {:?}", chara_effect_ids);
            let unknown: Vec<i32> = chara_effect_ids
                .iter()
                .copied()
                .filter(|&id| !crate::chara_effects::is_known(id))
                .collect();
            if !unknown.is_empty() {
                hlog_warn!("Unmapped chara effect ids (add to chara_effects table): {:?}", unknown);
            }
        }
    }
    {
        // One-shot diagnostic so the live values land in hachimi.log for verification.
        static CMD_LOGGED: AtomicBool = AtomicBool::new(false);
        if !CMD_LOGGED.swap(true, Ordering::Relaxed) {
            hlog_info!(
                "Command info: scenario_id={} failure_rates={:?} stat_gains={:?} per_stat_gains={:?} bond_pressure={:?}",
                scenario_id,
                failure_rates,
                stat_gains,
                per_stat_gains,
                per_facility_bond_pressure
            );
        }
    }

    hlog_trace!("snapshot: complete (turn={}, total={})", current_turn, total_stats);
    Some(CareerSnapshot {
        is_playing: true,
        current_turn,
        month,
        speed,
        stamina,
        power,
        guts,
        wiz,
        total_stats,
        hp,
        max_hp,
        motivation,
        fan_count,
        card_id,
        skill_point: 0, // ObscuredInt — needs decryption, not yet implemented
        total_races,
        win_count,
        training_levels,
        stat_caps,
        aptitudes,
        star,
        // Filled by overlay_cache (self-computed via crate::evaluation).
        evaluation_value: None,
        failure_rates,
        stat_gains,
        per_stat_gains,
        per_facility_bond_pressure,
        partner_placements,
        scenario_command_base,
        scenario_id,
        // SAFETY: `chara` is a valid non-null IL2CPP object from the resolved chain.
        scenario_state: read_scenario_state(chara),
        chara_effect_ids,
    })
}

/// Build per-partner facility placement from live command infos.
fn partner_placements_from(infos: &[CommandInfo]) -> HashMap<i32, (usize, f32)> {
    let mut map = HashMap::new();
    for info in infos {
        let Some(facility) = facility_index_of(info.command_id) else {
            continue;
        };
        for &(target_id, pressure, _is_guest) in &info.partners {
            map.insert(target_id, (facility, pressure));
        }
    }
    map
}

/// Map live command infos onto facility slots [Speed, Stamina, Power, Guts, Wisdom]
/// by matching each `command_id` against the known command sets. Failure rates
/// default to `-1` (unknown); stat gains default to `0`.
/// Returns `(failure_rates, stat_gains, per_stat_gains, bond_pressure,
/// scenario_command_base)`. `scenario_command_base` is the Speed-slot command id
/// (set base), identifying the active scenario; `0` if no Speed facility was found.
#[allow(clippy::type_complexity)]
fn align_command_infos(infos: &[CommandInfo]) -> ([i32; 5], [i32; 5], [[i32; 5]; 5], [f32; 5], i32) {
    let mut failure = [-1i32; 5];
    let mut gain = [0i32; 5];
    let mut per_stat = [[0i32; 5]; 5];
    let mut bond = [0f32; 5];
    let mut base = 0;
    for info in infos {
        if let Some(idx) = facility_index_of(info.command_id) {
            failure[idx] = info.failure_rate;
            gain[idx] = info.stat_gain;
            per_stat[idx] = info.per_stat;
            bond[idx] = info.bond_pressure;
            if idx == 0 {
                base = info.command_id; // Speed-slot command id = scenario set base
            }
        }
    }
    (failure, gain, per_stat, bond, base)
}

/// Facility slot index (0..5) for a training `command_id`, searching all known sets.
fn facility_index_of(command_id: i32) -> Option<usize> {
    COMMAND_ID_SETS
        .iter()
        .find_map(|set| set.iter().position(|&c| c == command_id))
}

/// Read the 5 per-stat caps (live MaxSpeed/etc.) from ObscuredInt backing fields.
/// Returns [0; 5] for any cap that can't be resolved.
fn read_stat_caps(chara: *mut c_void) -> [i32; 5] {
    let sdk = Sdk::get();
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let klass = unsafe { *(chara as *const *mut c_void) };
    let names = [
        "<MaxSpeed>k__BackingField",
        "<MaxStamina>k__BackingField",
        "<MaxPower>k__BackingField",
        "<MaxGuts>k__BackingField",
        "<MaxWiz>k__BackingField",
    ];
    let mut caps = [0i32; 5];
    for (i, name) in names.iter().enumerate() {
        if let Some(field) = sdk.get_field_from_name(klass.cast(), name) {
            // SAFETY: ObscuredInt field on a valid IL2CPP chara object.
            caps[i] = unsafe { read_obscured_int_field(chara, field.cast()) };
        }
    }
    caps
}

/// Read all 10 aptitude grades from the chara object.
fn read_aptitudes(chara: *mut c_void, chain: &ResolvedChain) -> Aptitudes {
    // SAFETY: Reading getters on a non-null IL2CPP chara object.
    unsafe {
        Aptitudes {
            dist_short: call_i32(chara, chain.m_apt_dist_short),
            dist_mile: call_i32(chara, chain.m_apt_dist_mile),
            dist_middle: call_i32(chara, chain.m_apt_dist_middle),
            dist_long: call_i32(chara, chain.m_apt_dist_long),
            style_nige: call_i32(chara, chain.m_apt_style_nige),
            style_senko: call_i32(chara, chain.m_apt_style_senko),
            style_sashi: call_i32(chara, chain.m_apt_style_sashi),
            style_oikomi: call_i32(chara, chain.m_apt_style_oikomi),
            ground_turf: call_i32(chara, chain.m_apt_ground_turf),
            ground_dirt: call_i32(chara, chain.m_apt_ground_dirt),
        }
    }
}

/// Read the trainee star/rarity via `get_CardRarityData().Rarity`. 0 on failure.
fn read_star(chara: *mut c_void, chain: &ResolvedChain) -> i32 {
    // SAFETY: get_CardRarityData returns a MasterCardRarityData.CardRarityData object.
    let rarity_obj = unsafe { call_obj(chara, chain.m_get_card_rarity_data) };
    if rarity_obj.is_null() {
        return 0;
    }
    let sdk = Sdk::get();
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let klass = unsafe { *(rarity_obj as *const *mut c_void) };
    let Some(field) = sdk.get_field_from_name(klass.cast(), "Rarity") else {
        return 0;
    };
    let mut rarity: i32 = 0;
    // SAFETY: Reading an Int32 field from a valid IL2CPP object.
    unsafe {
        sdk.get_field_value(rarity_obj.cast(), field, &mut rarity as *mut _ as *mut c_void);
    }
    rarity
}

// ---------------------------------------------------------------------------
// Training level detection
// ---------------------------------------------------------------------------

/// Known command ID sets per scenario: [Speed, Stamina, Power, Guts, Wisdom].
const COMMAND_ID_SETS: &[[i32; 5]] = &[
    [101, 105, 102, 103, 106],      // URA Finale / base
    [601, 602, 603, 604, 605],      // Unity Cup (JP: Aoharu Hai)
    [1101, 1102, 1103, 1104, 1105], // Trackblazer (a.k.a. Make a New Track) — to confirm
    [2101, 2102, 2103, 2104, 2105], // UAF type A (JP-only so far)
    [2201, 2202, 2203, 2204, 2205], // UAF type B
    [2301, 2302, 2303, 2304, 2305], // UAF type C
    [901, 902, 903, 904, 906],      // Onsen (partially confirmed)
];

/// Read training levels for all 5 facilities.
/// Auto-detects the correct command ID set by probing known sets.
/// Returns [0; 5] if anything goes wrong.
fn read_training_levels(chara: *mut c_void, chain: &ResolvedChain) -> [i32; 5] {
    let sdk = Sdk::get();
    hlog_trace!("training_levels: checking _trainingLevelDic field");
    // SAFETY: IL2CPP object header — klass pointer at offset 0.
    let chara_klass = unsafe { *(chara as *const *mut c_void) };
    let Some(field) = sdk.get_field_from_name(chara_klass.cast(), "_trainingLevelDic") else {
        hlog_trace!("training_levels: _trainingLevelDic field not found");
        return [0; 5];
    };

    let mut dict_ptr: *mut c_void = std::ptr::null_mut();
    // SAFETY: IL2CPP object and field from resolved metadata.
    unsafe {
        sdk.get_field_value(chara.cast(), field, &mut dict_ptr as *mut _ as *mut c_void);
    }
    if dict_ptr.is_null() {
        hlog_trace!("training_levels: dictionary is null, skipping");
        return [0; 5];
    }

    hlog_trace!("training_levels: probing command ID sets (dict={:?})", dict_ptr);
    for set in COMMAND_ID_SETS {
        let mut levels = [0i32; 5];
        let mut any_positive = false;

        for (i, &cmd_id) in set.iter().enumerate() {
            // SAFETY: Reading field or calling method on non-null IL2CPP object pointer.
            let level = unsafe { call_i32_with_i32(chara, chain.m_get_training_level, cmd_id) };
            levels[i] = level;
            if level > 0 {
                any_positive = true;
            }
        }

        if any_positive {
            static LEVELS_LOGGED: AtomicBool = AtomicBool::new(false);
            if !LEVELS_LOGGED.swap(true, Ordering::Relaxed) {
                hlog_info!("Training levels matched set {:?} → {:?}", set, levels);
            }
            return levels;
        }
    }

    hlog_trace!("training_levels: no matching command ID set found");
    [0; 5]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facility_index_maps_known_command_ids() {
        // URA / base set.
        assert_eq!(facility_index_of(101), Some(0)); // Speed
        assert_eq!(facility_index_of(105), Some(1)); // Stamina
        assert_eq!(facility_index_of(106), Some(4)); // Wisdom
                                                     // Aoharu set.
        assert_eq!(facility_index_of(603), Some(2)); // Power
                                                     // Unknown id.
        assert_eq!(facility_index_of(9999), None);
    }

    #[test]
    fn align_command_infos_places_by_facility() {
        let infos = [
            CommandInfo {
                command_id: 101, // Speed
                failure_rate: 3,
                stat_gain: 12,
                per_stat: [10, 0, 2, 0, 0],
                bond_pressure: 0.6,
                partners: vec![],
            },
            CommandInfo {
                command_id: 103, // Guts
                failure_rate: 28,
                stat_gain: 9,
                per_stat: [0, 0, 3, 6, 0],
                bond_pressure: 0.0,
                partners: vec![],
            },
        ];
        let (failure, gain, per_stat, bond, base) = align_command_infos(&infos);
        assert_eq!(failure, [3, -1, -1, 28, -1]);
        assert_eq!(gain, [12, 0, 0, 9, 0]);
        assert_eq!(per_stat[0], [10, 0, 2, 0, 0]);
        assert_eq!(per_stat[3], [0, 0, 3, 6, 0]);
        assert_eq!(per_stat[1], [0; 5]);
        assert_eq!(bond[0], 0.6); // Speed facility's near-rainbow pressure
        assert_eq!(bond[3], 0.0);
        assert_eq!(base, 101); // Speed-slot command id
    }

    #[test]
    fn align_command_infos_ignores_unknown_ids() {
        let infos = [CommandInfo {
            command_id: 9999,
            failure_rate: 50,
            stat_gain: 20,
            per_stat: [4, 4, 4, 4, 4],
            bond_pressure: 0.9,
            partners: vec![],
        }];
        let (failure, gain, per_stat, bond, base) = align_command_infos(&infos);
        assert_eq!(failure, [-1; 5]);
        assert_eq!(gain, [0; 5]);
        assert_eq!(per_stat, [[0; 5]; 5]);
        assert_eq!(bond, [0.0; 5]); // unknown command → no bond mapping
        assert_eq!(base, 0); // no known facility → unknown base
    }
}
