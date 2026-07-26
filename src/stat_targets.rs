//! Per-stat training targets for the cap/target warning.
//!
//! A target of `0` means "no target" — fall back to the live game cap. A positive
//! target gives an earlier warning (e.g. stop Stamina at 600 even though the cap
//! is higher).
//!
//! This module is now a thin **façade** over the active build profile
//! ([`crate::build_profile`]), which owns the per-stat targets so they switch
//! together with the objective/weights when the user changes profiles. The
//! public API is unchanged so existing call sites (the scorer, the overlay, the
//! settings editor) keep working; persistence is owned by [`crate::config`].

use crate::build_profile;

/// Current per-stat targets (`0` = use game cap) — the active profile's targets.
/// Stat order: [Speed, Stamina, Power, Guts, Wit].
pub fn targets() -> [i32; 5] {
    build_profile::per_stat_target()
}

/// Effective warning threshold for a stat: the target when set, else the game cap.
pub fn effective_threshold(target: i32, cap: i32) -> i32 {
    if target > 0 {
        target
    } else {
        cap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_threshold_prefers_target() {
        assert_eq!(effective_threshold(600, 1200), 600); // target set
        assert_eq!(effective_threshold(0, 1200), 1200); // unset → cap
        assert_eq!(effective_threshold(0, 0), 0); // both unknown
        assert_eq!(effective_threshold(900, 0), 900); // target even with unknown cap
    }

    #[test]
    fn targets_reflect_active_profile() {
        build_profile::set_per_stat_target([1100, 700, 850, 250, 950]);
        assert_eq!(targets(), [1100, 700, 850, 250, 950]);
        build_profile::set_per_stat_target([0; 5]); // reset for other tests
    }
}
