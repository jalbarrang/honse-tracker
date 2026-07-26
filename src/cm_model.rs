//! Closed-form Champions Meeting (CM) race-utility model.
//!
//! Pure functions that answer: *given a target CM course + strategy + the
//! trainee's current stats, what is the race value of one more point in stat X?*
//! This is the foundation the CM-objective scorer builds on — it deliberately
//! replaces the 評価点 (rank) curve (`crate::evaluation::stat_score`), which
//! rewards raw stat magnitude for ranking rather than race-winning power.
//!
//! The formulas are ported from the Torena/uma-sim race engine
//! (`../uma-sim/packages/uma-sim-primitives`, itself a port of umasim) and
//! grounded in the community meta (gametora race-mechanics, uma.guide). They are
//! closed-form and self-contained: no IL2CPP, no cross-repo dependency, so the
//! shipped DLL stays standalone. Parity with the reference engine is asserted in
//! the tests where exact anchors exist.
//!
//! ## Marginal-value unit
//!
//! [`stat_marginal_value`] returns **uutil** ("utility units"): approximately
//! `1000 × (m/s contribution to effective race speed per +1 stat point)`. Speed
//! is the principled backbone (its last-spurt derivative is exact); the other
//! stats are scaled into the same unit through documented heuristic coefficients
//! (anchored, then tuned later). Only *internal consistency* matters — the scorer
//! sums these across a facility's per-stat gains, so the relative magnitudes are
//! what count, not the absolute scale.

// ---------------------------------------------------------------------------
// Value objects
// ---------------------------------------------------------------------------

use serde::{Deserialize, Serialize};

/// Race surface. Discriminants match the master.mdb `ground` column
/// (1 = Turf, 2 = Dirt); the `cm-course-data` tool fills these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Surface {
    Turf,
    Dirt,
}

/// Track (baba) condition. Discriminants match the game / uma-sim convention
/// (Firm = 1 … Heavy = 4) so the ground-modifier tables index directly. `Firm`
/// is the neutral baseline (zero penalty) and the default for planning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GroundCondition {
    /// 良 — firm/baseline: no penalty.
    #[default]
    Firm,
    /// 稍重 — good.
    Good,
    /// 重 — soft.
    Soft,
    /// 不良 — heavy: largest penalty.
    Heavy,
}

impl GroundCondition {
    /// All conditions, in game order, for UI pickers.
    pub const ALL: [GroundCondition; 4] = [
        GroundCondition::Firm,
        GroundCondition::Good,
        GroundCondition::Soft,
        GroundCondition::Heavy,
    ];

    /// Short English label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            GroundCondition::Firm => "Firm",
            GroundCondition::Good => "Good",
            GroundCondition::Soft => "Soft",
            GroundCondition::Heavy => "Heavy",
        }
    }

    /// 1-based index (Firm = 1 … Heavy = 4) into the ground-modifier tables.
    fn index(self) -> usize {
        match self {
            GroundCondition::Firm => 1,
            GroundCondition::Good => 2,
            GroundCondition::Soft => 3,
            GroundCondition::Heavy => 4,
        }
    }
}

/// Ground (surface × condition) **flat speed penalty** applied to the effective
/// in-race stat (port of uma-sim `GROUND_SPEED_MODIFIER`). Outer index is
/// `surface as usize` (0 = Turf, 1 = Dirt); inner index is the condition's
/// 1-based index (column 0 unused). Only Heavy penalizes speed (−50, both
/// surfaces).
const GROUND_SPEED_MODIFIER: [[i32; 5]; 2] = [[0, 0, 0, 0, -50], [0, 0, 0, 0, -50]];

/// Ground (surface × condition) **flat power penalty** (port of uma-sim
/// `GROUND_POWER_MODIFIER`). Same indexing as [`GROUND_SPEED_MODIFIER`]. Dirt is
/// power-hungry even on firm ground.
const GROUND_POWER_MODIFIER: [[i32; 5]; 2] = [[0, 0, 0, -50, -50], [0, -100, -50, -100, -100]];

/// Flat speed penalty (≤ 0) for a surface and ground condition.
pub fn ground_speed_modifier(surface: Surface, condition: GroundCondition) -> i32 {
    GROUND_SPEED_MODIFIER[surface as usize][condition.index()]
}

/// Flat power penalty (≤ 0) for a surface and ground condition.
pub fn ground_power_modifier(surface: Surface, condition: GroundCondition) -> i32 {
    GROUND_POWER_MODIFIER[surface as usize][condition.index()]
}

/// Running style. Discriminant follows the game / uma-sim convention
/// (FrontRunner = 1 … Runaway = 5) so HP-coefficient lookups index directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Strategy {
    /// 逃げ (nige).
    FrontRunner,
    /// 先行 (senko).
    PaceChaser,
    /// 差し (sashi).
    LateSurger,
    /// 追込 (oikomi).
    EndCloser,
    /// 大逃げ (oonige).
    Runaway,
}

impl Strategy {
    /// All running styles, in game order, for UI pickers.
    pub const ALL: [Strategy; 5] = [
        Strategy::FrontRunner,
        Strategy::PaceChaser,
        Strategy::LateSurger,
        Strategy::EndCloser,
        Strategy::Runaway,
    ];

    /// Short English label for the UI (no JP romanization).
    pub fn label(self) -> &'static str {
        match self {
            Strategy::FrontRunner => "Front",
            Strategy::PaceChaser => "Pace",
            Strategy::LateSurger => "Late",
            Strategy::EndCloser => "End",
            Strategy::Runaway => "Runaway",
        }
    }

    /// 1-based discriminant matching uma-sim's `Strategy as usize`.
    pub fn discriminant(self) -> usize {
        match self {
            Strategy::FrontRunner => 1,
            Strategy::PaceChaser => 2,
            Strategy::LateSurger => 3,
            Strategy::EndCloser => 4,
            Strategy::Runaway => 5,
        }
    }

    /// Stamina→HP conversion coefficient (`HP_STRATEGY_COEFFICIENT`).
    /// Late Surger / End Closer convert best; Pace Chaser worst.
    pub fn hp_coef(self) -> f64 {
        // [_, nige .95, senko .89, sashi 1.0, oikomi .995, oonige .86]
        const HP_STRATEGY_COEFFICIENT: [f64; 6] = [0.0, 0.95, 0.89, 1.0, 0.995, 0.86];
        HP_STRATEGY_COEFFICIENT[self.discriminant()]
    }

    /// Speed strategy×phase coefficient (uma-sim `speed::STRATEGY_PHASE_COEFFICIENT`).
    /// `phase_col`: 0 = early, 1 = mid, 2 = late/last-spurt.
    fn speed_phase_coef(self, phase_col: usize) -> f64 {
        const SPEED_STRATEGY_PHASE: [[f64; 3]; 6] = [
            [0.0, 0.0, 0.0],
            [1.0, 0.98, 0.962],
            [0.978, 0.991, 0.975],
            [0.938, 0.998, 0.994],
            [0.931, 1.0, 1.0],
            [1.063, 0.962, 0.95],
        ];
        SPEED_STRATEGY_PHASE[self.discriminant()][phase_col.min(2)]
    }
}

/// The five core stats, in the plugin's canonical order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatKind {
    Speed,
    Stamina,
    Power,
    Guts,
    Wit,
}

impl StatKind {
    /// Index into a `[_; 5]` stat array ([Speed, Stamina, Power, Guts, Wit]).
    pub fn index(self) -> usize {
        match self {
            StatKind::Speed => 0,
            StatKind::Stamina => 1,
            StatKind::Power => 2,
            StatKind::Guts => 3,
            StatKind::Wit => 4,
        }
    }
}

/// Per-course parameters the CM math needs. Owned here (the canonical shape);
/// the `cm-course-data` maintainer tool fills these from master.mdb. Fields the
/// model does not read (turn, finish times) are carried for the data layer / UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseParams {
    /// Course distance in meters.
    pub distance: f64,
    /// Turf or dirt.
    pub surface: Surface,
    /// Track orientation (master.mdb `turn`); unused by the math, kept for UI.
    pub turn: i32,
    /// Course "set status" stat thresholds: crossing each by multiples of 300
    /// (up to 900) grants a +5% speed bonus per step. Serialized as `thresholds`
    /// to match the `course-data` tool's asset.
    #[serde(rename = "thresholds", default)]
    pub set_status_thresholds: Vec<StatKind>,
    /// Reference finish-time window (master.mdb), carried for the data layer.
    #[serde(default)]
    pub finish_time_min: f64,
    /// Reference finish-time window (master.mdb), carried for the data layer.
    #[serde(default)]
    pub finish_time_max: f64,
}

/// Race aptitude grades relevant to the CM math, using the game's `ProperGrade`
/// convention: `Null = 0`, `G = 1` … `S = 8`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Aptitudes {
    /// Distance aptitude for the target course (affects top spurt speed + accel).
    pub distance_grade: i32,
    /// Surface aptitude for the target course (affects acceleration).
    pub surface_grade: i32,
}

// ---------------------------------------------------------------------------
// Proficiency tables (uma-sim `course::coefficients`), indexed S=0 … G=7
// ---------------------------------------------------------------------------

/// Map a `ProperGrade` (Null=0, G=1 … S=8) to a proficiency-table index
/// (S=0 … G=7). Unknown/Null falls back to A (neutral, index 1).
fn apt_index(grade: i32) -> usize {
    if grade <= 0 {
        1 // treat "no data" as A (1.0×) rather than punishing it
    } else {
        (8 - grade).clamp(0, 7) as usize
    }
}

/// Distance proficiency multiplier for **target speed** (speed table).
fn speed_distance_prof(grade: i32) -> f64 {
    const T: [f64; 8] = [1.05, 1.0, 0.9, 0.8, 0.6, 0.4, 0.2, 0.1];
    T[apt_index(grade)]
}

/// Distance proficiency multiplier for **acceleration** (accel table).
fn accel_distance_prof(grade: i32) -> f64 {
    const T: [f64; 8] = [1.0, 1.0, 1.0, 1.0, 1.0, 0.6, 0.5, 0.4];
    T[apt_index(grade)]
}

/// Surface (ground-type) proficiency multiplier for **acceleration**.
fn ground_surface_prof(grade: i32) -> f64 {
    const T: [f64; 8] = [1.05, 1.0, 0.9, 0.8, 0.7, 0.5, 0.3, 0.1];
    T[apt_index(grade)]
}

// ---------------------------------------------------------------------------
// Core scalar formulas (ports)
// ---------------------------------------------------------------------------

/// Course base speed: `20 − (distance − 2000) / 1000`.
pub fn base_speed(distance: f64) -> f64 {
    20.0 - (distance - 2000.0) / 1000.0
}

/// Maximum HP at race start: `0.8 × strategy_coef × stamina + distance`.
pub fn max_hp(stamina: f64, strategy: Strategy, distance: f64) -> f64 {
    0.8 * strategy.hp_coef() * stamina + distance
}

/// Spurt-phase HP-burn modifier from Guts: `1 + 200 / sqrt(600 × guts)`.
/// Higher Guts ⇒ lower modifier ⇒ less HP burned during the last spurt.
pub fn guts_modifier(guts: f64) -> f64 {
    1.0 + 200.0 / (600.0 * guts.max(1.0)).sqrt()
}

/// Firm-ground HP consumption coefficient for a surface (planning baseline:
/// CM weather varies, but firm is the reference; wetter ground only raises burn).
fn ground_consumption_coef(_surface: Surface) -> f64 {
    1.0
}

/// HP consumed per second at `velocity` (port of `calculate_hp_per_second` with
/// `status_modifier = 1`). The Guts modifier applies only in the spurt phase.
fn hp_per_second(velocity: f64, base_speed: f64, ground_coef: f64, guts_mod: f64, in_spurt: bool) -> f64 {
    let guts = if in_spurt { guts_mod } else { 1.0 };
    (20.0 * (velocity - base_speed + 12.0).powi(2) / 144.0) * ground_coef * guts
}

/// Mid-race target speed (no Speed-stat term in phases 0/1): `base × strat_coef`.
fn mid_target_speed(strategy: Strategy, base_speed: f64) -> f64 {
    base_speed * strategy.speed_phase_coef(1)
}

/// Maximum last-spurt speed (port of `calculate_last_spurt_speed`). Depends on
/// Speed (twice: phase-2 target + spurt term), Guts, distance aptitude, strategy.
pub fn last_spurt_speed(speed: f64, guts: f64, strategy: Strategy, distance_grade: i32, base_speed: f64) -> f64 {
    let prof = speed_distance_prof(distance_grade);
    let phase2_target = base_speed * strategy.speed_phase_coef(2) + (500.0 * speed).sqrt() * prof * 0.002;
    let mut result = (phase2_target + 0.01 * base_speed) * 1.05 + (500.0 * speed).sqrt() * prof * 0.002;
    result += (450.0 * guts.max(1.0)).powf(0.597) * 0.0001;
    result
}

// ---------------------------------------------------------------------------
// Soft-cap / overcap and course set-status thresholds
// ---------------------------------------------------------------------------

/// Stat value at which the in-race soft cap kicks in (points above count half).
pub const SOFT_CAP: i32 = 1200;

/// Power "enough" knee centre for a course: longer / dirt courses want more Power
/// before the marginal value tapers. Exposed so the UI can show a Power target.
pub fn power_knee(course: &CourseParams) -> f64 {
    POWER_KNEE_BASE
        + if course.surface == Surface::Dirt { 100.0 } else { 0.0 }
        + ((course.distance - 2000.0) / 400.0).clamp(-100.0, 200.0)
}

/// In-race effective stat value: points above 1200 count half (`adjust_overcap`).
pub fn effective_in_race_value(stat: f64) -> f64 {
    if stat > 1200.0 {
        1200.0 + ((stat - 1200.0) / 2.0).floor()
    } else {
        stat
    }
}

/// Course set-status speed multiplier (port of `speed_modifier`): each threshold
/// stat contributes `(1 + floor(min(stat, 901) / 300.01)) × 0.05`, averaged over
/// the threshold list. Returns `1.0` when the course has no thresholds.
pub fn speed_set_status_multiplier(stats: [i32; 5], course: &CourseParams) -> f64 {
    let list = &course.set_status_thresholds;
    if list.is_empty() {
        return 1.0;
    }
    let sum: f64 = list
        .iter()
        .map(|k| {
            let v = (stats[k.index()] as f64).min(901.0);
            (1.0 + (v / 300.01).floor()) * 0.05
        })
        .sum();
    1.0 + sum / list.len() as f64
}

// ---------------------------------------------------------------------------
// Stamina survival threshold
// ---------------------------------------------------------------------------

/// Rush HP buffer expressed in *stamina points*: ≈45 (short) … 180 (long),
/// linear in distance. Rushing (掛かり) raises consumption ~1.6× for a window;
/// this reserves stamina so a rush does not break the spurt.
pub fn rush_buffer_stamina(distance: f64) -> f64 {
    let t = ((distance - 1200.0) / (3000.0 - 1200.0)).clamp(0.0, 1.0);
    45.0 + t * (180.0 - 45.0)
}

/// Stamina headroom freed up by recovery skills totalling `heal_bp` **basis
/// points** of max HP, given a `base_stamina` to size max HP against.
///
/// Game formula (uma-sim `calculate_equivalent_stamina`): a heal of `bp` basis
/// points restores `(bp/10000)·maxHP` HP, and one stamina point buys
/// `0.8·hp_coef` HP of max, so the heal is worth `actual / (0.8·hp_coef)`
/// stamina. With `maxHP = 0.8·hp_coef·stamina + distance`, this simplifies to
/// `(bp/10000)·(stamina + distance/(0.8·hp_coef))`. Recovery skills carry
/// different `bp` values (tiers 35…950), so the caller sums the player's planned
/// set. Proc randomness is ignored — this is the deliberate plan.
pub fn recovery_stamina_relief(heal_bp: f64, course: &CourseParams, strategy: Strategy, base_stamina: f64) -> f64 {
    if heal_bp <= 0.0 {
        return 0.0;
    }
    (heal_bp / 10000.0) * (base_stamina.max(0.0) + course.distance / (0.8 * strategy.hp_coef()))
}

/// The stamina the player actually needs to **train**, given a plan to run
/// recovery skills totalling `recovery_heal_bp` basis points: the survival floor
/// minus the recovery headroom (sized against the floor itself), never dropping
/// below the rush buffer (so it never implies "no stamina").
pub fn effective_stamina_need(
    course: &CourseParams,
    strategy: Strategy,
    guts: f64,
    speed: f64,
    distance_grade: i32,
    condition: GroundCondition,
    recovery_heal_bp: f64,
) -> f64 {
    let floor = stamina_survival_threshold(course, strategy, guts, speed, distance_grade, condition);
    let relief = recovery_stamina_relief(recovery_heal_bp, course, strategy, floor);
    (floor - relief).max(rush_buffer_stamina(course.distance))
}

/// Stamina needed to sustain a **full max last-spurt** for this course/strategy,
/// including the rush buffer. This is the dominant CM non-linearity: below it the
/// trainee gasses out and cannot spurt; above it, extra stamina is mostly wasted.
///
/// Heuristic closed-form: estimate total HP burned over the race as
/// `non-spurt portion (0 → 2/3 distance at mid speed) + spurt portion
/// (final 1/3 at max spurt speed, Guts modifier on)`, then solve
/// `max_hp(stamina) ≥ total_hp` for stamina and add the rush buffer. Recovery
/// skills are not modelled (the player equips those separately), so this is a
/// deliberately conservative survival floor.
pub fn stamina_survival_threshold(
    course: &CourseParams,
    strategy: Strategy,
    guts: f64,
    speed: f64,
    distance_grade: i32,
    condition: GroundCondition,
) -> f64 {
    let distance = course.distance;
    let bs = base_speed(distance);
    let ground = ground_consumption_coef(course.surface);
    let g_mod = guts_modifier(guts);

    // Soft/heavy ground lowers effective speed, so the trainee runs (and burns)
    // a little slower; use the condition-adjusted speed for the spurt estimate.
    let eff_speed = (speed + ground_speed_modifier(course.surface, condition) as f64).max(1.0);

    let mid_speed = mid_target_speed(strategy, bs);
    let spurt_speed = last_spurt_speed(eff_speed, guts, strategy, distance_grade, bs);

    // Non-spurt portion: start → 2/3 of the course at mid target speed (Guts off).
    let nonspurt_len = distance * 2.0 / 3.0;
    let hp_nonspurt = hp_per_second(mid_speed, bs, ground, g_mod, false) * (nonspurt_len / mid_speed);

    // Spurt portion: final third at max spurt speed (Guts on).
    let spurt_len = distance / 3.0;
    let hp_spurt = hp_per_second(spurt_speed, bs, ground, g_mod, true) * (spurt_len / spurt_speed);

    let total_hp = hp_nonspurt + hp_spurt;
    // Solve 0.8 * hp_coef * stamina + distance >= total_hp.
    let stamina = (total_hp - distance) / (0.8 * strategy.hp_coef());
    stamina.max(0.0) + rush_buffer_stamina(distance)
}

// ---------------------------------------------------------------------------
// Marginal stat value (the scoring backbone)
// ---------------------------------------------------------------------------

/// Heuristic anchors that scale the non-speed stats into the speed-derived
/// uutil unit. Tuned later against real builds; tests assert *shape* only.
const SPEED_DERIV_UUTIL: f64 = 1000.0; // m/s → uutil
/// How strongly being below the stamina survival floor is valued (stamina that
/// unlocks the spurt is mandatory, so it dominates while deficient).
const STAMINA_UNLOCK_UUTIL: f64 = 9.0;
/// Smoothing width (stamina points) of the survival knee.
const STAMINA_KNEE_WIDTH: f64 = 120.0;
/// Converts a power-driven acceleration *derivative* into an approximate
/// m/s-equivalent. This folds in the game's small acceleration coefficient
/// (≈0.0006) that the Speed branch carries as its `0.002` term — without it the
/// raw `0.5·√500/√power` derivative (~0.8) is ~190× the Speed per-point value and
/// makes Power dominate every facility. Calibrated so `mv_power` at low power
/// (~200) is a touch above `mv_speed`, tapering to ~0 past the course knee.
const POWER_ACCEL_TO_SPEED: f64 = 0.01;
/// Power "enough" knee center before the surface/distance adjustment.
const POWER_KNEE_BASE: f64 = 900.0;
/// Gentle, never-zero Wit value (skill-proc consistency; no soft cap).
const WIT_UUTIL: f64 = 1.4;
/// Baseline Guts value (minor: small last-spurt + HP-saving contribution).
const GUTS_UUTIL: f64 = 1.1;

/// Smooth 0→1 logistic ramp.
fn logistic(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Per-scenario career-race stat bonus `[Speed, Stamina, Power, Guts, Wit]`.
///
/// During career-mode races the game adds a flat stat line on top of the
/// trainee's trained stats (post-career events — Team Trials, Champions Meeting
/// — add nothing). The threshold math in [`stat_marginal_value`] must place us on
/// the in-race curve using `raw + bonus`, otherwise the stamina survival knee
/// sits ~bonus points too high and Stamina is over-valued mid-career.
///
/// Known values (extend as scenarios are verified):
/// - `1` URA Finale: `+400` to every stat (confirmed).
/// - `4` Trackblazer: assumed `+400` (unverified).
///
/// Unknown/other scenarios fall back to the URA line, mirroring the dashboard's
/// `scenarioRaceBonus`. `0` (no/unknown scenario) yields no bonus so non-career
/// scoring is unchanged.
#[must_use]
pub fn scenario_race_bonus(scenario_id: i32) -> [i32; 5] {
    const URA: [i32; 5] = [400, 400, 400, 400, 400];
    match scenario_id {
        0 => [0; 5],
        1 | 4 => URA,
        _ => URA,
    }
}

/// The race-value (uutil) of **one more point** of `stat`, given current stats,
/// the target course, strategy and aptitudes. Threshold-aware: it bakes in the
/// stamina survival floor, the 1200 soft-cap, the power "enough" knee, and the
/// Wit "no soft-cap" behaviour. See the module-level note for the unit.
pub fn stat_marginal_value(
    stat: StatKind,
    current: [i32; 5],
    course: &CourseParams,
    strategy: Strategy,
    apt: Aptitudes,
    condition: GroundCondition,
    recovery_heal_bp: f64,
    race_bonus: [i32; 5],
) -> f64 {
    // Soft/heavy ground (and dirt) applies a flat penalty to the effective
    // in-race speed/power, so a given target needs more *raw* stat to reach the
    // same curve position (soft cap / power knee shift up). The marginal
    // derivative is unchanged; only where we sit on the curve moves.
    // Career-race scenario bonus: a flat line added to every stat in-race, so the
    // curve position (survival floor, soft-caps, knees) is evaluated at `raw +
    // bonus`. The marginal derivative w.r.t. a *raw* training point is unchanged
    // (the bonus is a constant), so only where we sit on the curve shifts.
    let speed = (current[StatKind::Speed.index()] as f64
        + race_bonus[StatKind::Speed.index()] as f64
        + ground_speed_modifier(course.surface, condition) as f64)
        .max(1.0);
    let stamina = current[StatKind::Stamina.index()] as f64 + race_bonus[StatKind::Stamina.index()] as f64;
    let power = (current[StatKind::Power.index()] as f64
        + race_bonus[StatKind::Power.index()] as f64
        + ground_power_modifier(course.surface, condition) as f64)
        .max(1.0);
    let guts = current[StatKind::Guts.index()] as f64 + race_bonus[StatKind::Guts.index()] as f64;
    let wit = current[StatKind::Wit.index()] as f64 + race_bonus[StatKind::Wit.index()] as f64;

    // Overcap halves marginal in-race value above 1200.
    let overcap = |v: f64| if v >= 1200.0 { 0.5 } else { 1.0 };

    match stat {
        StatKind::Speed => {
            // d(last_spurt_speed)/d(speed): the Speed term appears in the phase-2
            // target (×1.05) and again in the spurt term ⇒ factor 2.05.
            let prof = speed_distance_prof(apt.distance_grade);
            let s_eff = speed.max(1.0);
            let dv = 2.05 * 0.002 * prof * (500.0_f64).sqrt() / (2.0 * s_eff.sqrt());
            dv * SPEED_DERIV_UUTIL * overcap(speed)
        }
        StatKind::Stamina => {
            // High below the survival floor, ~0 above (smooth knee). Crossing the
            // floor unlocks the full spurt, so deficient stamina dominates.
            // `speed` is already condition-adjusted above, so pass `Firm` here to
            // avoid applying the ground speed penalty twice. Planned recovery
            // skills lower the floor we actually need to train toward.
            let base =
                stamina_survival_threshold(course, strategy, guts, speed, apt.distance_grade, GroundCondition::Firm);
            let floor = base - recovery_stamina_relief(recovery_heal_bp, course, strategy, base);
            let deficit = floor - stamina;
            STAMINA_UNLOCK_UUTIL
                * SPEED_DERIV_UUTIL
                * 0.001
                * logistic(deficit / STAMINA_KNEE_WIDTH)
                * if stamina >= 1200.0 { 0.5 } else { 1.0 }
        }
        StatKind::Power => {
            // Acceleration derivative, ramped down past a course-tuned knee.
            let strat = strategy.speed_phase_coef(2).max(0.5); // proxy weight
            let g_prof = ground_surface_prof(apt.surface_grade);
            let d_prof = accel_distance_prof(apt.distance_grade);
            let p_eff = power.max(1.0);
            // accel ∝ (500·power)^0.5 ; derivative ∝ 0.5·sqrt(500)/sqrt(power).
            let d_accel = 0.5 * (500.0_f64).sqrt() / p_eff.sqrt() * strat * g_prof * d_prof;
            // Knee: longer / dirt courses want more power before tapering.
            let knee = power_knee(course);
            let knee_factor = 1.0 - logistic((power - knee) / 150.0) * 0.8;
            d_accel * POWER_ACCEL_TO_SPEED * SPEED_DERIV_UUTIL * knee_factor * overcap(power)
        }
        StatKind::Guts => {
            // Baseline: minor last-spurt + HP saving. Small bump for short / front.
            let g_eff = guts.max(1.0);
            let short = if course.distance <= 1600.0 { 1.4 } else { 1.0 };
            let front = matches!(strategy, Strategy::FrontRunner | Strategy::Runaway);
            let style = if front { 1.3 } else { 1.0 };
            let baseline = GUTS_UUTIL * short * style / g_eff.sqrt() * 10.0 * overcap(guts);

            // Stamina coupling: Guts lowers spurt HP burn, so it shrinks the
            // survival floor. When stamina is *below* that floor, an extra Guts
            // point is worth the stamina it frees up — valued in the same unit as
            // stamina-below-floor. Vanishes once stamina clears the floor (no
            // double-count). `speed` is already condition-adjusted ⇒ pass `Firm`.
            // Planned recovery skills lower the floor (so Guts matters less once
            // they cover the gap); the relief is sized against each base floor.
            let base =
                stamina_survival_threshold(course, strategy, guts, speed, apt.distance_grade, GroundCondition::Firm);
            let base_next = stamina_survival_threshold(
                course,
                strategy,
                guts + 1.0,
                speed,
                apt.distance_grade,
                GroundCondition::Firm,
            );
            let floor = base - recovery_stamina_relief(recovery_heal_bp, course, strategy, base);
            let floor_next = base_next - recovery_stamina_relief(recovery_heal_bp, course, strategy, base_next);
            let stamina_saved_per_guts = (floor - floor_next).max(0.0);
            let relief = STAMINA_UNLOCK_UUTIL
                * SPEED_DERIV_UUTIL
                * 0.001
                * stamina_saved_per_guts
                * logistic((floor - stamina) / STAMINA_KNEE_WIDTH);

            baseline + relief
        }
        StatKind::Wit => {
            // Gentle, diminishing, never zero (no soft cap per uma.guide). Style
            // aptitude scales wit's in-race usefulness, but proc rate is unaffected.
            let w_eff = wit.max(1.0);
            WIT_UUTIL * 30.0 / w_eff.sqrt()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course(distance: f64, surface: Surface, thresholds: Vec<StatKind>) -> CourseParams {
        CourseParams {
            distance,
            surface,
            turn: 1,
            set_status_thresholds: thresholds,
            finish_time_min: 0.0,
            finish_time_max: 0.0,
        }
    }

    // ---- parity anchors against uma-sim ----

    #[test]
    fn max_hp_parity() {
        // LateSurger 1200 stamina @ 2400 = 0.8*1.0*1200 + 2400 = 3360.
        assert_eq!(max_hp(1200.0, Strategy::LateSurger, 2400.0), 3360.0);
        // Runaway coef 0.86: 0.8*0.86*1000 + 1600 = 688 + 1600 = 2288.
        assert_eq!(max_hp(1000.0, Strategy::Runaway, 1600.0), 2288.0);
    }

    #[test]
    fn hp_strategy_coefficients_match() {
        assert_eq!(Strategy::FrontRunner.hp_coef(), 0.95);
        assert_eq!(Strategy::PaceChaser.hp_coef(), 0.89);
        assert_eq!(Strategy::LateSurger.hp_coef(), 1.0);
        assert_eq!(Strategy::EndCloser.hp_coef(), 0.995);
        assert_eq!(Strategy::Runaway.hp_coef(), 0.86);
    }

    #[test]
    fn base_speed_parity() {
        assert_eq!(base_speed(2000.0), 20.0);
        assert_eq!(base_speed(2400.0), 19.6);
        assert_eq!(base_speed(1600.0), 20.4);
    }

    #[test]
    fn overcap_halves_above_1200() {
        assert_eq!(effective_in_race_value(1000.0), 1000.0);
        assert_eq!(effective_in_race_value(1300.0), 1250.0); // 1200 + floor(100/2)
        assert_eq!(effective_in_race_value(1500.0), 1350.0); // 1200 + 150
    }

    #[test]
    fn set_status_multiplier_parity() {
        // One Speed threshold at 900 → (1 + floor(900/300.01))*0.05 = (1+2)*0.05 = 0.15.
        let c = course(2000.0, Surface::Turf, vec![StatKind::Speed]);
        let m = speed_set_status_multiplier([900, 0, 0, 0, 0], &c);
        assert!((m - 1.15).abs() < 1e-9);
        // No thresholds → 1.0.
        let c0 = course(2000.0, Surface::Turf, vec![]);
        assert_eq!(speed_set_status_multiplier([900, 900, 900, 900, 900], &c0), 1.0);
    }

    #[test]
    fn guts_modifier_decreases_with_guts() {
        assert!(guts_modifier(1200.0) < guts_modifier(400.0));
        assert!(guts_modifier(400.0) > 1.0);
    }

    // ---- survival-threshold shape ----

    #[test]
    fn survival_threshold_grows_with_distance() {
        let short = course(1600.0, Surface::Turf, vec![]);
        let long = course(2400.0, Surface::Turf, vec![]);
        let t_short = stamina_survival_threshold(&short, Strategy::LateSurger, 400.0, 1000.0, 7, GroundCondition::Firm);
        let t_long = stamina_survival_threshold(&long, Strategy::LateSurger, 400.0, 1000.0, 7, GroundCondition::Firm);
        assert!(
            t_long > t_short,
            "longer course needs more stamina ({t_short} -> {t_long})"
        );
        // Sanity: a 2400m survival floor lands in a plausible CM range.
        assert!((300.0..1400.0).contains(&t_long), "implausible threshold {t_long}");
    }

    #[test]
    fn survival_threshold_lower_for_better_hp_strategy() {
        let c = course(2400.0, Surface::Turf, vec![]);
        let sashi = stamina_survival_threshold(&c, Strategy::LateSurger, 400.0, 1000.0, 7, GroundCondition::Firm); // coef 1.0
        let senko = stamina_survival_threshold(&c, Strategy::PaceChaser, 400.0, 1000.0, 7, GroundCondition::Firm); // coef 0.89
                                                                                                                   // Worse conversion (senko) needs MORE stamina for the same HP.
        assert!(senko > sashi);
    }

    // ---- marginal-value shape ----

    #[test]
    fn speed_marginal_drops_past_1200() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let below = stat_marginal_value(
            StatKind::Speed,
            [900, 600, 600, 400, 600],
            &c,
            Strategy::PaceChaser,
            Aptitudes {
                distance_grade: 7,
                surface_grade: 7,
            },
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let above = stat_marginal_value(
            StatKind::Speed,
            [1300, 600, 600, 400, 600],
            &c,
            Strategy::PaceChaser,
            Aptitudes {
                distance_grade: 7,
                surface_grade: 7,
            },
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(
            below > above,
            "speed value should fall past the soft cap ({below} vs {above})"
        );
    }

    #[test]
    fn stamina_marginal_high_below_floor_low_above() {
        let c = course(2400.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let floor = stamina_survival_threshold(&c, Strategy::LateSurger, 400.0, 1100.0, 7, GroundCondition::Firm);
        let low_stam = (floor - 250.0).max(50.0) as i32;
        let high_stam = (floor + 350.0) as i32;
        let deficient = stat_marginal_value(
            StatKind::Stamina,
            [1100, low_stam, 800, 400, 600],
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let satisfied = stat_marginal_value(
            StatKind::Stamina,
            [1100, high_stam, 800, 400, 600],
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(
            deficient > satisfied * 3.0,
            "stamina must dominate below the floor ({deficient} vs {satisfied})"
        );
    }

    #[test]
    fn scenario_race_bonus_table_mirrors_dashboard() {
        // URA (1) and Trackblazer (4) get the +400 line; 0 ⇒ no bonus.
        assert_eq!(scenario_race_bonus(1), [400; 5]);
        assert_eq!(scenario_race_bonus(4), [400; 5]);
        assert_eq!(scenario_race_bonus(0), [0; 5]);
        // Unknown scenarios fall back to the URA line (mirrors the web `?? URA`).
        assert_eq!(scenario_race_bonus(99), [400; 5]);
    }

    #[test]
    fn career_race_bonus_lowers_the_stamina_knee() {
        // A stamina value that is below the *raw* survival floor but comfortably
        // above it once the +400 career-race bonus is folded in. The bonus must
        // move us past the knee, dropping Stamina's marginal value sharply.
        let c = course(2400.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let floor = stamina_survival_threshold(&c, Strategy::LateSurger, 400.0, 1100.0, 7, GroundCondition::Firm);
        // ~150 under the raw floor: deficient on raw stats, but +400 clears it.
        let stam = (floor - 150.0).max(50.0) as i32;
        let current = [1100, stam, 800, 400, 600];
        let raw = stat_marginal_value(
            StatKind::Stamina,
            current,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let career = stat_marginal_value(
            StatKind::Stamina,
            current,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            scenario_race_bonus(1),
        );
        assert!(
            career < raw * 0.5,
            "career race bonus should clear the stamina knee, cutting its value \
             (raw {raw} vs career {career})"
        );
    }

    #[test]
    fn power_marginal_ramps_down_past_knee() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let low = stat_marginal_value(
            StatKind::Power,
            [1100, 700, 500, 400, 600],
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let high = stat_marginal_value(
            StatKind::Power,
            [1100, 700, 1150, 400, 600],
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(low > high, "power value should taper past the knee ({low} vs {high})");
    }

    #[test]
    fn wit_marginal_positive_with_no_hard_cap() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let mid = stat_marginal_value(
            StatKind::Wit,
            [1100, 700, 800, 400, 800],
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let high = stat_marginal_value(
            StatKind::Wit,
            [1100, 700, 800, 400, 1400],
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(mid > 0.0 && high > 0.0, "wit always has positive value");
        assert!(mid > high, "wit has diminishing (not zero) returns");
    }

    #[test]
    fn guts_is_minor_relative_to_speed_and_stamina() {
        let c = course(2400.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        // Stamina kept well above the survival floor so it is NOT the bottleneck;
        // then Guts is the minor stat it should normally be (vs Speed).
        let cur = [1000, 1800, 800, 400, 600];
        let guts = stat_marginal_value(
            StatKind::Guts,
            cur,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let speed = stat_marginal_value(
            StatKind::Speed,
            cur,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(guts > 0.0);
        assert!(guts < speed, "guts should be a minor stat vs speed ({guts} vs {speed})");
    }

    // ---- ground condition ----

    #[test]
    fn ground_modifiers_match_reference_tables() {
        // Speed only penalized on Heavy (both surfaces).
        assert_eq!(ground_speed_modifier(Surface::Turf, GroundCondition::Firm), 0);
        assert_eq!(ground_speed_modifier(Surface::Turf, GroundCondition::Heavy), -50);
        assert_eq!(ground_speed_modifier(Surface::Dirt, GroundCondition::Heavy), -50);
        // Power: turf soft/heavy -50; dirt is power-hungry even on firm.
        assert_eq!(ground_power_modifier(Surface::Turf, GroundCondition::Soft), -50);
        assert_eq!(ground_power_modifier(Surface::Dirt, GroundCondition::Firm), -100);
        assert_eq!(ground_power_modifier(Surface::Dirt, GroundCondition::Good), -50);
    }

    #[test]
    fn heavy_ground_raises_power_value_for_a_given_raw_stat() {
        // On heavy ground the effective power is lower, so a fixed raw power sits
        // earlier on the knee curve => a marginal point is worth more. (Turf: Firm
        // power penalty 0 vs Heavy -50.)
        let c = course(2000.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let cur = [1100, 700, 900, 400, 600];
        let firm = stat_marginal_value(
            StatKind::Power,
            cur,
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let heavy = stat_marginal_value(
            StatKind::Power,
            cur,
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Heavy,
            0.0,
            [0; 5],
        );
        assert!(
            heavy > firm,
            "heavy ground should make raw power more valuable ({heavy} vs {firm})"
        );
    }

    #[test]
    fn heavy_ground_shifts_speed_soft_cap_up() {
        // Effective speed is -50 on heavy, so a raw speed just past 1200 still
        // sits below the in-race soft cap => it keeps the higher marginal value.
        let c = course(2000.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let cur = [1230, 700, 800, 400, 600];
        let firm = stat_marginal_value(
            StatKind::Speed,
            cur,
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let heavy = stat_marginal_value(
            StatKind::Speed,
            cur,
            &c,
            Strategy::PaceChaser,
            apt,
            GroundCondition::Heavy,
            0.0,
            [0; 5],
        );
        assert!(
            heavy > firm,
            "heavy ground should push the speed soft cap higher ({heavy} vs {firm})"
        );
    }

    // ---- marginal-value cross-stat magnitude (scale sanity) ----

    #[test]
    fn power_marginal_is_same_order_as_speed() {
        // Regression guard: Power once mis-scaled to ~190x Speed (missing the
        // acceleration coefficient), which made it dominate every facility.
        let c = course(2000.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 8,
            surface_grade: 8,
        };
        let cur = [155, 144, 197, 92, 127];
        let speed = stat_marginal_value(
            StatKind::Speed,
            cur,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let power = stat_marginal_value(
            StatKind::Power,
            cur,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(speed > 0.0 && power > 0.0);
        assert!(
            power < 5.0 * speed,
            "power must be the same order as speed, not dominate ({power} vs {speed})"
        );
    }

    #[test]
    fn guts_gains_value_when_stamina_starved_on_long_course() {
        // Guts lowers the survival floor, so on a long course where stamina is
        // far below the floor an extra Guts point should be worth noticeably
        // more than when stamina is comfortably above the floor.
        // 2000m so the "satisfied" stamina is genuinely above the floor.
        let c = course(2000.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 8,
            surface_grade: 8,
        };
        let starved = stat_marginal_value(
            StatKind::Guts,
            [1100, 300, 800, 400, 600],
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let satisfied = stat_marginal_value(
            StatKind::Guts,
            [1100, 1300, 800, 400, 600],
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        assert!(
            starved > satisfied * 1.5,
            "guts should be worth more when stamina-starved ({starved} vs {satisfied})"
        );
    }

    // ---- recovery relief ----

    #[test]
    fn recovery_relief_matches_game_formula() {
        // (bp/10000)*(stamina + distance/(0.8*hp_coef)). LateSurger coef 1.0,
        // 2400m, base stamina 1000, 600 bp => 0.06*(1000 + 2400/0.8) = 0.06*4000 = 240.
        let c = course(2400.0, Surface::Turf, vec![]);
        let relief = recovery_stamina_relief(600.0, &c, Strategy::LateSurger, 1000.0);
        assert!((relief - 240.0).abs() < 1e-6, "got {relief}");
        assert_eq!(recovery_stamina_relief(0.0, &c, Strategy::LateSurger, 1000.0), 0.0);
    }

    #[test]
    fn effective_need_decreases_with_recovery_and_floors_at_rush_buffer() {
        let c = course(2400.0, Surface::Turf, vec![]);
        let none = effective_stamina_need(&c, Strategy::LateSurger, 400.0, 1000.0, 7, GroundCondition::Firm, 0.0);
        let some = effective_stamina_need(&c, Strategy::LateSurger, 400.0, 1000.0, 7, GroundCondition::Firm, 600.0);
        let lots = effective_stamina_need(
            &c,
            Strategy::LateSurger,
            400.0,
            1000.0,
            7,
            GroundCondition::Firm,
            99999.0,
        );
        assert!(some < none, "recovery should lower the need ({some} vs {none})");
        assert!(
            lots >= rush_buffer_stamina(c.distance) - 1e-6,
            "never below the rush buffer"
        );
    }

    #[test]
    fn recovery_lowers_stamina_marginal_value() {
        // With enough recovery to clear the floor, an extra stamina point is
        // worth much less than with no recovery (the floor is covered).
        let c = course(2400.0, Surface::Turf, vec![]);
        let apt = Aptitudes {
            distance_grade: 7,
            surface_grade: 7,
        };
        let cur = [1100, 700, 800, 400, 600];
        let no_rec = stat_marginal_value(
            StatKind::Stamina,
            cur,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            0.0,
            [0; 5],
        );
        let big_rec = stat_marginal_value(
            StatKind::Stamina,
            cur,
            &c,
            Strategy::LateSurger,
            apt,
            GroundCondition::Firm,
            4000.0,
            [0; 5],
        );
        assert!(
            big_rec < no_rec,
            "recovery should de-value stamina ({big_rec} vs {no_rec})"
        );
    }
}
