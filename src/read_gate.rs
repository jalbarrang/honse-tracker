//! Event-driven career lifecycle and crash-safety read law.
//!
//! IL2CPP career reads are permitted in exactly one lifecycle state:
//! [`CareerState::CommandSelectActive`]. Every transition is driven by a game
//! hook; there is no timer or independent boolean gate that can reopen reads.
//!
//! # Identity is not policy
//!
//! Screens are identified by [`View`] over in `honse_services::scene_views`,
//! which knows only *which screen this is*. This module owns what that means:
//! [`career_state_for_view`] is the whole of it, and it is exhaustive so a newly
//! catalogued screen cannot be added without deciding its behaviour.

pub use honse_services::scene_views::View;

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
    /// An in-career screen that is not the command screen and not playback:
    /// shops, lists, the completion summary, the Quick-mode story text. Reads
    /// stay closed because assets can move underneath them, but nothing is
    /// covering the screen, so panels keep painting their last settled capture.
    ///
    /// The dividing line against [`CareerState::CutsceneActive`] is whether a
    /// panel would be drawn over a video, not whether the player is idle.
    CareerMenu = 7,
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
            7 => Self::CareerMenu,
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

/// Inputs accepted by the pure lifecycle reducer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CareerEvent {
    CommandSubmitted,
    CommandSelectCompleted,
    CommandViewPlayInCompleted,
    Applied(ApplyEvent),
    ViewChanged(View),
    Reset,
}

/// The lifecycle state merely *being on* a screen implies.
///
/// This is policy, and it is exhaustive on purpose: adding a [`View`] will not
/// compile until its behaviour is decided here. That is what stops the old
/// failure mode, where an uncatalogued id fell through a catch-all and silently
/// blacked out the HUD.
///
/// No screen grants read permission — only the completion hooks do. The choice
/// each arm is really making is whether panels keep painting.
#[must_use]
pub const fn career_state_for_view(view: View) -> CareerState {
    match view {
        // The command screen itself. Still not a settle proof: the poll can
        // observe it before the UI has finished rebuilding.
        View::CareerTraining | View::Intermission => CareerState::AssetTransition,

        // A generic transition. Assets are moving and the destination is not
        // known yet, so hold rather than blink the HUD off and on again.
        View::ScreenTransition => CareerState::AssetTransition,

        // In-career screens that are not playback. Reads closed, panels
        // painting: shops and lists are where you spend resources and want your
        // numbers; the completion screens are where leftover skill points and
        // lessons go; Quick mode is the summarised light-novel text, read at
        // your own pace rather than watched.
        View::TrackblazerPointsShop
        | View::CareerSkillsShop
        | View::GrandConcertTechniquesShop
        | View::RaceList
        | View::CareerPreComplete
        | View::CareerInspiration
        | View::CareerStoryQuick => CareerState::CareerMenu,

        // Race flow, up to and including the paddock.
        View::RacePaddock | View::RacePlayback => CareerState::RaceActive,

        // Full-screen playback. Panels would be drawing over a video.
        //
        // Independent Training looks like a career screen — it has a timer and
        // a menu button — but it is a montage that plays itself out. There is
        // no decision to support, so panels stay out of the way.
        View::ConcertPlayback | View::GrandConcertConcertView | View::CareerIndependentTraining => {
            CareerState::CutsceneActive
        }

        // Outside a career: boot, the home screen, every mode reached from it,
        // and the point where a run is actually over. `Idle` is the honest
        // answer — there is no career lifecycle to be in, so reads fail closed
        // and career panels have nothing left to say.
        //
        // `CareerComplete` is the exit, not the summary: 1300 (Pre-Complete) is
        // still career mode and keeps its panels, 1301 is done and stops.
        View::Launch
        | View::PressStart
        | View::Home
        | View::CareerComplete
        | View::GachaPull
        | View::TeamTrialsHome
        | View::TeamTrialsEditTeam
        | View::TeamTrialsOpponentSelect
        | View::TeamTrialsScheduledBattle
        | View::TeamTrialsBattle
        | View::TeamTrialsResults
        | View::DailyLegendRaceHome
        | View::DailyLegendRaceSingle
        | View::DailyLegendRacePaddock
        | View::ConcertTheaterHome => CareerState::Idle,

        // Uncatalogued. Fail closed on both axes until someone names it — the
        // debug panel calls these out for exactly this reason.
        View::Unknown => CareerState::CutsceneActive,
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
        // The view poll runs on present and may observe the training screen
        // after a main-thread completion hook already proved readiness. Do not
        // let that delayed identity observation close a genuinely settled
        // window. This is the one place a view is more than its policy.
        CareerEvent::ViewChanged(View::CareerTraining) if matches!(state, CareerState::CommandSelectActive) => {
            CareerState::CommandSelectActive
        }
        CareerEvent::ViewChanged(view) => career_state_for_view(view),
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

    const STATES: [CareerState; 8] = [
        CareerState::Idle,
        CareerState::CareerLoading,
        CareerState::CommandSelectActive,
        CareerState::CommandInFlight,
        CareerState::AssetTransition,
        CareerState::RaceActive,
        CareerState::CutsceneActive,
        CareerState::CareerMenu,
    ];

    /// Every screen policy must answer for: the whole catalogue, plus the
    /// uncatalogued case. Read from the table itself so adding a screen extends
    /// the coverage automatically instead of quietly escaping it.
    fn all_views() -> impl Iterator<Item = View> {
        View::catalogued().chain(std::iter::once(View::Unknown))
    }

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
        let view_seen = transition(loading, CareerEvent::ViewChanged(View::from_id(1101)));
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
            CareerEvent::ViewChanged(View::from_id(400)),
        );
        assert_eq!(race, CareerState::RaceActive);
        let race_applied = transition(race, CareerEvent::Applied(ApplyEvent::RaceEnd));
        assert_eq!(race_applied, CareerState::AssetTransition);
        assert!(!reads_permitted(race_applied));

        let concert = transition(
            CareerState::CommandInFlight,
            CareerEvent::ViewChanged(View::from_id(1621)),
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
                CareerEvent::ViewChanged(View::CareerTraining)
            ),
            CareerState::CommandSelectActive
        );
    }

    /// The regression the whole refactor exists for: the two shops were
    /// classified as playback and hid the HUD. They must reach `CareerMenu`,
    /// which keeps reads closed but lets panels paint.
    #[test]
    fn in_career_shops_are_menus_not_playback() {
        for id in [1620, 1400, 35, 1210, 1300, 3000] {
            let view = View::from_id(id);
            assert_eq!(
                career_state_for_view(view),
                CareerState::CareerMenu,
                "view {id} ({view:?}) should be a menu"
            );
        }
        // …while the screens that really are playback stay hidden.
        for id in [1621, 6600] {
            assert_eq!(
                career_state_for_view(View::from_id(id)),
                CareerState::CutsceneActive,
                "view {id} plays itself out; panels would sit on top of it"
            );
        }
    }

    /// A career ends at 1301, not 1300. Getting this pair backwards either
    /// keeps tracking a finished run or drops the screen where the last skill
    /// points are spent.
    #[test]
    fn a_career_ends_at_complete_not_pre_complete() {
        assert_eq!(career_state_for_view(View::from_id(1300)), CareerState::CareerMenu);
        assert_eq!(career_state_for_view(View::from_id(1301)), CareerState::Idle);
    }

    #[test]
    fn no_screen_ever_grants_read_permission() {
        for view in all_views() {
            assert!(
                !reads_permitted(career_state_for_view(view)),
                "{view:?} granted reads"
            );
            for state in STATES {
                let next = transition(state, CareerEvent::ViewChanged(view));
                // The one exception is the delayed observation of the training
                // screen, which preserves an already-proven window.
                let preserved = state == CareerState::CommandSelectActive && view == View::CareerTraining;
                assert_eq!(reads_permitted(next), preserved, "{state:?} + {view:?} -> {next:?}");
            }
        }
    }

    #[test]
    fn an_uncatalogued_screen_fails_closed() {
        let unknown = View::from_id(424_242);
        assert_eq!(unknown, View::Unknown);
        assert!(!reads_permitted(career_state_for_view(unknown)));
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
                    11 => CareerEvent::ViewChanged(View::CareerTraining),
                    12 => CareerEvent::ViewChanged(View::RacePlayback),
                    13 => CareerEvent::ViewChanged(View::GrandConcertTechniquesShop),
                    14 => CareerEvent::ViewChanged(View::Unknown),
                    _ => CareerEvent::Reset,
                };
                state = transition(state, event);
                prop_assert_eq!(reads_permitted(state), state == CareerState::CommandSelectActive);
                prop_assert!(read_gate(&read_state(state)));
            }
        }
    }
}
