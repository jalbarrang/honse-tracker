//! Crash-safety read gate (hiker `read_gate` law).
//!
//! IL2CPP reads are permitted only when BOTH independent gates are open:
//! 1. view-transition cooldown inactive
//! 2. command-submit suspension depth == 0
//!
//! Field names match the tent sorts EXACTLY. `read_gate` returns whether a
//! [`ReadState`] satisfies the law (implication). The real read path builds
//! `permitted` as the iff of the two gates, then consults this function.

/// Snapshot of the two crash-safety gates + the claimed permit bit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadState {
    pub view_cooldown_active: i64,
    pub command_suspend_depth: i64,
    pub permitted: i64,
}

/// Law check: `permitted == 1` ⇒ both gates open.
///
/// Generated property tests assert this matches the tent oracle bit-for-bit.
#[must_use]
pub fn read_gate(s: &ReadState) -> bool {
    (!(s.permitted == 1) || s.view_cooldown_active == 0) && (!(s.permitted == 1) || s.command_suspend_depth == 0)
}

/// Build a consistent [`ReadState`] from the live gate flags and decide permit.
///
/// `permitted == 1` iff both gates are open. Always satisfies [`read_gate`].
#[must_use]
pub fn read_state_from_gates(view_cooldown_active: bool, command_suspend_depth: i64) -> ReadState {
    let view_cooldown_active = i64::from(view_cooldown_active);
    let permitted = if view_cooldown_active == 0 && command_suspend_depth == 0 {
        1
    } else {
        0
    };
    ReadState {
        view_cooldown_active,
        command_suspend_depth,
        permitted,
    }
}

/// True when the real read path may touch IL2CPP Single Mode objects.
#[must_use]
pub fn reads_permitted(view_cooldown_active: bool, command_suspend_depth: i64) -> bool {
    let s = read_state_from_gates(view_cooldown_active, command_suspend_depth);
    debug_assert!(read_gate(&s), "constructed ReadState must satisfy the law");
    s.permitted == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permitted_only_when_both_open() {
        assert!(reads_permitted(false, 0));
        assert!(!reads_permitted(true, 0));
        assert!(!reads_permitted(false, 1));
        assert!(!reads_permitted(true, 1));
    }

    #[test]
    fn law_rejects_permitted_with_closed_gate() {
        assert!(!read_gate(&ReadState {
            view_cooldown_active: 1,
            command_suspend_depth: 0,
            permitted: 1,
        }));
        assert!(!read_gate(&ReadState {
            view_cooldown_active: 0,
            command_suspend_depth: 1,
            permitted: 1,
        }));
        assert!(read_gate(&ReadState {
            view_cooldown_active: 0,
            command_suspend_depth: 0,
            permitted: 1,
        }));
        assert!(read_gate(&ReadState {
            view_cooldown_active: 1,
            command_suspend_depth: 1,
            permitted: 0,
        }));
    }
}
