//! Crash-safety read gate (hiker `read_gate` law).
//!
//! IL2CPP reads are permitted only when ALL THREE independent gates are open:
//! 1. view-transition cooldown inactive
//! 2. command-submit suspension depth == 0
//! 3. view-settle confirmation received (SetupCommandSelectStart fired after view change)
//!
//! Field names match the tent sorts EXACTLY. `read_gate` returns whether a
//! [`ReadState`] satisfies the law (implication). The real read path builds
//! `permitted` as the iff of the three gates, then consults this function.

/// Snapshot of the three crash-safety gates + the claimed permit bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadState {
    pub view_cooldown_active: i64,
    pub command_suspend_depth: i64,
    pub view_settle_pending: i64,
    pub permitted: i64,
}

/// Law check: `permitted == 1` ⇒ all three gates open.
///
/// Generated property tests assert this matches the tent oracle bit-for-bit.
#[must_use]
pub fn read_gate(s: &ReadState) -> bool {
    (!(s.permitted == 1) || s.view_cooldown_active == 0)
        && (!(s.permitted == 1) || s.command_suspend_depth == 0)
        && (!(s.permitted == 1) || s.view_settle_pending == 0)
}

/// Build a consistent [`ReadState`] from the live gate flags and decide permit.
///
/// `permitted == 1` iff all three gates are open. Always satisfies [`read_gate`].
#[must_use]
pub fn read_state_from_gates(
    view_cooldown_active: bool,
    command_suspend_depth: i64,
    view_settle_pending: bool,
) -> ReadState {
    let view_cooldown_active = i64::from(view_cooldown_active);
    let view_settle_pending = i64::from(view_settle_pending);
    let permitted = if view_cooldown_active == 0 && command_suspend_depth == 0 && view_settle_pending == 0 {
        1
    } else {
        0
    };
    ReadState {
        view_cooldown_active,
        command_suspend_depth,
        view_settle_pending,
        permitted,
    }
}

/// True when the real read path may touch IL2CPP Single Mode objects.
#[must_use]
pub fn reads_permitted(
    view_cooldown_active: bool,
    command_suspend_depth: i64,
    view_settle_pending: bool,
) -> bool {
    let s = read_state_from_gates(view_cooldown_active, command_suspend_depth, view_settle_pending);
    debug_assert!(read_gate(&s), "constructed ReadState must satisfy the law");
    s.permitted == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permitted_only_when_all_three_open() {
        // All open → permitted
        assert!(reads_permitted(false, 0, false));
        // Any single gate closed → blocked
        assert!(!reads_permitted(true, 0, false));
        assert!(!reads_permitted(false, 1, false));
        assert!(!reads_permitted(false, 0, true));
        // Two gates closed → blocked
        assert!(!reads_permitted(true, 1, false));
        assert!(!reads_permitted(true, 0, true));
        assert!(!reads_permitted(false, 1, true));
        // All closed → blocked
        assert!(!reads_permitted(true, 1, true));
    }

    #[test]
    fn law_rejects_permitted_with_closed_gate() {
        // view_cooldown_active closed, claim permitted → law rejects
        assert!(!read_gate(&ReadState {
            view_cooldown_active: 1,
            command_suspend_depth: 0,
            view_settle_pending: 0,
            permitted: 1,
        }));
        // command_suspend_depth closed, claim permitted → law rejects
        assert!(!read_gate(&ReadState {
            view_cooldown_active: 0,
            command_suspend_depth: 1,
            view_settle_pending: 0,
            permitted: 1,
        }));
        // view_settle_pending closed, claim permitted → law rejects
        assert!(!read_gate(&ReadState {
            view_cooldown_active: 0,
            command_suspend_depth: 0,
            view_settle_pending: 1,
            permitted: 1,
        }));
        // All open, permitted=1 → law accepts
        assert!(read_gate(&ReadState {
            view_cooldown_active: 0,
            command_suspend_depth: 0,
            view_settle_pending: 0,
            permitted: 1,
        }));
        // All closed, permitted=0 → law accepts (no claim)
        assert!(read_gate(&ReadState {
            view_cooldown_active: 1,
            command_suspend_depth: 1,
            view_settle_pending: 1,
            permitted: 0,
        }));
        // Two closed, permitted=0 → law accepts
        assert!(read_gate(&ReadState {
            view_cooldown_active: 1,
            command_suspend_depth: 0,
            view_settle_pending: 1,
            permitted: 0,
        }));
    }

    #[test]
    fn read_state_from_gates_always_satisfies_law() {
        // Exhaustive check over all boolean combinations
        for &vc in &[false, true] {
            for &cd in &[0i64, 1, 2] {
                for &vs in &[false, true] {
                    let s = read_state_from_gates(vc, cd, vs);
                    assert!(
                        read_gate(&s),
                        "law violated for vc={vc}, cd={cd}, vs={vs}: {s:?}"
                    );
                }
            }
        }
    }
}
