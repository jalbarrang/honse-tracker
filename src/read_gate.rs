//! Event-driven career lifecycle and crash-safety read law.
//!
//! IL2CPP career reads are permitted in exactly one lifecycle state:
//! [`CareerState::CommandSelectActive`]. Every transition is driven by a game
//! hook; there is no timer or independent boolean gate that can reopen reads.

/// Career lifecycle states relevant to IL2CPP read safety.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum CareerState {
    /// No active career view is known. Reads fail closed.
    #[default]
    Idle = 0,
    /// Start/load data arrived, but the career view has not completed play-in.
    CareerLoading = 1,
    /// The command-select UI is rebuilt and the player can act.
    CommandSelectActive = 2,
    /// A command was submitted and its coroutine is running.
    CommandInFlight = 3,
    /// Fresh data may exist, but assets/UI are still being replaced.
    AssetTransition = 4,
    /// Paddock or race flow is active.
    RaceActive = 5,
    /// Story, concert, or another career cutscene is active.
    CutsceneActive = 6,
}

impl CareerState {
    /// Decode the value stored in the runtime atomic. Unknown values fail closed.
    #[must_use]
    pub const fn from_u8(value: u8) -> Self {
        match value {
            1 => Self::CareerLoading,
            2 => Self::CommandSelectActive,
            3 => Self::CommandInFlight,
            4 => Self::AssetTransition,
            5 => Self::RaceActive,
            6 => Self::CutsceneActive,
            _ => Self::Idle,
        }
    }

    #[must_use]
    pub const fn as_i64(self) -> i64 {
        self as u8 as i64
    }
}

/// WorkSingleModeData response categories. Apply events only report data
/// freshness and always lead to an unsafe state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyEvent {
    ExecCommand,
    RaceEntry,
    RaceEnd,
    RaceOut,
    CheckEvent,
    Continue,
    CareerStart,
    CareerLoad,
}

/// Semantic classification of `SceneManager.GetCurrentViewId()` observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewKind {
    Career,
    CareerIntermission,
    Paddock,
    Race,
    Concert,
    Cutscene,
    OutsideCareer,
}

/// Inputs accepted by the pure lifecycle reducer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CareerEvent {
    CommandSubmitted,
    CommandSelectCompleted,
    CommandViewPlayInCompleted,
    Applied(ApplyEvent),
    ViewChanged(ViewKind),
    Reset,
}

/// Classify observed game view IDs without treating any ID as a settle proof.
#[must_use]
pub const fn classify_view(view_id: i32) -> ViewKind {
    match view_id {
        1101 => ViewKind::Career,
        1100 => ViewKind::CareerIntermission,
        1200 => ViewKind::Paddock,
        // 400 is in the local scene catalog; 1400 is observed in current race flows.
        400 | 1400 => ViewKind::Race,
        1620 | 1621 => ViewKind::Concert,
        // Boot/start/home are known career-exit destinations.
        1 | 2 | 101 => ViewKind::OutsideCareer,
        // Unknown views reached from a career fail closed as story/cutscene flow.
        _ => ViewKind::Cutscene,
    }
}

/// Pure career lifecycle reducer.
///
/// The only events that can enter [`CareerState::CommandSelectActive`] are
/// post-original UI completion hooks. Apply and view events can only preserve
/// that state for a delayed observation of view 1101 or move to an unsafe state.
#[must_use]
pub const fn transition(state: CareerState, event: CareerEvent) -> CareerState {
    match event {
        CareerEvent::Reset => CareerState::Idle,
        CareerEvent::CommandSubmitted => CareerState::CommandInFlight,
        CareerEvent::CommandSelectCompleted | CareerEvent::CommandViewPlayInCompleted => {
            CareerState::CommandSelectActive
        }
        CareerEvent::Applied(ApplyEvent::CareerStart | ApplyEvent::CareerLoad) => CareerState::CareerLoading,
        CareerEvent::Applied(ApplyEvent::RaceEntry) => CareerState::RaceActive,
        CareerEvent::Applied(
            ApplyEvent::ExecCommand
            | ApplyEvent::RaceEnd
            | ApplyEvent::RaceOut
            | ApplyEvent::CheckEvent
            | ApplyEvent::Continue,
        ) => CareerState::AssetTransition,
        // The view poll runs on present and may observe 1101 after a main-thread
        // completion hook already proved readiness. Do not let that delayed
        // identity observation close a genuinely settled window.
        CareerEvent::ViewChanged(ViewKind::Career) if matches!(state, CareerState::CommandSelectActive) => {
            CareerState::CommandSelectActive
        }
        CareerEvent::ViewChanged(ViewKind::Career | ViewKind::CareerIntermission) => CareerState::AssetTransition,
        CareerEvent::ViewChanged(ViewKind::Paddock | ViewKind::Race) => CareerState::RaceActive,
        CareerEvent::ViewChanged(ViewKind::Concert | ViewKind::Cutscene) => CareerState::CutsceneActive,
        CareerEvent::ViewChanged(ViewKind::OutsideCareer) => CareerState::Idle,
    }
}

/// Hiker law state. `lifecycle_state == 2` is `CommandSelectActive`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadState {
    pub lifecycle_state: i64,
    pub permitted: i64,
}

/// Formal implication: claiming permission requires `CommandSelectActive`.
#[must_use]
pub const fn read_gate(state: &ReadState) -> bool {
    !(state.permitted == 1) || state.lifecycle_state == CareerState::CommandSelectActive.as_i64()
}

/// Build the formal state from the runtime lifecycle and decide permission.
#[must_use]
pub const fn read_state(lifecycle: CareerState) -> ReadState {
    let permitted = if matches!(lifecycle, CareerState::CommandSelectActive) {
        1
    } else {
        0
    };
    ReadState {
        lifecycle_state: lifecycle.as_i64(),
        permitted,
    }
}

/// True only while the command-select UI is known to be complete and actionable.
#[must_use]
pub const fn reads_permitted(lifecycle: CareerState) -> bool {
    let state = read_state(lifecycle);
    state.permitted == 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const STATES: [CareerState; 7] = [
        CareerState::Idle,
        CareerState::CareerLoading,
        CareerState::CommandSelectActive,
        CareerState::CommandInFlight,
        CareerState::AssetTransition,
        CareerState::RaceActive,
        CareerState::CutsceneActive,
    ];

    const APPLIES: [ApplyEvent; 8] = [
        ApplyEvent::ExecCommand,
        ApplyEvent::RaceEntry,
        ApplyEvent::RaceEnd,
        ApplyEvent::RaceOut,
        ApplyEvent::CheckEvent,
        ApplyEvent::Continue,
        ApplyEvent::CareerStart,
        ApplyEvent::CareerLoad,
    ];

    #[test]
    fn exactly_one_state_permits_reads() {
        for state in STATES {
            assert_eq!(
                reads_permitted(state),
                state == CareerState::CommandSelectActive,
                "wrong permission for {state:?}"
            );
            assert!(read_gate(&read_state(state)));
        }
    }

    #[test]
    fn no_apply_event_can_permit_reads() {
        for state in STATES {
            for apply in APPLIES {
                let next = transition(state, CareerEvent::Applied(apply));
                assert!(!reads_permitted(next), "{state:?} + {apply:?} reached {next:?}");
            }
        }
    }

    #[test]
    fn career_load_requires_play_in_completion() {
        let loading = transition(CareerState::Idle, CareerEvent::Applied(ApplyEvent::CareerLoad));
        assert_eq!(loading, CareerState::CareerLoading);
        assert!(!reads_permitted(loading));
        let view_seen = transition(loading, CareerEvent::ViewChanged(classify_view(1101)));
        assert_eq!(view_seen, CareerState::AssetTransition);
        assert!(!reads_permitted(view_seen));
        let ready = transition(view_seen, CareerEvent::CommandViewPlayInCompleted);
        assert_eq!(ready, CareerState::CommandSelectActive);
        assert!(reads_permitted(ready));
    }

    #[test]
    fn race_and_concert_sequences_fail_closed_until_command_select() {
        let race = transition(
            CareerState::CommandInFlight,
            CareerEvent::ViewChanged(classify_view(1400)),
        );
        assert_eq!(race, CareerState::RaceActive);
        let race_applied = transition(race, CareerEvent::Applied(ApplyEvent::RaceEnd));
        assert_eq!(race_applied, CareerState::AssetTransition);
        assert!(!reads_permitted(race_applied));

        let concert = transition(
            CareerState::CommandInFlight,
            CareerEvent::ViewChanged(classify_view(1620)),
        );
        assert_eq!(concert, CareerState::CutsceneActive);
        assert!(!reads_permitted(concert));

        assert!(reads_permitted(transition(
            race_applied,
            CareerEvent::CommandSelectCompleted
        )));
        assert!(reads_permitted(transition(
            concert,
            CareerEvent::CommandSelectCompleted
        )));
    }

    #[test]
    fn delayed_career_view_observation_does_not_undo_completion() {
        assert_eq!(
            transition(
                CareerState::CommandSelectActive,
                CareerEvent::ViewChanged(ViewKind::Career)
            ),
            CareerState::CommandSelectActive
        );
    }

    proptest! {
        #[test]
        fn arbitrary_event_sequences_obey_single_permit_state(events in prop::collection::vec(0u8..32, 0..256)) {
            let mut state = CareerState::Idle;
            for raw in events {
                let event = match raw % 16 {
                    0 => CareerEvent::CommandSubmitted,
                    1 => CareerEvent::CommandSelectCompleted,
                    2 => CareerEvent::CommandViewPlayInCompleted,
                    3 => CareerEvent::Applied(ApplyEvent::ExecCommand),
                    4 => CareerEvent::Applied(ApplyEvent::RaceEntry),
                    5 => CareerEvent::Applied(ApplyEvent::RaceEnd),
                    6 => CareerEvent::Applied(ApplyEvent::RaceOut),
                    7 => CareerEvent::Applied(ApplyEvent::CheckEvent),
                    8 => CareerEvent::Applied(ApplyEvent::Continue),
                    9 => CareerEvent::Applied(ApplyEvent::CareerStart),
                    10 => CareerEvent::Applied(ApplyEvent::CareerLoad),
                    11 => CareerEvent::ViewChanged(ViewKind::Career),
                    12 => CareerEvent::ViewChanged(ViewKind::Race),
                    13 => CareerEvent::ViewChanged(ViewKind::Concert),
                    14 => CareerEvent::ViewChanged(ViewKind::OutsideCareer),
                    _ => CareerEvent::Reset,
                };
                state = transition(state, event);
                prop_assert_eq!(reads_permitted(state), state == CareerState::CommandSelectActive);
                prop_assert!(read_gate(&read_state(state)));
            }
        }
    }
}
