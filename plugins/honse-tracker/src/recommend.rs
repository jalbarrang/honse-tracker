//! Smart training recommendation: score each facility by its projected 評価点
//! (evaluation-point) gain this turn, failure-adjusted and clamped at the player's
//! per-stat targets/caps. Pure logic — no IL2CPP, safe on the render thread.
//!
//! Model (agreed with the user):
//! ```text
//! eval_delta = Σ_stat [ stat_score(min(cur + gain, ceiling)) − stat_score(cur) ]
//!   ceiling = effective_threshold(target, cap)   (0 ⇒ no clamp)
//! p = failure_rate / 100
//! score = eval_delta × (1 − p)                         # EV of the gains
//! if failure_rate > risk_threshold_pct:                # extra risk penalty
//!     loss = eval_cost_of_losing failure_stat_loss pts + mood_drop_penalty
//!     score −= p × loss
//! ```
//!
//! The thresholds/penalties are user-tunable via [`RecommendParams`] (surfaced in
//! the L1 settings page and persisted by `crate::config`); the scoring functions
//! take them explicitly so the logic stays pure and deterministically testable.
//!
//! Known v1 limitation: greedy per-turn. It does not value building bonds early for
//! later rainbow payoff, nor cross-turn energy management.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::build_profile::Objective;
use crate::cm_model::{
    self, Aptitudes, CourseParams, GroundCondition, StatKind, Strategy, Surface,
};
use crate::evaluation::{self, stat_score};
use crate::stat_targets;

/// Default failure % above which the extra downside penalty applies.
pub const DEFAULT_RISK_THRESHOLD_PCT: i32 = 25;
/// Default: if EVERY available facility's failure % exceeds this, training is a bad
/// turn — suggest resting (or racing on race-encouraged scenarios) instead.
pub const DEFAULT_ALL_RISKY_PCT: i32 = 30;
/// Default eval-point cost of one motivation-level drop on failure.
pub const DEFAULT_MOOD_DROP_PENALTY: i32 = 30;
/// Default stat points lost on a failed training (applied to the trained stats).
pub const DEFAULT_FAILURE_STAT_LOSS: i32 = 5;

/// User-tunable weights for the recommendation model. Persisted in
/// `training_config.json` (see `crate::config`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RecommendParams {
    /// Failure % above which a facility gets the extra downside penalty.
    #[serde(default = "default_risk_threshold")]
    pub risk_threshold_pct: i32,
    /// If every facility's failure % exceeds this, suggest Rest/Race.
    #[serde(default = "default_all_risky")]
    pub all_risky_pct: i32,
    /// Eval-point cost charged for the mood drop on a failed training.
    #[serde(default = "default_mood_penalty")]
    pub mood_drop_penalty: i32,
    /// Modeled stat points lost on a failed training.
    #[serde(default = "default_failure_stat_loss")]
    pub failure_stat_loss: i32,
}

impl RecommendParams {
    /// The built-in defaults (a `const` so it can seed a `static` store).
    pub const DEFAULT: RecommendParams = RecommendParams {
        risk_threshold_pct: DEFAULT_RISK_THRESHOLD_PCT,
        all_risky_pct: DEFAULT_ALL_RISKY_PCT,
        mood_drop_penalty: DEFAULT_MOOD_DROP_PENALTY,
        failure_stat_loss: DEFAULT_FAILURE_STAT_LOSS,
    };

    /// Clamp to sane ranges (percentages `0..=100`, penalties non-negative).
    fn clamped(self) -> Self {
        Self {
            risk_threshold_pct: self.risk_threshold_pct.clamp(0, 100),
            all_risky_pct: self.all_risky_pct.clamp(0, 100),
            mood_drop_penalty: self.mood_drop_penalty.max(0),
            failure_stat_loss: self.failure_stat_loss.max(0),
        }
    }
}

impl Default for RecommendParams {
    fn default() -> Self {
        Self::DEFAULT
    }
}

fn default_risk_threshold() -> i32 {
    DEFAULT_RISK_THRESHOLD_PCT
}
fn default_all_risky() -> i32 {
    DEFAULT_ALL_RISKY_PCT
}
fn default_mood_penalty() -> i32 {
    DEFAULT_MOOD_DROP_PENALTY
}
fn default_failure_stat_loss() -> i32 {
    DEFAULT_FAILURE_STAT_LOSS
}

/// Live, user-tunable parameters (defaults until config loads).
static PARAMS: Mutex<RecommendParams> = Mutex::new(RecommendParams::DEFAULT);

/// Current recommendation parameters.
pub fn params() -> RecommendParams {
    PARAMS.lock().map(|g| *g).unwrap_or(RecommendParams::DEFAULT)
}

/// Replace the parameters (clamped to sane ranges). Call [`crate::config::persist`]
/// to write them to disk.
pub fn set_params(p: RecommendParams) {
    if let Ok(mut g) = PARAMS.lock() {
        *g = p.clamped();
    }
}

/// Scenario training-set bases (Speed-slot command id) where racing is the better
/// fallback when all trainings are too risky. Trackblazer (Make a New Track, base
/// 1101) rewards racing; URA (101) and Unity Cup (601) do not.
const RACE_ENCOURAGED_BASES: &[i32] = &[1101];

/// Whether the active scenario (identified by its Speed-slot command base) rewards
/// racing enough to prefer it over resting when every facility is too risky.
#[must_use]
pub fn scenario_encourages_racing(scenario_command_base: i32) -> bool {
    RACE_ENCOURAGED_BASES.contains(&scenario_command_base)
}

/// The overall suggestion for the turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnSuggestion {
    /// Train the given facility slot (the best by projected score).
    Train(usize),
    /// All facilities are too risky — rest to recover energy.
    Rest,
    /// All facilities are too risky, but this scenario rewards racing.
    Race,
}

/// Decide the turn suggestion: if every facility with live data exceeds
/// `params.all_risky_pct` failure, suggest Rest (or Race when `race_encouraged`);
/// otherwise train the best-scoring facility.
#[must_use]
pub fn turn_suggestion(
    scores: &[FacilityScore; 5],
    failure_rates: [i32; 5],
    race_encouraged: bool,
    params: &RecommendParams,
) -> TurnSuggestion {
    let known: Vec<usize> = (0..5).filter(|&i| scores[i].known).collect();
    let all_risky = !known.is_empty() && known.iter().all(|&i| failure_rates[i] > params.all_risky_pct);
    if all_risky {
        return if race_encouraged {
            TurnSuggestion::Race
        } else {
            TurnSuggestion::Rest
        };
    }
    match scores.iter().position(|f| f.is_best) {
        Some(i) => TurnSuggestion::Train(i),
        None => TurnSuggestion::Rest,
    }
}

/// Per-facility recommendation result.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FacilityScore {
    /// Projected 評価点 gain this turn after failure/risk adjustment (can be negative).
    pub score: i32,
    /// `true` for the single best facility (highest score among known facilities).
    pub is_best: bool,
    /// Whether live command info was available for this facility this turn.
    pub known: bool,
}

/// Inputs for the recommendation, all per facility-slot [Speed, Stamina, Power,
/// Guts, Wisdom] (outer index = facility, matching the snapshot layout).
pub struct Inputs<'a> {
    /// Current stat values [Speed, Stamina, Power, Guts, Wisdom].
    pub current: [i32; 5],
    /// Per-facility, per-stat gain (facility × stat).
    pub per_stat_gains: &'a [[i32; 5]; 5],
    /// Per-stat caps [Speed..Wisdom]; `0` ⇒ unknown.
    pub caps: [i32; 5],
    /// Per-stat targets [Speed..Wisdom]; `0` ⇒ use cap.
    pub targets: [i32; 5],
    /// Per-facility failure %; `< 0` ⇒ unknown (treated as 0% for scoring).
    pub failure_rates: [i32; 5],
    /// Objective + CM race context (objective, weights, target course, aptitudes,
    /// strategy). Defaults to the Rank objective; the UI fills it from the active
    /// build profile + course data. Pure: the caller builds it, the scorer reads it.
    pub ctx: ScoringContext<'a>,
}

/// Normalization factor converting summed CM marginal value (uutil, from
/// [`cm_model::stat_marginal_value`]) into 評価点-equivalent points. This keeps a
/// single comparable unit across objectives so (a) the failure/risk EV model —
/// denominated in eval points — applies uniformly, and (b) CM and Rank facility
/// scores land in the same readable band (tens–low-hundreds), so switching the
/// objective does not change the magnitude wildly. CM marginal value is ~flat
/// across stats while the Rank curve is cheap-early / expensive-mid, so they can
/// only be *comparable*, not identical, at every stat. Scoring tests assert shape
/// + a magnitude band, not this exact value.
const CM_EVAL_SCALE: f64 = 1.0;

/// Stat slots in canonical order [Speed, Stamina, Power, Guts, Wit], for indexing
/// CM marginal value by a facility's per-stat gain slot.
const STAT_KINDS: [StatKind; 5] = [
    StatKind::Speed,
    StatKind::Stamina,
    StatKind::Power,
    StatKind::Guts,
    StatKind::Wit,
];

/// The objective + CM race context a scoring pass needs beyond the raw stat
/// inputs. Built by the UI from the active [`crate::build_profile`] + the target
/// course's [`CourseParams`]; the scorer only reads it (stays pure). `Copy` so it
/// rides along inside [`Inputs`] cheaply.
#[derive(Clone, Copy)]
pub struct ScoringContext<'a> {
    /// Which objective to optimize (`Off` | `Rank` | `Cm`).
    pub objective: Objective,
    /// Per-stat weights from the active profile (CM only; Rank ignores them).
    pub stat_weights: [f32; 5],
    /// Target CM course params, or `None` when no course is configured / loaded
    /// (drives graceful fallback to Rank).
    pub course: Option<&'a CourseParams>,
    /// CM model aptitude grades (distance + surface) for the target course.
    pub aptitudes: Aptitudes,
    /// Intended running style for the target CM race.
    pub strategy: Strategy,
    /// Track (baba) condition for the target CM race (affects effective
    /// speed/power, hence the stat targets). `Firm` is the neutral baseline.
    pub ground_condition: GroundCondition,
    /// Total heal (basis points of max HP) of the player's planned recovery
    /// skills; lowers the stamina the scorer expects (see `cm_model`).
    pub recovery_heal_bp: f64,
    /// Flat career-race stat bonus `[Speed, Stamina, Power, Guts, Wit]` for the
    /// active scenario (see [`cm_model::scenario_race_bonus`]). Shifts the in-race
    /// curve position so the scorer doesn't over-value Stamina mid-career. `[0; 5]`
    /// for the Rank context (no scenario).
    pub scenario_race_bonus: [i32; 5],
}

impl Default for ScoringContext<'_> {
    /// The Rank-objective context: preserves shipped behaviour (no course, neutral
    /// weights). Not derivable — neutral weights are `1.0`, not `0.0`.
    fn default() -> Self {
        Self {
            objective: Objective::Rank,
            stat_weights: [1.0; 5],
            course: None,
            aptitudes: Aptitudes::default(),
            strategy: Strategy::LateSurger,
            ground_condition: GroundCondition::Firm,
            recovery_heal_bp: 0.0,
            scenario_race_bonus: [0; 5],
        }
    }
}

/// The objective actually used for scoring, after graceful degradation: a CM
/// objective falls back to Rank when no target-course params are available (so
/// the scorer never needs course data it does not have). `Off` is preserved (no
/// fallback) — it means "show nothing".
#[must_use]
pub fn effective_objective(ctx: &ScoringContext) -> Objective {
    match ctx.objective {
        Objective::Off => Objective::Off,
        Objective::Cm if ctx.course.is_none() => Objective::Rank,
        other => other,
    }
}

/// Whether the configured objective wanted CM weight but had to fall back to Rank
/// (no course params). Surfaced so the UI can flag the degraded state.
// Consumed by the objective selector / course picker UI (cm-ui-config-docs);
// remove the allow once that wires it into the footer caption.
#[allow(dead_code)]
#[must_use]
pub fn cm_fallback_active(ctx: &ScoringContext) -> bool {
    ctx.objective.cm_weight() > 0.0 && ctx.course.is_none()
}

/// Derive the CM model's distance + surface aptitude grades for a specific course
/// from the trainee's full aptitude set. The UI uses this when building the
/// [`ScoringContext`]. Distance bucket follows the game's grouping
/// (≤1400 sprint, ≤1800 mile, ≤2400 medium, else long).
#[must_use]
pub fn cm_aptitudes_for_course(apt: &evaluation::Aptitudes, course: &CourseParams) -> Aptitudes {
    let surface_grade = match course.surface {
        Surface::Turf => apt.ground_turf,
        Surface::Dirt => apt.ground_dirt,
    };
    let distance_grade = if course.distance <= 1400.0 {
        apt.dist_short
    } else if course.distance <= 1800.0 {
        apt.dist_mile
    } else if course.distance <= 2400.0 {
        apt.dist_middle
    } else {
        apt.dist_long
    };
    Aptitudes {
        distance_grade,
        surface_grade,
    }
}

/// Score all five facilities and flag the best. Facilities with no live gain data
/// (all-zero row) are marked `known: false` and excluded from the best pick.
#[must_use]
pub fn score_facilities(input: &Inputs, params: &RecommendParams) -> [FacilityScore; 5] {
    let mut out = [FacilityScore::default(); 5];
    let mut best: Option<(usize, i32)> = None;

    for (i, slot) in out.iter_mut().enumerate() {
        let gains = input.per_stat_gains[i];
        let known = gains.iter().any(|&g| g != 0);
        let fail = input.failure_rates[i].max(0);
        let score = facility_score(
            input.current,
            gains,
            input.caps,
            input.targets,
            fail,
            &input.ctx,
            params,
        );
        *slot = FacilityScore {
            score,
            is_best: false,
            known,
        };
        if known && best.is_none_or(|(_, bs)| score > bs) {
            best = Some((i, score));
        }
    }

    if let Some((b, _)) = best {
        out[b].is_best = true;
    }
    out
}

/// Score a single facility: failure-adjusted projected objective delta (in 評価点-
/// equivalent units, so the EV/risk model is objective-agnostic).
fn facility_score(
    current: [i32; 5],
    gains: [i32; 5],
    caps: [i32; 5],
    targets: [i32; 5],
    fail_pct: i32,
    ctx: &ScoringContext,
    params: &RecommendParams,
) -> i32 {
    let delta = objective_delta(current, gains, caps, targets, ctx);
    let p = fail_pct as f64 / 100.0;
    let mut score = delta * (1.0 - p);

    if fail_pct > params.risk_threshold_pct {
        score -= p * failure_loss(current, gains, params) as f64;
    }
    score.round() as i32
}

/// Objective-aware projected value of a facility's gains this turn, in 評価点-
/// equivalent units (see [`CM_EVAL_SCALE`]).
///
/// - `Rank` → the validated projected 評価点 delta ([`projected_eval_delta`]).
/// - `Cm` → Σ [`cm_model::stat_marginal_value`] over the facility's per-stat gains
///   (capped at the manual target/cap ceiling), weighted by `stat_weights`, scaled
///   into eval-equivalent units.
/// - `Off` → `0` (the UI hides scores under this objective, so the value is
///   immaterial; we never claim one facility beats another).
///
/// Falls back to Rank when CM is requested but no course is available
/// ([`effective_objective`]).
fn objective_delta(current: [i32; 5], gains: [i32; 5], caps: [i32; 5], targets: [i32; 5], ctx: &ScoringContext) -> f64 {
    match effective_objective(ctx) {
        Objective::Off => 0.0,
        Objective::Rank => projected_eval_delta(current, gains, caps, targets) as f64,
        Objective::Cm => cm_eval_delta(current, gains, caps, targets, ctx),
    }
}

/// CM marginal-value delta for a facility, in eval-equivalent units. Sums each
/// trained stat's [`cm_model::stat_marginal_value`] × useful-gain × weight, where
/// useful gain is clamped at the manual target/cap ceiling (gains past it earn
/// nothing, mirroring the Rank path). Only called when `ctx.course` is `Some`
/// (guaranteed by [`effective_objective`]).
fn cm_eval_delta(current: [i32; 5], gains: [i32; 5], caps: [i32; 5], targets: [i32; 5], ctx: &ScoringContext) -> f64 {
    let course = ctx
        .course
        .expect("cm_eval_delta requires course params (guarded by effective_objective)");
    let mut total = 0.0;
    for s in 0..5 {
        if gains[s] == 0 {
            continue;
        }
        // Manual target/cap still caps useful gains (no value past the ceiling).
        let ceiling = stat_targets::effective_threshold(targets[s], caps[s]);
        let raised = current[s] + gains[s];
        let capped = if ceiling > 0 { raised.min(ceiling) } else { raised };
        let useful = (capped - current[s]).max(0);
        if useful == 0 {
            continue;
        }
        let mv = cm_model::stat_marginal_value(
            STAT_KINDS[s],
            current,
            course,
            ctx.strategy,
            ctx.aptitudes,
            ctx.ground_condition,
            ctx.recovery_heal_bp,
            ctx.scenario_race_bonus,
        );
        total += mv * useful as f64 * ctx.stat_weights[s] as f64;
    }
    total * CM_EVAL_SCALE
}

/// Projected 評価点 gain from a facility's per-stat gains, clamping useful gains at
/// `min(target, cap)` (gains past the ceiling earn no evaluation).
fn projected_eval_delta(current: [i32; 5], gains: [i32; 5], caps: [i32; 5], targets: [i32; 5]) -> i32 {
    let mut delta = 0;
    for s in 0..5 {
        if gains[s] == 0 {
            continue;
        }
        let ceiling = stat_targets::effective_threshold(targets[s], caps[s]);
        let raised = current[s] + gains[s];
        let capped = if ceiling > 0 { raised.min(ceiling) } else { raised };
        delta += stat_score(capped) - stat_score(current[s]);
    }
    delta
}

/// Eval-point cost of a failed training: losing `params.failure_stat_loss` on each
/// stat the facility would have raised, plus the mood-drop penalty.
fn failure_loss(current: [i32; 5], gains: [i32; 5], params: &RecommendParams) -> i32 {
    let mut loss = params.mood_drop_penalty;
    for s in 0..5 {
        if gains[s] == 0 {
            continue;
        }
        let dropped = (current[s] - params.failure_stat_loss).max(0);
        loss += stat_score(current[s]) - stat_score(dropped);
    }
    loss
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Default params for tests (the scoring logic is independent of the live store).
    fn p() -> RecommendParams {
        RecommendParams::default()
    }

    fn one_facility(slot: usize, gains: [i32; 5]) -> [[i32; 5]; 5] {
        let mut m = [[0i32; 5]; 5];
        m[slot] = gains;
        m
    }

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

    /// A CM-objective context aimed at `course` with the given weights (S/A apts).
    fn cm_ctx(course: &CourseParams, weights: [f32; 5]) -> ScoringContext<'_> {
        ScoringContext {
            objective: Objective::Cm,
            stat_weights: weights,
            course: Some(course),
            aptitudes: Aptitudes {
                distance_grade: 7,
                surface_grade: 7,
            },
            strategy: Strategy::LateSurger,
            ground_condition: GroundCondition::Firm,
            recovery_heal_bp: 0.0,
            scenario_race_bonus: [0; 5],
        }
    }

    #[test]
    fn unknown_facilities_are_not_best() {
        let input = Inputs {
            current: [100; 5],
            per_stat_gains: &[[0i32; 5]; 5],
            caps: [0; 5],
            targets: [0; 5],
            failure_rates: [-1; 5],
            ctx: ScoringContext::default(),
        };
        let out = score_facilities(&input, &p());
        assert!(out.iter().all(|f| !f.known));
        assert!(out.iter().all(|f| !f.is_best));
    }

    #[test]
    fn best_is_highest_score() {
        // Speed facility gains 20 on Speed; Guts facility gains 20 on Guts but at 40% fail.
        let mut gains = [[0i32; 5]; 5];
        gains[0] = [20, 0, 0, 0, 0];
        gains[3] = [0, 0, 0, 20, 0];
        let input = Inputs {
            current: [300; 5],
            per_stat_gains: &gains,
            caps: [0; 5],
            targets: [0; 5],
            failure_rates: [0, -1, -1, 40, -1],
            ctx: ScoringContext::default(),
        };
        let out = score_facilities(&input, &p());
        assert!(out[0].is_best, "safe Speed should beat risky Guts");
        assert!(out[0].score > out[3].score);
        assert!(out[3].known);
    }

    #[test]
    fn higher_failure_lowers_score() {
        let gains = one_facility(0, [20, 0, 0, 0, 0]);
        let mk = |fail: i32| {
            let input = Inputs {
                current: [300; 5],
                per_stat_gains: &gains,
                caps: [0; 5],
                targets: [0; 5],
                failure_rates: [fail, -1, -1, -1, -1],
                ctx: ScoringContext::default(),
            };
            score_facilities(&input, &p())[0].score
        };
        assert!(mk(0) > mk(20));
        assert!(mk(20) > mk(50)); // risk penalty kicks in above 25%
    }

    #[test]
    fn gains_past_ceiling_earn_nothing() {
        // Stat at its target already → projected delta is ~0 regardless of gain.
        let gains = one_facility(0, [50, 0, 0, 0, 0]);
        let at_target = Inputs {
            current: [1200, 0, 0, 0, 0],
            per_stat_gains: &gains,
            caps: [1200, 0, 0, 0, 0],
            targets: [1200, 0, 0, 0, 0],
            failure_rates: [0, -1, -1, -1, -1],
            ctx: ScoringContext::default(),
        };
        assert_eq!(score_facilities(&at_target, &p())[0].score, 0);

        // Same gain with headroom scores positive.
        let with_room = Inputs {
            current: [800, 0, 0, 0, 0],
            per_stat_gains: &gains,
            caps: [1200, 0, 0, 0, 0],
            targets: [0, 0, 0, 0, 0],
            failure_rates: [0, -1, -1, -1, -1],
            ctx: ScoringContext::default(),
        };
        assert!(score_facilities(&with_room, &p())[0].score > 0);
    }

    #[test]
    fn suggest_rest_when_all_risky() {
        let mut gains = [[0i32; 5]; 5];
        for (i, g) in gains.iter_mut().enumerate() {
            g[i] = 10; // every facility raises one stat
        }
        let input = Inputs {
            current: [300; 5],
            per_stat_gains: &gains,
            caps: [0; 5],
            targets: [0; 5],
            failure_rates: [35, 40, 31, 50, 33], // all > 30%
            ctx: ScoringContext::default(),
        };
        let scores = score_facilities(&input, &p());
        assert_eq!(
            turn_suggestion(&scores, input.failure_rates, false, &p()),
            TurnSuggestion::Rest
        );
        assert_eq!(
            turn_suggestion(&scores, input.failure_rates, true, &p()),
            TurnSuggestion::Race
        );
    }

    #[test]
    fn suggest_train_when_one_is_safe() {
        let mut gains = [[0i32; 5]; 5];
        for (i, g) in gains.iter_mut().enumerate() {
            g[i] = 10;
        }
        let input = Inputs {
            current: [300; 5],
            per_stat_gains: &gains,
            caps: [0; 5],
            targets: [0; 5],
            failure_rates: [35, 40, 5, 50, 33], // Power is safe
            ctx: ScoringContext::default(),
        };
        let scores = score_facilities(&input, &p());
        assert_eq!(
            turn_suggestion(&scores, input.failure_rates, false, &p()),
            TurnSuggestion::Train(2)
        );
    }

    #[test]
    fn no_data_suggests_rest() {
        let input = Inputs {
            current: [300; 5],
            per_stat_gains: &[[0i32; 5]; 5],
            caps: [0; 5],
            targets: [0; 5],
            failure_rates: [-1; 5],
            ctx: ScoringContext::default(),
        };
        let scores = score_facilities(&input, &p());
        assert_eq!(
            turn_suggestion(&scores, input.failure_rates, false, &p()),
            TurnSuggestion::Rest
        );
    }

    #[test]
    fn target_clamps_before_cap() {
        // Target 900 below cap 1200: gain past 900 is wasted even though under cap.
        let gains = one_facility(0, [40, 0, 0, 0, 0]);
        let capped_at_target = projected_eval_delta([880, 0, 0, 0, 0], gains[0], [1200, 0, 0, 0, 0], [900, 0, 0, 0, 0]);
        let capped_at_cap = projected_eval_delta([880, 0, 0, 0, 0], gains[0], [1200, 0, 0, 0, 0], [0, 0, 0, 0, 0]);
        assert!(capped_at_target < capped_at_cap);
    }

    #[test]
    fn params_clamp_to_sane_ranges() {
        set_params(RecommendParams {
            risk_threshold_pct: 250,
            all_risky_pct: -5,
            mood_drop_penalty: -100,
            failure_stat_loss: -1,
        });
        let got = params();
        assert_eq!(got.risk_threshold_pct, 100);
        assert_eq!(got.all_risky_pct, 0);
        assert_eq!(got.mood_drop_penalty, 0);
        assert_eq!(got.failure_stat_loss, 0);
        set_params(RecommendParams::default());
    }

    // ---- CM-objective shape ----

    #[test]
    fn cm_stamina_dominates_below_survival_floor() {
        let c = course(2400.0, Surface::Turf, vec![]);
        let ctx = cm_ctx(&c, [1.0; 5]);
        let floor =
            cm_model::stamina_survival_threshold(&c, Strategy::LateSurger, 400.0, 1100.0, 7, GroundCondition::Firm);
        let low = (floor - 250.0).max(50.0) as i32;
        let high = (floor + 350.0) as i32;
        let below = cm_eval_delta([1100, low, 800, 400, 600], [0, 20, 0, 0, 0], [0; 5], [0; 5], &ctx);
        let above = cm_eval_delta([1100, high, 800, 400, 600], [0, 20, 0, 0, 0], [0; 5], [0; 5], &ctx);
        assert!(
            below > above * 3.0,
            "stamina must dominate below the survival floor ({below} vs {above})"
        );
    }

    #[test]
    fn cm_speed_value_drops_past_softcap() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let ctx = cm_ctx(&c, [1.0; 5]);
        let below = cm_eval_delta([900, 600, 600, 400, 600], [20, 0, 0, 0, 0], [0; 5], [0; 5], &ctx);
        let above = cm_eval_delta([1300, 600, 600, 400, 600], [20, 0, 0, 0, 0], [0; 5], [0; 5], &ctx);
        assert!(
            below > above,
            "speed value should fall past the 1200 soft cap ({below} vs {above})"
        );
    }

    #[test]
    fn cm_power_value_ramps_down_past_knee() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let ctx = cm_ctx(&c, [1.0; 5]);
        let low = cm_eval_delta([1100, 700, 500, 400, 600], [0, 0, 20, 0, 0], [0; 5], [0; 5], &ctx);
        let high = cm_eval_delta([1100, 700, 1150, 400, 600], [0, 0, 20, 0, 0], [0; 5], [0; 5], &ctx);
        assert!(low > high, "power value should taper past the knee ({low} vs {high})");
    }

    #[test]
    fn cm_weights_scale_contribution_linearly() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let cur = [900, 600, 600, 400, 600];
        let full = cm_eval_delta(cur, [20, 0, 0, 0, 0], [0; 5], [0; 5], &cm_ctx(&c, [1.0; 5]));
        let half = cm_eval_delta(
            cur,
            [20, 0, 0, 0, 0],
            [0; 5],
            [0; 5],
            &cm_ctx(&c, [0.5, 1.0, 1.0, 1.0, 1.0]),
        );
        assert!(full > 0.0);
        assert!((full * 0.5 - half).abs() < 1e-9, "weight should scale the contribution");
    }

    #[test]
    fn cm_gains_past_manual_target_earn_nothing() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let ctx = cm_ctx(&c, [1.0; 5]);
        // Speed already at the manual target 1200 → no useful gain, zero CM value.
        let at = cm_eval_delta(
            [1200, 600, 600, 400, 600],
            [50, 0, 0, 0, 0],
            [1200, 0, 0, 0, 0],
            [1200, 0, 0, 0, 0],
            &ctx,
        );
        assert_eq!(at, 0.0);
    }

    #[test]
    fn off_objective_scores_zero() {
        let c = course(2000.0, Surface::Turf, vec![]);
        let cur = [800, 600, 600, 400, 600];
        let gains = [20, 0, 0, 0, 0];
        let base = cm_ctx(&c, [1.0; 5]);
        // CM scores something positive...
        let cm = objective_delta(cur, gains, [0; 5], [0; 5], &base);
        assert!(cm > 0.0);
        // ...but Off always scores zero (scoring is hidden under this objective).
        let off = objective_delta(
            cur,
            gains,
            [0; 5],
            [0; 5],
            &ScoringContext {
                objective: Objective::Off,
                ..base
            },
        );
        assert_eq!(off, 0.0);
        assert_eq!(
            effective_objective(&ScoringContext {
                objective: Objective::Off,
                ..base
            }),
            Objective::Off
        );
    }

    #[test]
    fn cm_without_course_falls_back_to_rank() {
        let cur = [800, 600, 600, 400, 600];
        let gains = [20, 0, 0, 0, 0];
        let ctx = ScoringContext {
            objective: Objective::Cm,
            course: None,
            ..ScoringContext::default()
        };
        assert_eq!(effective_objective(&ctx), Objective::Rank);
        assert!(cm_fallback_active(&ctx));
        let delta = objective_delta(cur, gains, [0; 5], [0; 5], &ctx);
        let rank = projected_eval_delta(cur, gains, [0; 5], [0; 5]) as f64;
        assert_eq!(delta, rank, "missing course must degrade CM to the Rank delta");
    }

    #[test]
    fn cm_aptitudes_pick_grades_by_course() {
        let apt = evaluation::Aptitudes {
            ground_turf: 7,
            ground_dirt: 3,
            dist_short: 6,
            dist_mile: 5,
            dist_middle: 8,
            dist_long: 4,
            ..Default::default()
        };
        let mile = course(1600.0, Surface::Turf, vec![]);
        let a = cm_aptitudes_for_course(&apt, &mile);
        assert_eq!(a.distance_grade, 5, "1600m → mile grade");
        assert_eq!(a.surface_grade, 7, "turf grade");
        let dirt_long = course(3000.0, Surface::Dirt, vec![]);
        let b = cm_aptitudes_for_course(&apt, &dirt_long);
        assert_eq!(b.distance_grade, 4, "3000m → long grade");
        assert_eq!(b.surface_grade, 3, "dirt grade");
    }

    #[test]
    fn cm_speed_facility_is_comparable_to_rank() {
        // After recalibration, a Speed-only facility under CM should land in the
        // same ballpark as the Rank objective (objectives are interchangeable),
        // not orders of magnitude apart.
        let c = course(2000.0, Surface::Turf, vec![]);
        let cur = [800, 600, 600, 400, 600];
        let gains = [18, 0, 0, 0, 0];
        let base = cm_ctx(&c, [1.0; 5]);
        let cm = objective_delta(cur, gains, [0; 5], [0; 5], &base);
        let rank = objective_delta(
            cur,
            gains,
            [0; 5],
            [0; 5],
            &ScoringContext {
                objective: Objective::Rank,
                ..base
            },
        );
        assert!(cm > 0.0 && rank > 0.0);
        let ratio = cm / rank;
        assert!(
            (0.3..=3.0).contains(&ratio),
            "CM and Rank should be comparable in magnitude (cm {cm}, rank {rank}, ratio {ratio})"
        );
    }
}
