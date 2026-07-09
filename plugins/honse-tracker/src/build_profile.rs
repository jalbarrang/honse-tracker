//! Build-target **profiles**: the desired end-state stat shape + objective the
//! recommender aims for. Sourced from curated **community presets** and a
//! **manual editor**. This replaces the old flat per-stat targets as the single
//! source of truth (`crate::stat_targets` is now a thin façade over the active
//! profile's `per_stat_target`).
//!
//! - The closed-form [`crate::cm_model`] supplies *threshold-aware marginal
//!   value* (survival floor, 1200 soft-cap, power knee). A profile says *what to
//!   aim at* (objective, targets, weights, course/strategy); the model says *how
//!   much each point is worth getting there*.
//! - Presets encode veteran wisdom per distance/strategy (uma.guide / gametora
//!   meta). Manual edits let power users override any field.
//!
//! In-memory state only; persistence lives in [`crate::config`]. Every persisted
//! field is `#[serde(default)]` so older configs migrate cleanly.

use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

use crate::cm_model::{GroundCondition, Strategy};
use crate::race_context::{Season, TimeOfDay, Weather};

/// Stat order: [Speed, Stamina, Power, Guts, Wit].
pub const STAT_LABELS: [&str; 5] = ["Speed", "Stamina", "Power", "Guts", "Wit"];

/// Upper bound for a per-stat target (matches the highest reachable stat cap).
pub const MAX_TARGET: i32 = 2000;

/// Which scoring objective the recommender optimizes for.
///
/// - `Off` — the scoring + recommendation system is hidden entirely (the tab
///   still tracks stats/gains/failure, just no scores or turn suggestion).
/// - `Rank` — the validated 評価点 (career-rank) model (default).
/// - `Cm` — Champions Meeting race-utility (threshold-aware, via `cm_model`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Objective {
    /// Recommendations off — tracking only.
    Off,
    /// Preserve the shipped behaviour until the user opts into CM.
    #[default]
    Rank,
    Cm,
}

impl Objective {
    /// CM weight (`Cm` ⇒ 1, everything else ⇒ 0). Used only to flag the
    /// "CM wanted but no course" degraded state.
    pub fn cm_weight(self) -> f32 {
        match self {
            Objective::Cm => 1.0,
            Objective::Off | Objective::Rank => 0.0,
        }
    }
}

/// A complete build target: objective + the stat shape and race context to aim
/// at. Switching profiles switches all of these together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildProfile {
    /// Human-readable label (preset name or a user's custom name).
    pub name: String,
    /// Scoring objective.
    #[serde(default)]
    pub objective: Objective,
    /// Per-stat targets [Speed, Stamina, Power, Guts, Wit]; `0` ⇒ use the live
    /// game cap (keeps the old `stat_targets` semantics).
    #[serde(default)]
    pub per_stat_target: [i32; 5],
    /// Per-stat scoring weights (secondary tuning on top of the marginal model).
    #[serde(default = "default_weights")]
    pub stat_weights: [f32; 5],
    /// Intended running style for the target CM race (drives HP conversion etc.).
    #[serde(default = "default_strategy")]
    pub strategy: Strategy,
    /// Target CM course id (key into `crate::course_data`); `0` ⇒ none chosen.
    #[serde(default)]
    pub target_course_id: i32,
    /// Track (baba) condition for the target CM race; `Firm` is the neutral
    /// baseline (no penalty). Soft/Heavy/dirt raise the speed/power targets.
    #[serde(default)]
    pub ground_condition: GroundCondition,
    /// Announced weather (context only — does not affect scoring).
    #[serde(default)]
    pub weather: Weather,
    /// Announced season (context only — does not affect scoring).
    #[serde(default)]
    pub season: Season,
    /// Announced time of day (context only — does not affect scoring).
    #[serde(default)]
    pub time_of_day: TimeOfDay,
    /// Stamina rush-buffer override in points; `0` ⇒ auto (distance-derived).
    #[serde(default)]
    pub rush_buffer: i32,
    /// Recovery skill ids the player plans to run (gametora skill ids). Their
    /// total heal lowers the stamina the recommender expects you to train.
    #[serde(default)]
    pub recovery_skill_ids: Vec<i64>,
    /// Free-form notes (preset provenance / user reminders).
    #[serde(default)]
    pub notes: String,
}

fn default_weights() -> [f32; 5] {
    [1.0; 5]
}

fn default_strategy() -> Strategy {
    Strategy::LateSurger
}

impl Default for BuildProfile {
    fn default() -> Self {
        // The default profile preserves shipped behaviour: Rank objective, no
        // targets (fall back to game caps), neutral weights.
        Self {
            name: "Default".to_owned(),
            objective: Objective::Rank,
            per_stat_target: [0; 5],
            stat_weights: default_weights(),
            strategy: default_strategy(),
            target_course_id: 0,
            ground_condition: GroundCondition::default(),
            weather: Weather::default(),
            season: Season::default(),
            time_of_day: TimeOfDay::default(),
            rush_buffer: 0,
            recovery_skill_ids: Vec::new(),
            notes: String::new(),
        }
    }
}

impl BuildProfile {
    /// Clamp every field to a sane range (targets `0..=MAX_TARGET`, weights
    /// `0..=5`, rush buffer `0..=600`).
    pub fn clamped(mut self) -> Self {
        for t in &mut self.per_stat_target {
            *t = (*t).clamp(0, MAX_TARGET);
        }
        for w in &mut self.stat_weights {
            *w = w.clamp(0.0, 5.0);
        }
        self.rush_buffer = self.rush_buffer.clamp(0, 600);
        self
    }
}

// ---------------------------------------------------------------------------
// Live state (active profile)
// ---------------------------------------------------------------------------

#[derive(Default)]
struct ProfileState {
    active: BuildProfile,
}

fn state() -> &'static Mutex<ProfileState> {
    static S: OnceLock<Mutex<ProfileState>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(ProfileState::default()))
}

/// The active build profile (cloned).
pub fn active() -> BuildProfile {
    state().lock().map(|s| s.active.clone()).unwrap_or_default()
}

/// Replace the active profile (clamped to sane ranges).
pub fn set_active(profile: BuildProfile) {
    if let Ok(mut s) = state().lock() {
        s.active = profile.clamped();
    }
}

/// The active objective.
pub fn objective() -> Objective {
    active().objective
}

/// The active profile's per-stat targets (`0` ⇒ use game cap).
pub fn per_stat_target() -> [i32; 5] {
    active().per_stat_target
}

/// Set the active profile's per-stat targets (clamped to `0..=MAX_TARGET`).
pub fn set_per_stat_target(targets: [i32; 5]) {
    if let Ok(mut s) = state().lock() {
        for (slot, v) in s.active.per_stat_target.iter_mut().zip(targets) {
            *slot = v.clamp(0, MAX_TARGET);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objective_cm_weight() {
        assert_eq!(Objective::Off.cm_weight(), 0.0);
        assert_eq!(Objective::Rank.cm_weight(), 0.0);
        assert_eq!(Objective::Cm.cm_weight(), 1.0);
    }

    #[test]
    fn default_profile_preserves_rank_behaviour() {
        let p = BuildProfile::default();
        assert_eq!(p.objective, Objective::Rank);
        assert_eq!(p.per_stat_target, [0; 5]);
        assert_eq!(p.stat_weights, [1.0; 5]);
    }

    #[test]
    fn clamped_bounds_every_field() {
        let p = BuildProfile {
            per_stat_target: [5000, -10, 1200, 0, 600],
            stat_weights: [9.0, -1.0, 1.0, 1.0, 1.0],
            objective: Objective::Cm,
            rush_buffer: 9000,
            ..Default::default()
        }
        .clamped();
        assert_eq!(p.per_stat_target[0], MAX_TARGET); // clamped down
        assert_eq!(p.per_stat_target[1], 0); // clamped up from negative
        assert_eq!(p.stat_weights[0], 5.0);
        assert_eq!(p.stat_weights[1], 0.0);
        assert_eq!(p.objective, Objective::Cm);
        assert_eq!(p.rush_buffer, 600);
    }

    #[test]
    fn active_state_round_trips() {
        let p = BuildProfile {
            name: "Test".to_owned(),
            objective: Objective::Cm,
            per_stat_target: [1200, 800, 900, 300, 1000],
            ..Default::default()
        };
        set_active(p);
        assert_eq!(active().objective, Objective::Cm);
        assert_eq!(per_stat_target(), [1200, 800, 900, 300, 1000]);

        set_per_stat_target([1100, 700, 850, 250, 950]);
        assert_eq!(per_stat_target(), [1100, 700, 850, 250, 950]);

        // Reset for other tests sharing global state.
        set_active(BuildProfile::default());
    }
}
