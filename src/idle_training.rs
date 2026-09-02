//! Tell the player when Independent Training lands.
//!
//! Independent Training runs on a real-world timer — three quarters of an hour
//! is normal — and the whole point of it is that you go and do something else.
//! So the notification has to survive the player not looking at the game, which
//! rules out anything driven by the screen they are on.
//!
//! # Two clocks, one deadline
//!
//! `WorkIdleSingleModeData.EndTime` is known the moment a session starts, so
//! the watcher does not have to *detect* completion — it only has to remember
//! the deadline and watch the wall clock:
//!
//! - a slow poll (every [`POLL_INTERVAL_SECS`]) reads the game's session on the
//!   Unity main thread and arms the deadline;
//! - the per-frame tick compares that deadline against the clock and needs no
//!   IL2CPP at all, so it is free and runs on the render thread.
//!
//! The poll interval is therefore only how fast a *new* session is noticed, not
//! how late the notification is. The notification lands within a frame.
//!
//! # Fail closed
//!
//! Nothing arms unless the game says `Playing` with an end time that is both in
//! the future and inside [`MAX_LEAD_SECS`]. A session already over when first
//! seen — the player was away for it — never notifies, because the game's own
//! result screen is waiting for them anyway.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::compat::Sdk;
use crate::memory_reader::{self, IdleSession};

/// Re-exported so a consumer of the countdown has one place to import from.
pub use crate::memory_reader::IdleState;

/// How often the game's session is re-read. Only affects how fast a new or
/// cancelled session is noticed — never how late the notification is.
const POLL_INTERVAL_SECS: i64 = 15;

/// The furthest ahead an end time may sit and still be believed. Independent
/// Training runs for well under a day; a value beyond this is not a long
/// session, it is a unit mismatch (milliseconds read as seconds), and arming on
/// it would mean never notifying while looking like it worked.
const MAX_LEAD_SECS: i64 = 24 * 60 * 60;

/// What the watcher is waiting for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Watch {
    /// Unix second the armed session lands on; `0` when nothing is armed.
    deadline: i64,
    /// The deadline already notified for. Keeps a session that still reports
    /// `Playing` after its own end time from notifying twice.
    notified: i64,
}

impl Watch {
    const IDLE: Self = Self {
        deadline: 0,
        notified: 0,
    };
}

/// The watcher state. Taken only when a deadline is actually in play — the
/// per-frame fast path reads [`DEADLINE`] and returns without locking.
static WATCH: Mutex<Watch> = Mutex::new(Watch::IDLE);

/// Mirror of `WATCH.deadline`, so the per-frame check costs one atomic load.
static DEADLINE: AtomicI64 = AtomicI64::new(0);

/// Unix second of the last poll, so [`tick`] can pace itself.
static LAST_POLL_SECS: AtomicI64 = AtomicI64::new(0);
/// Whether a poll callback is scheduled or running.
static POLL_SCHEDULED: AtomicBool = AtomicBool::new(false);

/// Last observation, for the diagnostic panel. `None` until the first poll.
static LAST_SEEN: Mutex<Option<IdleSession>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// Pure rules
// ---------------------------------------------------------------------------

/// Whether an end time is one we are willing to count down to.
const fn believable(end_time: i64, now: i64) -> bool {
    end_time > now && end_time - now <= MAX_LEAD_SECS
}

/// Fold one observation into the watch.
///
/// Callers must settle an already-due deadline *before* calling this (see
/// [`fire_if_due`]) — an observation whose end time has passed disarms, and
/// doing that to a deadline nobody has acted on yet would swallow the
/// notification.
fn observe(watch: Watch, session: Option<IdleSession>, now: i64) -> Watch {
    let Some(session) = session else {
        // No session object at all: the game has nothing for us to wait on.
        return Watch::IDLE;
    };
    let running = session.state == IdleState::Playing;
    if running && believable(session.end_time, now) && session.end_time != watch.notified {
        return Watch {
            deadline: session.end_time,
            ..watch
        };
    }
    // Cancelled, collected, already notified, or a time we do not believe.
    Watch { deadline: 0, ..watch }
}

/// Settle an armed deadline the clock has reached: returns the watch to record
/// and whether the caller should notify.
fn settle(watch: Watch, now: i64) -> (Watch, bool) {
    if watch.deadline == 0 || now < watch.deadline {
        return (watch, false);
    }
    (
        Watch {
            deadline: 0,
            notified: watch.deadline,
        },
        true,
    )
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Per-frame pump, called from the host frame callback. One atomic load in the
/// common case; no IL2CPP, so it is safe on the render thread.
pub fn tick() {
    let now = now_secs();
    fire_if_due(now);

    if now - LAST_POLL_SECS.load(Ordering::Acquire) < POLL_INTERVAL_SECS {
        return;
    }
    if !honse_services::is_game_ready() {
        return;
    }
    if POLL_SCHEDULED.swap(true, Ordering::AcqRel) {
        return; // one in flight
    }
    LAST_POLL_SECS.store(now, Ordering::Release);
    Sdk::get().schedule_on_main_thread(poll_cb);
}

/// Notify exactly once when the clock reaches an armed deadline.
fn fire_if_due(now: i64) {
    // Fast path: nothing armed, or not there yet. No lock.
    let deadline = DEADLINE.load(Ordering::Acquire);
    if deadline == 0 || now < deadline {
        return;
    }
    let fire = {
        let mut watch = WATCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (next, fire) = settle(*watch, now);
        *watch = next;
        DEADLINE.store(next.deadline, Ordering::Release);
        fire
    };
    if fire {
        announce();
    }
}

/// Read the game's session and re-arm. Unity main thread (scheduled by [`tick`]).
extern "C" fn poll_cb() {
    // Settle first: if the deadline passed while frames were not being drawn
    // (minimised, or alt-tabbed to something heavy), this is where it gets
    // noticed, and `observe` below would otherwise disarm it as already over.
    let now = now_secs();
    fire_if_due(now);

    let session = memory_reader::read_idle_session();
    *LAST_SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = session;

    {
        let mut watch = WATCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let before = *watch;
        *watch = observe(*watch, session, now);
        DEADLINE.store(watch.deadline, Ordering::Release);
        if watch.deadline != 0 && watch.deadline != before.deadline {
            hlog_info!(
                target: "training-tracker",
                "Independent Training lands in {}s",
                watch.deadline - now
            );
            // Get the tray icon in place now rather than at the moment the
            // notification fires: the shell refuses a balloon on an icon it has
            // not finished establishing, and "now" is usually three quarters of
            // an hour of head start.
            #[cfg(windows)]
            honse_services::toast::prepare();
        }
    }

    POLL_SCHEDULED.store(false, Ordering::Release);
}

/// Say it everywhere a player might be: in the game if they are watching it, on
/// the taskbar if they are in another window, and in the notification centre if
/// they are not at the machine at all.
fn announce() {
    hlog_info!(target: "training-tracker", "Independent Training complete");
    Sdk::get().show_notification("Independent Training is done!");
    #[cfg(windows)]
    {
        honse_services::input_block::flash_window();
        honse_services::toast::show(
            "Independent Training",
            "Your trainee is back \u{2014} the run is ready to collect.",
        );
    }
}

/// Whether a notification is armed and waiting on the clock.
///
/// Not the same question as "is a session running": a session whose end time
/// was refused as implausible still runs, and still counts down on screen, but
/// nothing will announce it. That gap is the thing worth being able to see.
#[must_use]
pub fn is_armed() -> bool {
    DEADLINE.load(Ordering::Acquire) != 0
}

/// The countdown as it stands right now, or `None` before the first poll.
#[must_use]
pub fn countdown() -> Option<Countdown> {
    let session = *LAST_SEEN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    Some(countdown_at(session?, now_secs()))
}

/// How a session reads on the clock: how long is left, and how far along it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Countdown {
    pub state: IdleState,
    /// Seconds until it lands; `0` once it has.
    pub remaining: i64,
    /// `0.0` at the start, `1.0` once landed — the same span the game's own
    /// gauge fills across.
    pub progress: f32,
}

/// Pure: read one session against a moment.
///
/// The game gives both ends of the session, so progress is measured rather than
/// guessed from a duration we would otherwise have to hardcode. A session with
/// no span left to measure (both ends equal, or the pair unset) reads as
/// finished rather than dividing by zero.
fn countdown_at(session: IdleSession, now: i64) -> Countdown {
    let remaining = (session.end_time - now).max(0);
    let total = session.end_time - session.start_time;
    let progress = if total <= 0 {
        1.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let done = (total - remaining) as f32 / total as f32;
        done.clamp(0.0, 1.0)
    };
    Countdown {
        state: session.state,
        remaining,
        progress,
    }
}

#[cfg(test)]
mod tests {
    use super::{countdown_at, observe, settle, IdleSession, IdleState, Watch, MAX_LEAD_SECS};

    const NOW: i64 = 1_700_000_000;

    fn session(state: IdleState, end_time: i64) -> Option<IdleSession> {
        Some(IdleSession {
            state,
            start_time: end_time - 2700,
            end_time,
        })
    }

    #[test]
    fn a_running_session_arms_its_end_time() {
        let watch = observe(Watch::IDLE, session(IdleState::Playing, NOW + 2700), NOW);
        assert_eq!(watch.deadline, NOW + 2700);
    }

    /// The case the whole module exists for: the player walked away, the clock
    /// reached the deadline, and exactly one notification comes out of it.
    #[test]
    fn the_deadline_notifies_once() {
        let armed = observe(Watch::IDLE, session(IdleState::Playing, NOW + 60), NOW);

        let (waiting, fire) = settle(armed, NOW + 59);
        assert!(!fire, "not due yet");
        assert_eq!(waiting, armed);

        let (settled, fire) = settle(waiting, NOW + 60);
        assert!(fire, "due");
        assert_eq!(settled.deadline, 0);

        let (_, again) = settle(settled, NOW + 600);
        assert!(!again, "a settled watch never fires again");
    }

    /// The server can leave `State` at `Playing` past the end time until the
    /// player opens the result. That must not re-arm what was just notified.
    #[test]
    fn a_notified_session_does_not_re_arm() {
        let armed = observe(Watch::IDLE, session(IdleState::Playing, NOW + 60), NOW);
        let (settled, _) = settle(armed, NOW + 60);

        let after = observe(settled, session(IdleState::Playing, NOW + 60), NOW + 90);
        assert_eq!(after.deadline, 0);
        assert_eq!(after.notified, NOW + 60);

        // …but the next session is a different deadline, and does arm.
        let next = observe(after, session(IdleState::Playing, NOW + 4000), NOW + 90);
        assert_eq!(next.deadline, NOW + 4000);
    }

    /// Cancelling, or coming back after the game already settled the result,
    /// leaves nothing to announce.
    #[test]
    fn only_a_running_session_arms() {
        for state in [
            IdleState::Idle,
            IdleState::Finished,
            IdleState::LogChecked,
            IdleState::Unrecognised(7),
        ] {
            let watch = observe(Watch::IDLE, session(state, NOW + 2700), NOW);
            assert_eq!(watch.deadline, 0, "{state:?} must not arm");
        }
        assert_eq!(observe(Watch::IDLE, None, NOW), Watch::IDLE);
    }

    /// A session that ended while we were not running belongs to the game's own
    /// result screen, not to us.
    #[test]
    fn a_session_already_over_never_arms() {
        let watch = observe(Watch::IDLE, session(IdleState::Playing, NOW - 1), NOW);
        assert_eq!(watch.deadline, 0);
    }

    /// If `EndTime` ever turns out to be milliseconds, arming on it would mean
    /// a notification in the year 55000 — silence that looks like success.
    /// Refusing it is what puts the problem in the log instead.
    #[test]
    fn an_implausible_end_time_is_refused() {
        let watch = observe(Watch::IDLE, session(IdleState::Playing, NOW + MAX_LEAD_SECS + 1), NOW);
        assert_eq!(watch.deadline, 0);
    }

    /// Progress is measured against the game's own start/end pair rather than a
    /// duration we would have to hardcode, so the bar matches its gauge.
    #[test]
    fn the_countdown_measures_the_span_the_game_gave_us() {
        let hour = IdleSession {
            state: IdleState::Playing,
            start_time: NOW,
            end_time: NOW + 3600,
        };
        let halfway = countdown_at(hour, NOW + 1800);
        assert_eq!(halfway.remaining, 1800);
        assert!((halfway.progress - 0.5).abs() < 0.001, "{}", halfway.progress);

        let landed = countdown_at(hour, NOW + 3600);
        assert_eq!(landed.remaining, 0);
        assert!((landed.progress - 1.0).abs() < f32::EPSILON);
    }

    /// Past the end time the countdown must sit at zero rather than going
    /// negative — the panel prints it, and "-0:04:11 left" is nonsense.
    #[test]
    fn an_overrun_countdown_stops_at_zero() {
        let session = session(IdleState::Playing, NOW).expect("session");
        let over = countdown_at(session, NOW + 600);
        assert_eq!(over.remaining, 0);
        assert!((over.progress - 1.0).abs() < f32::EPSILON);
    }

    /// A session with no span cannot be divided by. Reading as finished is the
    /// answer that neither panics nor claims a fake countdown.
    #[test]
    fn a_session_with_no_span_reads_as_finished() {
        let instant = IdleSession {
            state: IdleState::Finished,
            start_time: NOW,
            end_time: NOW,
        };
        let c = countdown_at(instant, NOW);
        assert_eq!(c.remaining, 0);
        assert!((c.progress - 1.0).abs() < f32::EPSILON);
    }

    /// Losing frames at the wrong moment must not lose the notification: the
    /// poll settles a due deadline before it folds in the fresh observation.
    #[test]
    fn a_deadline_reached_while_frames_stopped_still_fires() {
        let armed = observe(Watch::IDLE, session(IdleState::Playing, NOW + 60), NOW);
        // Game minimised for ten minutes; the poll wakes up well past the end.
        let (settled, fire) = settle(armed, NOW + 600);
        assert!(fire);
        // Only now does the stale-looking observation get folded in.
        let after = observe(settled, session(IdleState::Playing, NOW + 60), NOW + 600);
        assert_eq!(after.deadline, 0);
    }
}
