//! Multi-turn planning layer (pure): lifts the recommender beyond the greedy
//! per-turn scorer in [`crate::recommend`] by layering three trajectory-aware
//! terms onto its objective scores:
//!
//! 1. **Energy / rest option-value** — when HP is low (and failures cascade),
//!    resting has option value: it preserves the turns you have left. Surfaced as
//!    a fatigue discount on training value plus a rest override on the turn
//!    suggestion. Uses live `hp/max_hp/motivation/failure_rates` only.
//! 2. **Bond / rainbow lookahead** — training with a support that is *near* the
//!    friendship (rainbow) threshold now pays off over future turns. Modelled as a
//!    per-facility uplift, weighted toward the early career (front-load bonds). The
//!    per-facility "near-rainbow pressure" is read live from each facility's
//!    present supports (`memory_reader::command_info` walks the `TrainingHorseList`
//!    bond gauges); it degrades to zero — back to greedy — when no partner is near
//!    the threshold or the data is unavailable.
//! 3. **Career-phase weighting** — late in the career, facilities that close a
//!    distance-from-target gap ([`crate::build_profile`]) are boosted, shifting the
//!    plan from bond-building early to stat-maxing late.
//!
//! ## Design constraints
//!
//! Pure, deterministic, allocation-free, and cheap (it runs on the render thread).
//! No game simulation: instead of a real N-turn rollout, the depth/aggressiveness
//! knob scales a **discounted closed-form influence** (`influence`) that fades all
//! three terms toward zero, so `lookahead_depth == 0` reproduces the greedy result
//! exactly. It layers on top of [`recommend`]'s objective delta and never forks the
//! objective logic; it degrades to greedy whenever its extra signals are missing.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::recommend::{self, FacilityScore, RecommendParams, TurnSuggestion};

// ---------------------------------------------------------------------------
// Tunable constants (documented heuristics; tests assert *shape*, not magnitude)
// ---------------------------------------------------------------------------

/// Approximate total training turns in a classic career. `GetFinalTurn` does a
/// master-data lookup that crashes on the render thread, so we cannot read the
/// real length live; this constant only drives *relative* phase weighting, which
/// is robust to a modest error.
pub const CAREER_TURNS_TOTAL: i32 = 78;
/// Bond value at which a support becomes friendship- (rainbow-) trainable.
pub const RAINBOW_BOND_THRESHOLD: i32 = 80;
/// Hard cap on the lookahead depth knob (keeps the render-thread budget bounded).
pub const MAX_LOOKAHEAD_DEPTH: i32 = 4;

/// Stat-point gap that counts as a "full" (==1.0) deficit when normalizing the
/// distance-from-target term.
const DEFICIT_NORM: f32 = 400.0;
/// How strongly a near-rainbow facility is boosted early-career (per unit of
/// pressure × influence × early-weight).
const BOND_GAIN: f64 = 1.5;
/// How strongly an under-target facility is boosted late-career.
const TARGET_GAIN: f64 = 1.0;
/// How much low energy discounts training value (per unit of fatigue × influence).
const FATIGUE_PEN: f64 = 0.4;
/// Rest option-value gain: scales how readily a low-HP turn flips to Rest.
const REST_GAIN: f64 = 8.0;
/// Extra rest pull per motivation level below Normal (3): low mood makes a
/// recovery/recreation turn comparatively more attractive.
const MOOD_REST_GAIN: f64 = 0.25;

// ---------------------------------------------------------------------------
// Persisted, user-tunable params
// ---------------------------------------------------------------------------

/// User-tunable lookahead knobs. Persisted in `training_config.json` (see
/// [`crate::config`]); every field is `#[serde(default)]` for clean migration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PlannerParams {
    /// Lookahead depth `0..=MAX_LOOKAHEAD_DEPTH`. `0` ⇒ pure greedy (the planner
    /// is a no-op and the suggestion equals [`recommend::turn_suggestion`]).
    #[serde(default = "default_depth")]
    pub lookahead_depth: i32,
    /// Overall multi-turn aggressiveness multiplier `0.0..=2.0`.
    #[serde(default = "default_aggressiveness")]
    pub lookahead_aggressiveness: f32,
    /// HP % below which rest/fatigue pressure ramps up.
    #[serde(default = "default_energy_floor")]
    pub energy_floor_pct: i32,
    /// Opt-in: only count a support's near-rainbow pressure toward a facility when
    /// that facility is the card's **own specialty** (Speed card on Speed, etc.),
    /// matching where a friendship/rainbow training can actually fire. Off by
    /// default — the bond term then credits bond-building on any facility the card
    /// currently sits on (its future-rainbow value). Pal/friend/group cards never
    /// contribute their own pressure under either mode (they have no rainbow).
    #[serde(default)]
    pub specialty_rainbow_gating: bool,
}

impl PlannerParams {
    /// Conservative built-in defaults: a shallow lookahead with a mild energy
    /// floor. Non-zero so the layer is active out of the box, but gentle enough
    /// not to upend the greedy pick at full HP.
    pub const DEFAULT: PlannerParams = PlannerParams {
        lookahead_depth: 2,
        lookahead_aggressiveness: 0.6,
        energy_floor_pct: 40,
        specialty_rainbow_gating: false,
    };

    /// Clamp every field to a sane range.
    fn clamped(self) -> Self {
        Self {
            lookahead_depth: self.lookahead_depth.clamp(0, MAX_LOOKAHEAD_DEPTH),
            lookahead_aggressiveness: self.lookahead_aggressiveness.clamp(0.0, 2.0),
            energy_floor_pct: self.energy_floor_pct.clamp(0, 100),
            specialty_rainbow_gating: self.specialty_rainbow_gating,
        }
    }
}

impl Default for PlannerParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn default_depth() -> i32 {
    PlannerParams::DEFAULT.lookahead_depth
}
fn default_aggressiveness() -> f32 {
    PlannerParams::DEFAULT.lookahead_aggressiveness
}
fn default_energy_floor() -> i32 {
    PlannerParams::DEFAULT.energy_floor_pct
}

/// Live, user-tunable parameters (defaults until config loads).
static PARAMS: Mutex<PlannerParams> = Mutex::new(PlannerParams::DEFAULT);

/// Current planner parameters.
pub fn params() -> PlannerParams {
    PARAMS.lock().map(|g| *g).unwrap_or(PlannerParams::DEFAULT)
}

/// Replace the parameters (clamped). Call [`crate::config::persist`] to save.
pub fn set_params(p: PlannerParams) {
    if let Ok(mut g) = PARAMS.lock() {
        *g = p.clamped();
    }
}

// ---------------------------------------------------------------------------
// Context
// ---------------------------------------------------------------------------

/// Everything the planner needs beyond the greedy per-facility scores. Built by
/// the UI from the live snapshot + active build profile; the planner only reads
/// it (stays pure).
#[derive(Debug, Clone, Copy)]
pub struct PlannerContext {
    /// Current energy.
    pub hp: i32,
    /// Maximum energy (`0` ⇒ unknown — disables the energy term).
    pub max_hp: i32,
    /// Motivation level `1..=5` (1 Awful … 5 Great). Below Normal (3) it adds
    /// rest pull, since a low-mood turn trains poorly.
    pub motivation: i32,
    /// Current career turn (drives phase weighting).
    pub current_turn: i32,
    /// Per-facility failure % (`< 0` ⇒ unknown), used by the rest option-value.
    pub failure_rates: [i32; 5],
    /// Per-facility normalized distance-from-target `0..=1` (facility `i` raises
    /// stat `i`); drives the late-career stat-maxing boost.
    pub stat_deficit: [f32; 5],
    /// Per-facility near-rainbow pressure `0..=1`, or `None` when support
    /// placement is unknown (then the bond term degrades to greedy).
    pub bond_pressure: Option<[f32; 5]>,
}

impl Default for PlannerContext {
    fn default() -> Self {
        Self {
            hp: 100,
            max_hp: 100,
            motivation: 3,
            current_turn: 0,
            failure_rates: [-1; 5],
            stat_deficit: [0.0; 5],
            bond_pressure: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Core scalars
// ---------------------------------------------------------------------------

/// Discounted closed-form lookahead influence `≥ 0`: `aggressiveness × depth/CAP`.
/// `0` ⇒ greedy (every multi-turn term vanishes). Monotonic in both knobs.
#[must_use]
pub fn influence(params: &PlannerParams) -> f32 {
    let depth = params.lookahead_depth.clamp(0, MAX_LOOKAHEAD_DEPTH) as f32 / MAX_LOOKAHEAD_DEPTH as f32;
    (params.lookahead_aggressiveness.max(0.0) * depth).max(0.0)
}

/// Fatigue `0..=1`: `0` at/above the energy floor, ramping to `1` at empty HP.
/// `0` when `max_hp` is unknown (energy term disabled).
fn fatigue(ctx: &PlannerContext, params: &PlannerParams) -> f32 {
    if ctx.max_hp <= 0 {
        return 0.0;
    }
    let hp_pct = (ctx.hp.max(0) as f32 / ctx.max_hp as f32) * 100.0;
    let floor = params.energy_floor_pct.max(1) as f32;
    if hp_pct >= floor {
        0.0
    } else {
        ((floor - hp_pct) / floor).clamp(0.0, 1.0)
    }
}

/// Fraction of the career still ahead `0..=1` (`1` at the start, `0` at the end).
fn phase_remaining(turn: i32) -> f32 {
    let rem = (CAREER_TURNS_TOTAL - turn.max(0)).max(0) as f32;
    (rem / CAREER_TURNS_TOTAL as f32).clamp(0.0, 1.0)
}

/// Mean of the known (`>= 0`) failure rates, or `0` when none are known.
fn mean_known_failure(ctx: &PlannerContext) -> f32 {
    let (sum, n) = ctx
        .failure_rates
        .iter()
        .fold((0i32, 0i32), |(s, n), &f| if f >= 0 { (s + f, n + 1) } else { (s, n) });
    if n == 0 {
        0.0
    } else {
        sum as f32 / n as f32
    }
}

/// Per-facility normalized distance-from-target `0..=1` (facility `i` ↔ stat `i`).
/// Ceiling is the manual target when set, else the live cap; `0` when neither is
/// known. The UI calls this when building the [`PlannerContext`].
#[must_use]
pub fn stat_deficits(current: [i32; 5], targets: [i32; 5], caps: [i32; 5]) -> [f32; 5] {
    std::array::from_fn(|i| {
        let ceiling = if targets[i] > 0 { targets[i] } else { caps[i] };
        if ceiling <= 0 {
            return 0.0;
        }
        ((ceiling - current[i]).max(0) as f32 / DEFICIT_NORM).clamp(0.0, 1.0)
    })
}

/// Near-rainbow pressure `0..=1` for a single support's bond value: rises as the
/// bond approaches [`RAINBOW_BOND_THRESHOLD`] (closer ⇒ crosses sooner ⇒ more
/// future-turn value), and is `0` once already rainbow (no further unlock). A
/// helper for callers that *do* know per-facility support placement to build
/// [`PlannerContext::bond_pressure`]. Consumed by `memory_reader::command_info`,
/// which reads each facility's `TrainingHorseList` bond values live.
#[must_use]
pub fn near_rainbow_pressure(bond: i32) -> f32 {
    let b = bond.clamp(0, 100);
    if b >= RAINBOW_BOND_THRESHOLD {
        0.0
    } else {
        (b as f32 / RAINBOW_BOND_THRESHOLD as f32).clamp(0.0, 1.0)
    }
}

// ---------------------------------------------------------------------------
// Score adjustment + suggestion
// ---------------------------------------------------------------------------

/// Re-flag the single best known facility after scores change.
fn recompute_best(scores: &mut [FacilityScore; 5]) {
    let mut best: Option<(usize, i32)> = None;
    for (i, s) in scores.iter().enumerate() {
        if s.known && best.is_none_or(|(_, bs)| s.score > bs) {
            best = Some((i, s.score));
        }
    }
    for s in scores.iter_mut() {
        s.is_best = false;
    }
    if let Some((b, _)) = best {
        scores[b].is_best = true;
    }
}

/// Layer the multi-turn terms onto the greedy per-facility scores: a fatigue
/// discount (low energy), an early-career bond uplift, and a late-career
/// distance-from-target boost. With `influence == 0` (greedy) the input is
/// returned unchanged. The best-facility flag is recomputed.
#[must_use]
pub fn adjust_scores(base: &[FacilityScore; 5], ctx: &PlannerContext, params: &PlannerParams) -> [FacilityScore; 5] {
    let mut out = *base;
    let inf = influence(params) as f64;
    if inf <= 0.0 {
        return out; // greedy
    }

    let fat = fatigue(ctx, params) as f64;
    let phase_rem = phase_remaining(ctx.current_turn) as f64;
    let early_w = phase_rem; // 1 early → 0 late
    let late_w = 1.0 - phase_rem; // 0 early → 1 late
    let fatigue_factor = 1.0 - (inf * fat * FATIGUE_PEN);

    for (i, slot) in out.iter_mut().enumerate() {
        if !slot.known {
            continue;
        }
        let bond = ctx.bond_pressure.map_or(0.0, |b| b[i].clamp(0.0, 1.0) as f64);
        let uplift = 1.0 + inf * bond * early_w * BOND_GAIN;
        let target = 1.0 + inf * (ctx.stat_deficit[i].clamp(0.0, 1.0) as f64) * late_w * TARGET_GAIN;
        let factor = uplift * target * fatigue_factor;
        slot.score = (slot.score as f64 * factor).round() as i32;
    }

    recompute_best(&mut out);
    out
}

/// Rest option-value for this turn, in the same score units as a facility score
/// (anchored to `best_score` so the comparison is unit-consistent). Returns
/// `−∞` when the planner is greedy or energy is full, so rest never overrides.
#[must_use]
pub fn rest_value(best_score: i32, ctx: &PlannerContext, params: &PlannerParams) -> f64 {
    let inf = influence(params) as f64;
    if inf <= 0.0 {
        return f64::NEG_INFINITY;
    }
    let fat = fatigue(ctx, params) as f64;
    if fat <= 0.0 {
        return f64::NEG_INFINITY;
    }
    let phase_rem = phase_remaining(ctx.current_turn) as f64;
    let avg_fail = mean_known_failure(ctx) as f64 / 100.0;
    // Resting is worth less late-career (fewer turns to spend the energy) but
    // never nil — a final-stretch failure still hurts.
    let late_relief = 0.4 + 0.6 * phase_rem;
    // Low motivation (below Normal) adds rest pull.
    let mood_deficit = (3 - ctx.motivation.clamp(1, 5)).max(0) as f64;
    let mood = 1.0 + MOOD_REST_GAIN * mood_deficit;
    best_score.max(0) as f64 * fat * (1.0 + avg_fail) * late_relief * mood * inf * REST_GAIN
}

/// The planned turn suggestion: starts from [`recommend::turn_suggestion`] (which
/// owns the all-facilities-too-risky Rest/Race fallback) and adds an **energy
/// override** — when fatigue is high enough that the rest option-value beats the
/// best training pick, it flips a `Train` to `Rest` (or `Race` on race-encouraged
/// scenarios). Greedy (`influence == 0`) or full HP ⇒ identical to the base.
#[must_use]
pub fn plan_suggestion(
    adjusted: &[FacilityScore; 5],
    failure_rates: [i32; 5],
    race_encouraged: bool,
    ctx: &PlannerContext,
    recommend_params: &RecommendParams,
    params: &PlannerParams,
) -> TurnSuggestion {
    let base = recommend::turn_suggestion(adjusted, failure_rates, race_encouraged, recommend_params);
    let TurnSuggestion::Train(i) = base else {
        return base; // Rest/Race fallback already chosen
    };
    let best = adjusted[i].score;
    if rest_value(best, ctx, params) > best as f64 {
        return if race_encouraged {
            TurnSuggestion::Race
        } else {
            TurnSuggestion::Rest
        };
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known(score: i32) -> FacilityScore {
        FacilityScore {
            score,
            is_best: false,
            known: true,
        }
    }

    /// Five known facilities with the given scores.
    fn scores(s: [i32; 5]) -> [FacilityScore; 5] {
        std::array::from_fn(|i| known(s[i]))
    }

    fn ctx() -> PlannerContext {
        PlannerContext {
            hp: 100,
            max_hp: 100,
            motivation: 3,
            current_turn: 10,
            failure_rates: [5; 5],
            stat_deficit: [0.0; 5],
            bond_pressure: None,
        }
    }

    fn p() -> PlannerParams {
        PlannerParams::default()
    }

    // ---- params ----

    #[test]
    fn params_clamp_to_sane_ranges() {
        let c = PlannerParams {
            lookahead_depth: 99,
            lookahead_aggressiveness: 5.0,
            energy_floor_pct: 200,
            specialty_rainbow_gating: false,
        }
        .clamped();
        assert_eq!(c.lookahead_depth, MAX_LOOKAHEAD_DEPTH);
        assert_eq!(c.lookahead_aggressiveness, 2.0);
        assert_eq!(c.energy_floor_pct, 100);
        let c2 = PlannerParams {
            lookahead_depth: -3,
            lookahead_aggressiveness: -1.0,
            energy_floor_pct: -10,
            specialty_rainbow_gating: true,
        }
        .clamped();
        assert_eq!(c2.lookahead_depth, 0);
        assert_eq!(c2.lookahead_aggressiveness, 0.0);
        assert_eq!(c2.energy_floor_pct, 0);
    }

    #[test]
    fn influence_is_monotone_in_depth_and_zero_at_greedy() {
        let mk = |d: i32| {
            influence(&PlannerParams {
                lookahead_depth: d,
                ..p()
            })
        };
        assert_eq!(mk(0), 0.0);
        assert!(mk(1) < mk(2));
        assert!(mk(2) < mk(4));
    }

    // ---- degrade to greedy ----

    #[test]
    fn depth_zero_is_a_noop() {
        let base = scores([100, 90, 80, 70, 60]);
        let greedy = PlannerParams {
            lookahead_depth: 0,
            ..p()
        };
        let mut ctx = ctx();
        ctx.hp = 5; // even exhausted + deficits, depth 0 must not move anything
        ctx.stat_deficit = [1.0; 5];
        ctx.bond_pressure = Some([1.0; 5]);
        let out = adjust_scores(&base, &ctx, &greedy);
        for i in 0..5 {
            assert_eq!(out[i].score, base[i].score);
        }
        // And the suggestion matches the bare greedy one.
        let adj = out;
        let planned = plan_suggestion(&adj, [5; 5], false, &ctx, &RecommendParams::default(), &greedy);
        let greedy_sg = recommend::turn_suggestion(&adj, [5; 5], false, &RecommendParams::default());
        assert_eq!(planned, greedy_sg);
    }

    #[test]
    fn missing_bond_pressure_degrades_that_term() {
        let base = scores([100, 100, 100, 100, 100]);
        let mut c = ctx();
        c.current_turn = 2; // early career so a bond term *would* fire if present
        let no_bond = adjust_scores(&base, &c, &p());
        c.bond_pressure = Some([0.0, 0.0, 1.0, 0.0, 0.0]);
        let with_bond = adjust_scores(&base, &c, &p());
        assert_eq!(no_bond[2].score, base[2].score, "no pressure ⇒ greedy on that facility");
        assert!(
            with_bond[2].score > base[2].score,
            "near-rainbow pressure raises the facility"
        );
    }

    // ---- bond / rainbow lookahead ----

    #[test]
    fn near_rainbow_support_raises_facility_early_not_late() {
        let base = scores([100, 100, 100, 100, 100]);
        let mut early = ctx();
        early.current_turn = 2;
        early.bond_pressure = Some([0.0, 0.0, 1.0, 0.0, 0.0]);
        let mut late = early;
        late.current_turn = CAREER_TURNS_TOTAL - 2;
        let e = adjust_scores(&base, &early, &p());
        let l = adjust_scores(&base, &late, &p());
        assert!(e[2].score > base[2].score, "early bond uplift");
        assert!(
            e[2].score > l[2].score,
            "bond matters more early than late ({} vs {})",
            e[2].score,
            l[2].score
        );
    }

    #[test]
    fn near_rainbow_pressure_peaks_just_below_threshold() {
        assert_eq!(near_rainbow_pressure(0), 0.0);
        assert!(near_rainbow_pressure(40) > near_rainbow_pressure(0));
        assert!(near_rainbow_pressure(79) > near_rainbow_pressure(40));
        assert_eq!(
            near_rainbow_pressure(RAINBOW_BOND_THRESHOLD),
            0.0,
            "already rainbow ⇒ no unlock value"
        );
        assert_eq!(near_rainbow_pressure(95), 0.0);
    }

    // ---- career-phase / target weighting ----

    #[test]
    fn late_career_shifts_weight_to_unmet_targets() {
        let base = scores([100, 100, 100, 100, 100]);
        let mut c = ctx();
        c.stat_deficit = [0.0, 1.0, 0.0, 0.0, 0.0]; // Stamina far from target
        let mut early = c;
        early.current_turn = 2;
        let mut late = c;
        late.current_turn = CAREER_TURNS_TOTAL - 2;
        let e = adjust_scores(&base, &early, &p());
        let l = adjust_scores(&base, &late, &p());
        assert!(
            l[1].score > e[1].score,
            "under-target stat boosted more late ({} vs {})",
            l[1].score,
            e[1].score
        );
        assert!(l[1].score > base[1].score);
    }

    #[test]
    fn stat_deficits_normalize_target_then_cap() {
        // Target set below cap drives the deficit; gain headroom is (target-current).
        let d = stat_deficits([800, 0, 0, 0, 0], [1200, 0, 0, 0, 0], [1500, 0, 0, 0, 0]);
        assert!((d[0] - (400.0 / DEFICIT_NORM)).abs() < 1e-6);
        // No target ⇒ fall back to cap.
        let d2 = stat_deficits([1100, 0, 0, 0, 0], [0, 0, 0, 0, 0], [1200, 0, 0, 0, 0]);
        assert!((d2[0] - (100.0 / DEFICIT_NORM)).abs() < 1e-6);
        // Neither known ⇒ 0; already past ceiling ⇒ 0.
        assert_eq!(stat_deficits([500, 0, 0, 0, 0], [0; 5], [0; 5])[0], 0.0);
        assert_eq!(stat_deficits([1300, 0, 0, 0, 0], [1200, 0, 0, 0, 0], [0; 5])[0], 0.0);
    }

    // ---- energy / rest ----

    #[test]
    fn fatigue_rises_as_hp_falls() {
        let mut hi = ctx();
        hi.hp = 90;
        let mut lo = ctx();
        lo.hp = 10;
        assert_eq!(fatigue(&hi, &p()), 0.0, "above the floor ⇒ no fatigue");
        assert!(fatigue(&lo, &p()) > 0.0);
        let mut mid = ctx();
        mid.hp = 20;
        assert!(fatigue(&lo, &p()) > fatigue(&mid, &p()), "lower HP ⇒ more fatigue");
    }

    #[test]
    fn rest_value_monotone_in_fatigue() {
        let mut hi = ctx();
        hi.hp = 30;
        let mut lo = ctx();
        lo.hp = 5;
        assert!(rest_value(100, &lo, &p()) > rest_value(100, &hi, &p()));
        // Full HP ⇒ never rest.
        assert_eq!(rest_value(100, &ctx(), &p()), f64::NEG_INFINITY);
    }

    #[test]
    fn low_motivation_raises_rest_value() {
        let mut normal = ctx();
        normal.hp = 10;
        normal.motivation = 3;
        let mut grumpy = normal;
        grumpy.motivation = 1;
        assert!(
            rest_value(100, &grumpy, &p()) > rest_value(100, &normal, &p()),
            "low mood should add rest pull"
        );
    }

    #[test]
    fn low_hp_flips_suggestion_to_rest() {
        let base = scores([100, -10, -10, -10, -10]); // one clearly-best safe facility
        let fr = [5, 60, 60, 60, 60];
        let mut full = ctx();
        full.failure_rates = fr;
        full.hp = 95;
        let mut spent = full;
        spent.hp = 4;

        let adj_full = adjust_scores(&base, &full, &p());
        let adj_spent = adjust_scores(&base, &spent, &p());

        let sg_full = plan_suggestion(&adj_full, fr, false, &full, &RecommendParams::default(), &p());
        let sg_spent = plan_suggestion(&adj_spent, fr, false, &spent, &RecommendParams::default(), &p());

        assert_eq!(sg_full, TurnSuggestion::Train(0), "healthy ⇒ train the best facility");
        assert_eq!(sg_spent, TurnSuggestion::Rest, "exhausted ⇒ rest");
    }

    #[test]
    fn low_hp_prefers_race_on_race_encouraged_scenario() {
        let base = scores([100, -10, -10, -10, -10]);
        let fr = [5, 60, 60, 60, 60];
        let mut spent = ctx();
        spent.failure_rates = fr;
        spent.hp = 4;
        let adj = adjust_scores(&base, &spent, &p());
        let sg = plan_suggestion(&adj, fr, true, &spent, &RecommendParams::default(), &p());
        assert_eq!(sg, TurnSuggestion::Race);
    }

    // ---- depth knob influence on the result ----

    #[test]
    fn deeper_lookahead_moves_scores_further_from_greedy() {
        let base = scores([100, 100, 100, 100, 100]);
        let mut c = ctx();
        c.current_turn = CAREER_TURNS_TOTAL - 2; // late career so target term is live
        c.stat_deficit = [0.0, 1.0, 0.0, 0.0, 0.0];
        let dev = |d: i32| {
            let out = adjust_scores(
                &base,
                &c,
                &PlannerParams {
                    lookahead_depth: d,
                    ..p()
                },
            );
            (out[1].score - base[1].score).abs()
        };
        assert_eq!(dev(0), 0, "greedy ⇒ no deviation");
        assert!(dev(1) < dev(2), "deeper lookahead ⇒ stronger influence");
        assert!(dev(2) < dev(4));
    }
}
