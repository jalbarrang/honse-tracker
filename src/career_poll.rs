//! Event-driven settled-turn capture and telemetry publication.
//!
//! There is no periodic career poll. Passive game edges request a capture and
//! advance one lifecycle state; a per-frame atomic pump schedules the held
//! request onto the Unity main thread only in `CommandSelectActive`. The
//! callback rechecks that state, resolves the IL2CPP chain lazily, and reads
//! publishes exactly one atomic `SettledTurn` (content-deduplicated, with a
//! stable capture id) through the bounded telemetry transport.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hachimi_telemetry::{pb, Message};

use crate::compat::Sdk;

use crate::deck_bonuses;
use crate::memory_reader::{self, EvaluationInfo, FiredEvent};
use crate::read_gate::{ApplyEvent, CareerEvent, CareerState};

/// Equipped `(deck slot, support_card_id)` pairs from the previous capture.
static PREV_SUPPORT_IDS: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Event-driven career lifecycle (the only crash-safety authority)
// ---------------------------------------------------------------------------

/// One atomic lifecycle value replaces the former cooldown/deadline/pending
/// gates. Unknown values decode as `Idle`, so corruption or a missed event fails
/// closed. Only post-original UI completion hooks can enter the readable state.
static LIFECYCLE_STATE: AtomicU8 = AtomicU8::new(CareerState::Idle as u8);

#[must_use]
fn lifecycle_state() -> CareerState {
    CareerState::from_u8(LIFECYCLE_STATE.load(AtomicOrdering::Acquire))
}

/// Atomically apply one pure reducer event. Hooks can arrive on the Unity main,
/// render, or response thread, so a compare/exchange loop preserves every edge.
fn advance_lifecycle(event: CareerEvent) -> (CareerState, CareerState) {
    let mut raw = LIFECYCLE_STATE.load(AtomicOrdering::Acquire);
    loop {
        let from = CareerState::from_u8(raw);
        let to = crate::read_gate::transition(from, event);
        match LIFECYCLE_STATE.compare_exchange_weak(raw, to as u8, AtomicOrdering::AcqRel, AtomicOrdering::Acquire) {
            Ok(_) => {
                hlog_info!(target: "settle-diag", "lifecycle {from:?} --{event:?}--> {to:?}");
                return (from, to);
            }
            Err(observed) => raw = observed,
        }
    }
}

/// Record a polled current-view change. A view identity can classify an unsafe
/// phase but never opens reads; 1101 observed after a completion hook is treated
/// as a delayed poll observation by the reducer.
pub fn note_view_change(view_id: i32) {
    let kind = crate::read_gate::classify_view(view_id);
    advance_lifecycle(CareerEvent::ViewChanged(kind));
    request_capture();
}

/// Test/inspection helper for the single runtime lifecycle state.
#[must_use]
pub fn current_lifecycle_state() -> CareerState {
    lifecycle_state()
}

/// Test helper: whether IL2CPP career reads currently fail closed.
#[must_use]
pub fn reads_currently_unsafe() -> bool {
    reads_unsafe()
}

fn reads_unsafe() -> bool {
    !crate::read_gate::reads_permitted(lifecycle_state())
}

// ---------------------------------------------------------------------------
// Event-driven capture scheduling
// ---------------------------------------------------------------------------

/// Held capture request. Set by passive edges, consumed by the main-thread
/// capture callback. Requests made outside `CommandSelectActive` stay held and
/// coalesce until a post-original completion event permits one capture.
static CAPTURE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Whether a main-thread capture callback is currently scheduled/in flight.
static CAPTURE_SCHEDULED: AtomicBool = AtomicBool::new(false);
/// Wall-clock (ms) when the in-flight callback was scheduled; this repairs only
/// scheduler bookkeeping and never changes lifecycle/read permission.
static SCHEDULED_SINCE_MS: AtomicU64 = AtomicU64::new(0);

/// If a scheduled capture hasn't completed within this window, treat its slot
/// as lost. This is not a read gate: the lifecycle is rechecked independently.
const SCHEDULE_STALE_MS: u64 = 5000;

/// A career command was submitted before the original coroutine starts.
pub(crate) fn enter_command() {
    advance_lifecycle(CareerEvent::CommandSubmitted);
}

/// Observe fresh server data without claiming that assets or UI are stable.
pub(crate) fn apply_observed(event: ApplyEvent) {
    advance_lifecycle(CareerEvent::Applied(event));
    request_capture();
}

/// The command-select setup original returned; the UI is rebuilt and actionable.
pub(crate) fn command_select_settled() {
    advance_lifecycle(CareerEvent::CommandSelectCompleted);
    request_capture();
}

/// Initial/resumed career command-view play-in completed. This is the settle
/// edge for load paths that do not call `SetupCommandSelectStart*`.
pub(crate) fn command_view_play_in_completed() {
    advance_lifecycle(CareerEvent::CommandViewPlayInCompleted);
    request_capture();
}

/// Hold/coalesce a capture request. Never reads IL2CPP, never blocks — safe
/// from any thread. The pump schedules it only in `CommandSelectActive`.
pub(crate) fn request_capture() {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        return;
    }
    CAPTURE_REQUESTED.store(true, AtomicOrdering::Release);
}

/// Decide whether the pump should schedule the capture callback now, claiming
/// the schedule slot when it does. Atomic bookkeeping plus one lifecycle check;
/// the scheduling side effect stays in [`tick`] for deterministic tests.
fn take_schedule_slot(now: u64) -> bool {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        return false;
    }
    if !CAPTURE_REQUESTED.load(AtomicOrdering::Acquire) {
        return false;
    }
    // Require the sole readable lifecycle before scheduling; the callback
    // rechecks on the main thread (defense in depth).
    if reads_unsafe() {
        return false;
    }
    if CAPTURE_SCHEDULED.swap(true, AtomicOrdering::AcqRel) {
        // Already scheduled: coalesce, unless the in-flight callback looks lost
        // (scheduled long ago, never completed) — then reclaim the slot.
        let since = SCHEDULED_SINCE_MS.load(AtomicOrdering::Relaxed);
        if since == 0 || now.saturating_sub(since) < SCHEDULE_STALE_MS {
            return false;
        }
    }
    SCHEDULED_SINCE_MS.store(now, AtomicOrdering::Relaxed);
    true
}

/// Per-frame atomic pump called from the host frame callback. Schedules a held
/// capture only in `CommandSelectActive`. No IL2CPP access or career read happens
/// here — a per-frame atomic pump is allowed; a per-frame career read is not.
///
/// Captures are event-driven only (settled-turn hooks + view-change edges).
/// Periodic polling is intentionally absent: IL2CPP reads take ~80ms and asset
/// unloading can start on another thread mid-read, so timer-based captures
/// race use-after-free regardless of lifecycle checks at schedule time.
pub fn tick() {
    if take_schedule_slot(now_ms()) {
        Sdk::get().schedule_on_main_thread(capture_settled_turn_cb);
    }
}

// ---------------------------------------------------------------------------
// Career epoch + content deduplication
// ---------------------------------------------------------------------------

/// Per-career capture bookkeeping. A new epoch starts on the first active
/// capture, on a deck change, and on a turn rewind (both signal a new career
/// even when the game keeps `IsPlaying` true across the transition). The epoch
/// timestamp namespaces capture ids so identical content in two distinct
/// careers (e.g. an untouched turn 1 of the same trainee/deck) can never
/// collide on the sidecar's `capture_id` primary key.
struct EpochState {
    epoch_ms: u64,
    last_turn: i32,
    last_fingerprint: [u8; 32],
}

static EPOCH: Mutex<Option<EpochState>> = Mutex::new(None);

/// Outcome of the per-capture bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CaptureDecision {
    /// Publish under the given epoch namespace.
    Publish { epoch_ms: u64 },
    /// Exact same content as the previous capture (overlapping edges for one
    /// settled turn) — suppress.
    DuplicateContent,
}

/// Pure epoch/deduplication rules (unit-tested):
/// - no epoch yet, a deck change, or a turn rewind starts a new epoch and
///   always publishes;
/// - identical content fingerprint to the previous capture is suppressed;
/// - changed content (same or advanced turn) publishes under the same epoch.
fn epoch_decide(
    state: &mut Option<EpochState>,
    now_ms: u64,
    turn: i32,
    fingerprint: [u8; 32],
    deck_changed: bool,
) -> CaptureDecision {
    let new_epoch = match state.as_ref() {
        None => true,
        Some(s) => deck_changed || turn < s.last_turn,
    };
    if new_epoch {
        *state = Some(EpochState {
            epoch_ms: now_ms,
            last_turn: turn,
            last_fingerprint: fingerprint,
        });
        return CaptureDecision::Publish { epoch_ms: now_ms };
    }
    let s = state.as_mut().expect("checked above");
    if s.last_fingerprint == fingerprint {
        return CaptureDecision::DuplicateContent;
    }
    s.last_turn = turn;
    s.last_fingerprint = fingerprint;
    CaptureDecision::Publish { epoch_ms: s.epoch_ms }
}

/// Content fingerprint of a settled turn *before* identity is stamped (the
/// caller passes `capture_id: ""`, `captured_at_ms: 0`). Identical game state
/// read twice fingerprints identically, so overlapping edges dedupe.
fn fingerprint(turn: &pb::SettledTurn) -> [u8; 32] {
    debug_assert!(
        turn.capture_id.is_empty() && turn.captured_at_ms == 0,
        "fingerprint must run before identity is stamped"
    );
    *blake3::hash(&turn.encode_to_vec()).as_bytes()
}

/// Stable capture id: epoch namespace + turn + content fingerprint prefix.
/// Created once per logical capture; the encoded body built from it is
/// retained unchanged across delivery retries (at-least-once), and the sidecar
/// dedupes replays on this id.
fn capture_id_for(epoch_ms: u64, turn: i32, fingerprint: &[u8; 32]) -> String {
    use std::fmt::Write;
    let mut id = format!("e{epoch_ms:x}-t{turn}-");
    for b in &fingerprint[..6] {
        let _ = write!(id, "{b:02x}");
    }
    id
}

// ---------------------------------------------------------------------------
// View-poll lifecycle (active careers only)
// ---------------------------------------------------------------------------

/// Whether this module armed the per-frame view-id poll. The poll runs only
/// while a career is active: armed after the first active-career capture,
/// disarmed on career end and shutdown.
static VIEW_POLL_ARMED: AtomicBool = AtomicBool::new(false);

fn arm_view_poll() {
    if !VIEW_POLL_ARMED.swap(true, AtomicOrdering::AcqRel) {
        honse_services::set_view_poll_enabled(true);
    }
}

fn disarm_view_poll() {
    if VIEW_POLL_ARMED.swap(false, AtomicOrdering::AcqRel) {
        honse_services::set_view_poll_enabled(false);
    }
}

// ---------------------------------------------------------------------------
// Main-thread capture callback
// ---------------------------------------------------------------------------

extern "C" fn capture_settled_turn_cb() {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        CAPTURE_SCHEDULED.store(false, AtomicOrdering::Release);
        return;
    }
    // Defense in depth: a callback scheduled just before a view change or
    // command submit can arrive in a new unsafe state. Bail before any IL2CPP
    // access; the held request waits for the next completion edge.
    if reads_unsafe() {
        let state = lifecycle_state();
        hlog_debug!(target: "settle-diag", "capture DEFERRED — lifecycle={state:?}");
        CAPTURE_SCHEDULED.store(false, AtomicOrdering::Release);
        return;
    }
    // Consume the request before reading: an edge firing during the read
    // re-requests, so post-read state changes are never missed.
    CAPTURE_REQUESTED.store(false, AtomicOrdering::Release);
    // Run the (panic-prone) IL2CPP reads + telemetry behind a catch so a single
    // bad frame can never unwind across this `extern "C"` boundary nor wedge
    // the capture loop. CAPTURE_SCHEDULED is always restored, panic or not.
    if let Err(e) = std::panic::catch_unwind(capture_settled_turn_inner) {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic>".to_string());
        hlog_error!("capture_settled_turn_cb PANICKED: {msg} \u{2014} capture recovered for next edge");
    }
    CAPTURE_SCHEDULED.store(false, AtomicOrdering::Release);
}

/// No active career is readable: clear per-career state and stop the per-frame
/// view poll until the next active-career capture. Idempotent. A spurious call
/// mid-career is harmless — the sidecar groups careers by card/scenario/turn
/// monotonicity, so a fresh epoch never splits a stored run.
fn career_inactive() {
    advance_lifecycle(CareerEvent::Reset);
    let mut guard = EPOCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let was_active = guard.take().is_some();
    drop(guard);
    if was_active {
        hlog_info!(target: "training-tracker", "career ended \u{2014} capture epoch closed");
    }
    reset_career_state();
    disarm_view_poll();
}

fn capture_settled_turn_inner() {
    // Lazy, idempotent chain resolution — first capture pays the one-time
    // resolve; DLL load performs no career-state reads at all.
    if !memory_reader::ensure_resolved() {
        return; // game runtime not ready; the next settled edge retries
    }

    let Some(chara) = memory_reader::get_chara_ptr() else {
        // No active career (`IsPlaying` false or Character not built).
        career_inactive();
        return;
    };

    let mut snapshot = memory_reader::read_snapshot();
    let is_playing = snapshot.as_ref().is_some_and(|s| s.is_playing);
    if !is_playing {
        career_inactive();
        return;
    }

    let skills = memory_reader::read_acquired_skills();
    let evaluations = memory_reader::read_evaluations();
    let skill_points = memory_reader::read_skill_points();

    // Equipped support-card ids: re-read every capture (pure ObscuredInt field
    // reads, no Convert). Cheap, and avoids stale deck mapping when the game
    // keeps SingleMode "playing" across a career -> new-career transition.
    let support_ids = memory_reader::read_equipped_support_ids();
    // Deck change (new career / reshuffled deck) invalidates per-career progress,
    // the once-per-career deck-bonus capture, and the capture epoch. Require both
    // decks non-empty so a transient empty read can't wipe progress mid-career.
    let deck_changed = PREV_SUPPORT_IDS
        .lock()
        .ok()
        .is_some_and(|prev| !prev.is_empty() && !support_ids.is_empty() && prev.as_slice() != support_ids.as_slice());
    if deck_changed {
        crate::bond_progress::clear();
        deck_bonuses::clear(); // re-captured below via try_capture
        EVAL_DIAG_LOGGED.store(false, AtomicOrdering::Relaxed);
    }
    // Fired-event history: re-read each capture (read-only; grows over the career).
    let fired_events = memory_reader::read_fired_events();
    // Accumulate observed events into per-career progress (auto counter).
    crate::bond_progress::observe(&support_ids, &fired_events);
    deck_bonuses::try_capture(chara);
    // Self-computed evaluation estimate from stats + skills + aptitudes.
    if let Some(s) = snapshot.as_mut() {
        let stats = [s.speed, s.stamina, s.power, s.guts, s.wiz];
        s.evaluation_value = crate::evaluation::compute(stats, &s.aptitudes, s.star, &skills);
    }
    log_career_diagnostic(&evaluations, &support_ids, &fired_events);

    // Player-reserved races (the in-game agenda) are read for telemetry only.
    let reserved_races = memory_reader::read_reserved_races();

    // Remember the deck before publishing so a telemetry failure cannot prevent
    // deck-change detection on the next capture.
    if let Ok(mut prev) = PREV_SUPPORT_IDS.lock() {
        prev.clone_from(&support_ids);
    }

    // Build the atomic payload with identity blank, fingerprint the content,
    // then decide: duplicate edges for one settled turn publish once; changed
    // same-turn content publishes a new capture id; a retry of a published
    // body reuses the identical encoded bytes (publisher retains the body).
    let mut turn_pb = crate::telemetry::settled_turn_to_pb(&crate::telemetry::SettledTurnInput {
        capture_id: "",
        captured_at_ms: 0,
        snapshot: snapshot.as_ref(),
        skills: &skills,
        evaluations: &evaluations,
        skill_points,
        support_ids: &support_ids,
        reserved_races: &reserved_races,
    });
    let fp = fingerprint(&turn_pb);
    let turn_no = snapshot.as_ref().map_or(0, |s| s.current_turn);
    let decision = {
        let mut guard = EPOCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        epoch_decide(&mut guard, now_ms(), turn_no, fp, deck_changed)
    };
    match decision {
        CaptureDecision::DuplicateContent => {}
        CaptureDecision::Publish { epoch_ms } => {
            turn_pb.capture_id = capture_id_for(epoch_ms, turn_no, &fp);
            turn_pb.captured_at_ms = now_ms();
            // Non-blocking enqueue; a no-op when telemetry is disabled. The
            // outer catch_unwind contains any telemetry failure.
            crate::telemetry::publish_settled_turn(turn_pb);
        }
    }
    // A career is active: keep the per-frame view-id poll armed so view-settled
    // edges (races, story events, career exit) keep arriving.
    arm_view_poll();
}

// ---------------------------------------------------------------------------
// t-001 settled-turn diagnostics (TEMPORARY — removed once the runtime
// coverage matrix is recorded).
//
// One structured line per passive edge, proving (a) which edge fires after each
// settled turn across the career matrix and (b) that no IL2CPP read happens in
// an unsafe or pre-original window. Grep target: `settle-diag`.
//
// Schema (space-separated key=value, one line per edge):
//   seq=<u64>        global edge counter — proves ordering across hooks/events
//   t_ms=<u64>       unix wall-clock ms — coarse timing between edges
//   hook=<name>      IL2CPP hook or ViewChange
//   phase=<p>        before_original | after_original | event
//   reason=<r>       lifecycle edge reason
//   state=<s>        single career lifecycle state at log time
//   permitted=<0|1>  true only for CommandSelectActive
//   turn=<i32|na>    read only on a permitted post-original completion edge
//   view_id=<i32|na> VIEW_CHANGE payload (event edges only)
//   thread=<id>      correlates edges to the Unity main thread
// ---------------------------------------------------------------------------

static DIAG_SEQ: AtomicU64 = AtomicU64::new(0);

/// Pure formatter for one settle-diag line (schema locked by unit tests).
#[must_use]
fn format_diag_line(
    seq: u64,
    t_ms: u64,
    hook: &str,
    phase: &str,
    reason: &str,
    state: CareerState,
    permitted: bool,
    turn: Option<i32>,
    view_id: Option<i32>,
    thread: &str,
) -> String {
    let turn = turn.map_or_else(|| "na".to_owned(), |t| t.to_string());
    let view_id = view_id.map_or_else(|| "na".to_owned(), |v| v.to_string());
    format!(
        "seq={seq} t_ms={t_ms} hook={hook} phase={phase} reason={reason} state={state:?} permitted={} turn={turn} view_id={view_id} thread={thread}",
        u8::from(permitted)
    )
}

/// Log one settled-turn diagnostic edge.
///
/// `try_read_turn` may be true only on post-original completion edges running
/// on the Unity main thread. The turn read uses the same single-state check as
/// production; every other lifecycle logs `turn=na`.
pub(crate) fn diag_settle_edge(
    hook: &'static str,
    phase: &'static str,
    reason: &'static str,
    view_id: Option<i32>,
    try_read_turn: bool,
) {
    let seq = DIAG_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    let state = lifecycle_state();
    let permitted = crate::read_gate::reads_permitted(state);
    let turn = if try_read_turn && permitted {
        memory_reader::diag_read_current_turn()
    } else {
        None
    };
    let line = format_diag_line(
        seq,
        now_ms(),
        hook,
        phase,
        reason,
        state,
        permitted,
        turn,
        view_id,
        &format!("{:?}", std::thread::current().id()),
    );
    hlog_info!(target: "settle-diag", "{line}");
}

// ---------------------------------------------------------------------------
// Per-career accumulator resets + one-shot career diagnostics
// ---------------------------------------------------------------------------

pub(crate) fn reset_career_state() {
    EVAL_DIAG_LOGGED.store(false, AtomicOrdering::Relaxed);
    crate::bond_progress::clear();
    deck_bonuses::clear();
    if let Ok(mut guard) = PREV_SUPPORT_IDS.lock() {
        guard.clear();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// One-shot per career: dump the (safe, already-read) evaluation rows so the
/// `target_id` (deck slot 1–6 / guest) ↔ `story_step` relationship can be correlated
/// against a known deck. Evaluation-only — touches no support-card/deck memory.
static EVAL_DIAG_LOGGED: AtomicBool = AtomicBool::new(false);

fn log_career_diagnostic(evaluations: &[EvaluationInfo], support_ids: &[(i32, i32)], fired: &[FiredEvent]) {
    if evaluations.is_empty() || EVAL_DIAG_LOGGED.swap(true, AtomicOrdering::Relaxed) {
        return;
    }
    hlog_info!(target: "training-tracker", "Eval diagnostic ({} rows):", evaluations.len());
    for e in evaluations {
        hlog_info!(
            target: "training-tracker",
            "  target_id={} value={} story_step={} guest_chara_id={} is_appear={} name={:?}",
            e.target_id, e.value, e.story_step, e.guest_chara_id, e.is_appear, e.name
        );
    }
    // Probe the master evaluation table to learn target_id -> chara_id mapping.
    let target_ids: Vec<i32> = evaluations.iter().map(|e| e.target_id).collect();
    memory_reader::probe_eval_master(&target_ids);

    // Fired-event history sample (to compare ids against catalog chain keys).
    let ev_ids: std::collections::HashSet<i32> = fired.iter().map(|e| e.event_id).collect();
    let st_ids: std::collections::HashSet<i32> = fired.iter().map(|e| e.story_id).collect();
    hlog_info!(target: "training-tracker", "Fired events: {} total", fired.len());
    for e in fired.iter().take(12) {
        hlog_info!(target: "training-tracker", "  event_id={} story_id={}", e.event_id, e.story_id);
    }

    hlog_info!(target: "training-tracker", "Deck map ({} slots):", support_ids.len());
    for (slot, support_id) in support_ids {
        let name = crate::gametora_data::support_card_name(*support_id as i64).unwrap_or("?");
        let max = crate::gametora_data::max_chain_steps(*support_id as i64);
        let keys = crate::gametora_data::chain_event_keys(*support_id as i64);
        let matched = keys
            .iter()
            .filter(|(eid, sid)| {
                (*eid != 0 && ev_ids.contains(&(*eid as i32))) || (*sid != 0 && st_ids.contains(&(*sid as i32)))
            })
            .count();
        let sample: Vec<(i64, i64)> = keys.iter().take(3).copied().collect();
        hlog_info!(
            target: "training-tracker",
            "  slot={} support_id={} name={:?} max={:?} chain_keys={} matched={} keys_sample={:?}",
            slot, support_id, name, max, keys.len(), matched, sample
        );
    }
}

/// Stop scheduling captures and bail out of any in-flight main-thread callback.
/// Call from the plugin `SHUTDOWN` handler before the host frees the DLL.
/// Bounded: nothing waits; held requests are dropped and the view poll is
/// disarmed.
pub fn shutdown() {
    SHUTTING_DOWN.store(true, AtomicOrdering::Release);
    CAPTURE_REQUESTED.store(false, AtomicOrdering::Release);
    CAPTURE_SCHEDULED.store(false, AtomicOrdering::Release);
    SCHEDULED_SINCE_MS.store(0, AtomicOrdering::Release);
    LIFECYCLE_STATE.store(CareerState::Idle as u8, AtomicOrdering::Release);
    disarm_view_poll();
    if let Ok(mut guard) = EPOCH.lock() {
        *guard = None;
    }
    reset_career_state();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that touch the module's global lifecycle/schedule state.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    /// Reset every global this module owns so each test starts from a clean,
    /// non-shutdown state (some tests flip `SHUTTING_DOWN`).
    fn reset_state() {
        SHUTTING_DOWN.store(false, AtomicOrdering::Release);
        CAPTURE_REQUESTED.store(false, AtomicOrdering::Release);
        CAPTURE_SCHEDULED.store(false, AtomicOrdering::Release);
        SCHEDULED_SINCE_MS.store(0, AtomicOrdering::Release);
        LIFECYCLE_STATE.store(CareerState::Idle as u8, AtomicOrdering::Release);
        VIEW_POLL_ARMED.store(false, AtomicOrdering::Release);
        *EPOCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_state();
        guard
    }

    #[test]
    fn lifecycle_starts_fail_closed_until_ui_completion() {
        let _guard = lock();
        assert_eq!(current_lifecycle_state(), CareerState::Idle);
        assert!(reads_currently_unsafe());
        command_view_play_in_completed();
        assert_eq!(current_lifecycle_state(), CareerState::CommandSelectActive);
        assert!(!reads_currently_unsafe());
    }

    #[test]
    fn requests_coalesce_into_one_schedule_slot() {
        let _guard = lock();
        LIFECYCLE_STATE.store(CareerState::CommandSelectActive as u8, AtomicOrdering::Release);
        request_capture();
        request_capture(); // coalesces — still one held request
        assert!(take_schedule_slot(now_ms()), "first pump claims the slot");
        assert!(
            !take_schedule_slot(now_ms()),
            "second pump must coalesce while a callback is in flight"
        );
    }

    #[test]
    fn request_survives_unsafe_window_and_schedules_after_reopen() {
        let _guard = lock();
        LIFECYCLE_STATE.store(CareerState::CommandSelectActive as u8, AtomicOrdering::Release);
        enter_command();
        request_capture();
        assert!(
            !take_schedule_slot(now_ms()),
            "no scheduling outside the readable state"
        );
        assert!(
            CAPTURE_REQUESTED.load(AtomicOrdering::Acquire),
            "the request is retained, not dropped"
        );
        command_select_settled();
        assert!(
            take_schedule_slot(now_ms()),
            "pump schedules once command select is active"
        );
    }

    #[test]
    fn lost_schedule_slot_is_reclaimed_after_staleness_window() {
        let _guard = lock();
        LIFECYCLE_STATE.store(CareerState::CommandSelectActive as u8, AtomicOrdering::Release);
        request_capture();
        let t0 = now_ms();
        assert!(take_schedule_slot(t0));
        // The callback never completed. Within the window: coalesce.
        assert!(!take_schedule_slot(t0 + SCHEDULE_STALE_MS - 1));
        // Past the window: reclaim so capturing cannot wedge forever.
        assert!(take_schedule_slot(t0 + SCHEDULE_STALE_MS));
    }

    #[test]
    fn double_submit_is_idempotent_and_settle_restores_active_state() {
        let _guard = lock();
        command_view_play_in_completed();
        assert!(!reads_currently_unsafe(), "completion enters the readable state");
        enter_command();
        enter_command();
        assert_eq!(current_lifecycle_state(), CareerState::CommandInFlight);
        assert!(reads_currently_unsafe());
        command_select_settled();
        assert!(!reads_currently_unsafe(), "settle returns to the readable state");
        assert!(
            CAPTURE_REQUESTED.load(AtomicOrdering::Acquire),
            "settle holds a request"
        );
    }

    #[test]
    fn shutdown_blocks_scheduling_and_new_requests() {
        let _guard = lock();
        request_capture();
        shutdown();
        assert!(!take_schedule_slot(now_ms()), "no scheduling after shutdown");
        request_capture();
        assert!(
            !CAPTURE_REQUESTED.load(AtomicOrdering::Acquire),
            "requests are refused after shutdown"
        );
    }

    #[test]
    fn view_change_event_moves_to_unsafe_state_and_holds_request() {
        let _guard = lock();
        command_view_play_in_completed();
        CAPTURE_REQUESTED.store(false, AtomicOrdering::Release);
        // Subscribe the same handler the plugin uses, then dispatch VIEW_CHANGE
        // on the services bus (same path as the current-view poll).
        let _ = crate::hooks::subscribe_events();
        honse_services::dispatch_view_change(1620);
        assert_eq!(current_lifecycle_state(), CareerState::CutsceneActive);
        assert!(
            CAPTURE_REQUESTED.load(AtomicOrdering::Acquire),
            "a view change holds a capture request for the next settled state"
        );
        assert!(!take_schedule_slot(now_ms()));
    }

    // ── epoch / dedup rules ────────────────────────────────────────────────

    fn fp(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    #[test]
    fn first_capture_starts_epoch_and_publishes() {
        let mut state = None;
        assert_eq!(
            epoch_decide(&mut state, 1000, 5, fp(1), false),
            CaptureDecision::Publish { epoch_ms: 1000 }
        );
        assert!(state.is_some());
    }

    #[test]
    fn duplicate_content_from_overlapping_edges_publishes_once() {
        let mut state = None;
        assert!(matches!(
            epoch_decide(&mut state, 1000, 5, fp(1), false),
            CaptureDecision::Publish { .. }
        ));
        // The same settled turn re-read via an overlapping edge: suppressed.
        assert_eq!(
            epoch_decide(&mut state, 1500, 5, fp(1), false),
            CaptureDecision::DuplicateContent
        );
    }

    #[test]
    fn changed_same_turn_content_publishes_new_capture() {
        let mut state = None;
        assert!(matches!(
            epoch_decide(&mut state, 1000, 5, fp(1), false),
            CaptureDecision::Publish { .. }
        ));
        // Same turn, different preview content (e.g. training selection state).
        assert_eq!(
            epoch_decide(&mut state, 1500, 5, fp(2), false),
            CaptureDecision::Publish { epoch_ms: 1000 }
        );
    }

    #[test]
    fn turn_advance_publishes_once_within_same_epoch() {
        let mut state = None;
        let _ = epoch_decide(&mut state, 1000, 5, fp(1), false);
        assert_eq!(
            epoch_decide(&mut state, 2000, 6, fp(2), false),
            CaptureDecision::Publish { epoch_ms: 1000 }
        );
        assert_eq!(
            epoch_decide(&mut state, 2500, 6, fp(2), false),
            CaptureDecision::DuplicateContent
        );
    }

    #[test]
    fn turn_rewind_and_deck_change_start_new_epochs() {
        let mut state = None;
        let _ = epoch_decide(&mut state, 1000, 40, fp(1), false);
        // Turn rewind (new career, same deck/card): fresh epoch namespace.
        assert_eq!(
            epoch_decide(&mut state, 5000, 1, fp(1), false),
            CaptureDecision::Publish { epoch_ms: 5000 }
        );
        // Deck change (new career): fresh epoch even with the turn advancing.
        assert_eq!(
            epoch_decide(&mut state, 9000, 2, fp(1), true),
            CaptureDecision::Publish { epoch_ms: 9000 }
        );
    }

    #[test]
    fn capture_ids_are_stable_and_content_addressed() {
        let a = capture_id_for(0x1234, 12, &fp(0xAB));
        assert_eq!(a, "e1234-t12-abababababab");
        // Identical inputs → identical id (retries reuse the same identity).
        assert_eq!(a, capture_id_for(0x1234, 12, &fp(0xAB)));
        // Different content, epoch, or turn → different id.
        assert_ne!(a, capture_id_for(0x1234, 12, &fp(0xCD)));
        assert_ne!(a, capture_id_for(0x9999, 12, &fp(0xAB)));
        assert_ne!(a, capture_id_for(0x1234, 13, &fp(0xAB)));
    }

    #[test]
    fn fingerprint_is_stable_for_identical_content() {
        let turn = pb::SettledTurn {
            capture_id: String::new(),
            captured_at_ms: 0,
            snapshot: Some(pb::CareerSnapshot {
                is_playing: true,
                current_turn: 7,
                speed: 500,
                ..Default::default()
            }),
            extras: Some(pb::CareerExtras {
                skill_points: Some(120),
                ..Default::default()
            }),
        };
        let unchanged = turn.clone();
        assert_eq!(fingerprint(&turn), fingerprint(&unchanged));
        let mut changed = turn;
        if let Some(s) = changed.snapshot.as_mut() {
            s.speed = 501;
        }
        assert_ne!(fingerprint(&changed), fingerprint(&unchanged));
    }

    // ── diag schema (unchanged from t-001) ─────────────────────────────────

    #[test]
    fn diag_line_schema_is_stable() {
        let line = format_diag_line(
            7,
            1234,
            "SetupCommandSelectStart",
            "after_original",
            "command_select_settled",
            CareerState::CommandSelectActive,
            true,
            Some(12),
            None,
            "ThreadId(1)",
        );
        assert_eq!(
            line,
            "seq=7 t_ms=1234 hook=SetupCommandSelectStart phase=after_original reason=command_select_settled state=CommandSelectActive permitted=1 turn=12 view_id=na thread=ThreadId(1)"
        );
    }

    #[test]
    fn diag_line_unsafe_window_shows_state_and_no_turn() {
        let line = format_diag_line(
            8,
            1300,
            "SendCommandAsync",
            "before_original",
            "command_submit",
            CareerState::CommandInFlight,
            false,
            None,
            None,
            "ThreadId(1)",
        );
        assert_eq!(
            line,
            "seq=8 t_ms=1300 hook=SendCommandAsync phase=before_original reason=command_submit state=CommandInFlight permitted=0 turn=na view_id=na thread=ThreadId(1)"
        );
    }

    #[test]
    fn diag_line_event_edge_carries_view_id() {
        let line = format_diag_line(
            9,
            1400,
            "ViewChange",
            "event",
            "view_change",
            CareerState::AssetTransition,
            false,
            None,
            Some(1101),
            "ThreadId(2)",
        );
        assert_eq!(
            line,
            "seq=9 t_ms=1400 hook=ViewChange phase=event reason=view_change state=AssetTransition permitted=0 turn=na view_id=1101 thread=ThreadId(2)"
        );
    }

    #[test]
    fn diag_settle_edge_smoke_without_game_runtime() {
        // Without the game the edge logger must not panic and must not attempt
        // an IL2CPP read (try_read_turn=false on non-settle edges by contract).
        let before = DIAG_SEQ.load(AtomicOrdering::Relaxed);
        diag_settle_edge("SendCommandAsync", "before_original", "command_submit", None, false);
        diag_settle_edge("ViewChange", "event", "view_change", Some(101), false);
        let after = DIAG_SEQ.load(AtomicOrdering::Relaxed);
        assert!(after >= before + 2, "each edge must consume a sequence number");
    }

    #[test]
    fn command_submit_and_settle_round_trip_lifecycle() {
        let _guard = lock();
        crate::reads_on_command_view_play_in_completed();
        assert!(!reads_currently_unsafe());

        crate::suspend_reads_for_command();
        assert_eq!(current_lifecycle_state(), CareerState::CommandInFlight);
        assert!(reads_currently_unsafe());

        crate::resume_reads_on_command_select();
        assert_eq!(current_lifecycle_state(), CareerState::CommandSelectActive);
        assert!(!reads_currently_unsafe());
    }
}
