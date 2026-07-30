//! Event-driven settled-turn capture and telemetry publication.
//!
//! There is no periodic career poll. Passive game edges (command-select
//! rebuild hooks, view changes) *request* a capture; a per-frame atomic pump
//! schedules the held request onto the Unity main thread only while both
//! crash-safety gates are open. The main-thread callback rechecks the gates,
//! resolves the IL2CPP chain lazily, reads one complete career state, and
//! publishes exactly one atomic `SettledTurn` (content-deduplicated, with a
//! stable capture id) through the bounded telemetry transport.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use hachimi_telemetry::{pb, Message};

use crate::compat::Sdk;

use crate::deck_bonuses;
use crate::memory_reader::{self, EvaluationInfo, FiredEvent};

/// Equipped `(deck slot, support_card_id)` pairs from the previous capture.
static PREV_SUPPORT_IDS: Mutex<Vec<(i32, i32)>> = Mutex::new(Vec::new());
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);

// ---------------------------------------------------------------------------
// Crash-safety gates (unchanged semantics; see `read_gate`)
// ---------------------------------------------------------------------------

/// Wall-clock (ms) of the most recent game view change (`event::VIEW_CHANGE`).
/// While a view transition is in flight the game tears down and rebuilds the
/// `WorkSingleModeData → HomeInfo → TurnInfoListDic` objects we walk, so reading
/// them races a use-after-free and crashes the game (surfaces later in the game's
/// own `HomeBgController.CreateBgModel`). We suspend all IL2CPP reads for a cooldown
/// after each view change; intermediate transitions re-arm it. `0` means no change
/// has been observed yet.
static LAST_VIEW_CHANGE_MS: AtomicU64 = AtomicU64::new(0);

/// How long after a view change to keep reads suspended. The training-click
/// `ChangeViewSequence` (fade out → mass asset unload → BG rebuild → fade in) spans
/// well under this window in practice; each intermediate `VIEW_CHANGE` refreshes the
/// timestamp so a chained transition keeps reads suspended until it settles.
const VIEW_TRANSITION_COOLDOWN_MS: u64 = 2000;

/// Whether a view transition is pending confirmation from SetupCommandSelectStart.
/// Set by note_view_change(); cleared by command_select_settled() or safety timeout.
static VIEW_SETTLE_PENDING: AtomicBool = AtomicBool::new(false);

/// Safety ceiling: if SetupCommandSelectStart never fires after a view change
/// (e.g. non-career views, menus), auto-clear the settle gate after this long.
/// Must be longer than the longest observed post-race asset unload (~5-8s).
const VIEW_SETTLE_TIMEOUT_MS: u64 = 15_000;

/// Record that the game changed view. Called from the tracker's `VIEW_CHANGE`
/// subscription (see `hooks.rs`). Suspends reads for [`VIEW_TRANSITION_COOLDOWN_MS`],
/// arms the view-settle gate, and holds a capture request: once the transition
/// settles (all three gates reopen) the pump captures the post-transition state.
/// This is the passive edge that covers career entry/exit and race/story returns
/// that never pass through the command-select hooks. Harmless outside a career —
/// the capture callback no-ops when no career is active.
pub fn note_view_change() {
    LAST_VIEW_CHANGE_MS.store(now_ms(), AtomicOrdering::Relaxed);
    VIEW_SETTLE_PENDING.store(true, AtomicOrdering::Release);
    hlog_info!(target: "settle-diag", "view-settle gate ARMED — waiting for SetupCommandSelectStart or {VIEW_SETTLE_TIMEOUT_MS}ms timeout");
    request_capture();
}

/// Test/inspection helper: wall-clock ms of the last view change (`0` = none).
#[must_use]
pub fn last_view_change_ms() -> u64 {
    LAST_VIEW_CHANGE_MS.load(AtomicOrdering::Relaxed)
}

/// Test helper: whether the combined read gate currently blocks IL2CPP reads.
#[must_use]
pub fn reads_currently_unsafe() -> bool {
    reads_unsafe()
}

/// Pure gate: is a view transition still within its cooldown window? `last == 0`
/// (no view change observed) is never a transition.
#[must_use]
fn is_in_transition(now: u64, last: u64, cooldown_ms: u64) -> bool {
    last != 0 && now.saturating_sub(last) < cooldown_ms
}

/// True while the most recent view change is still inside its cooldown window, i.e.
/// the Single Mode objects may be mid-teardown and unsafe to read.
fn in_view_transition() -> bool {
    is_in_transition(
        now_ms(),
        LAST_VIEW_CHANGE_MS.load(AtomicOrdering::Relaxed),
        VIEW_TRANSITION_COOLDOWN_MS,
    )
}

/// Explicit read-suspend bracketing a career command (training / rest / infirmary /
/// outing). Submitting a command kicks off a coroutine that hits the server, plays
/// an animation, then unloads+rebuilds the Home scene (`Push/PopSceneResourceHash`)
/// — all WITHOUT a `SceneManager.ChangeView`, so [`in_view_transition`] does not
/// cover it. Reading `HomeInfo`/`TurnInfo` during this window races a use-after-free
/// and crashes the game. The command-submit hooks call [`enter_command`]; the
/// command-select rebuild hooks call [`command_select_settled`] after the original
/// method ran. `0` = not suspended.
static SUSPEND_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);

/// Safety ceiling: if the command-select "resume" signal is somehow missed, reads
/// auto-resume after this long so the pump can't wedge on a shut gate forever.
/// Generously covers a full (un-skipped) training animation + asset reload.
const SUSPEND_MAX_MS: u64 = 30_000;

/// Suspend IL2CPP reads until the command-select screen is rebuilt (or the safety
/// deadline elapses). Called from the career command-submit IL2CPP hooks.
/// Idempotent — a double submit merely refreshes the deadline.
pub(crate) fn suspend_reads() {
    SUSPEND_DEADLINE_MS.store(now_ms().saturating_add(SUSPEND_MAX_MS), AtomicOrdering::Relaxed);
}

/// Resume IL2CPP reads. Called once the Single Mode objects are freshly built
/// and safe to read again.
pub(crate) fn resume_reads() {
    SUSPEND_DEADLINE_MS.store(0, AtomicOrdering::Relaxed);
}

/// True while a view change is pending confirmation AND the safety timeout has
/// not elapsed. Auto-clears after [`VIEW_SETTLE_TIMEOUT_MS`] using
/// [`LAST_VIEW_CHANGE_MS`] as the timer base so non-career views that never
/// fire SetupCommandSelectStart don't wedge captures permanently.
#[must_use]
fn view_settle_pending() -> bool {
    if !VIEW_SETTLE_PENDING.load(AtomicOrdering::Acquire) {
        return false;
    }
    let last = LAST_VIEW_CHANGE_MS.load(AtomicOrdering::Relaxed);
    if last == 0 {
        // Defensive: pending without a recorded view change → clear.
        VIEW_SETTLE_PENDING.store(false, AtomicOrdering::Release);
        return false;
    }
    let elapsed = now_ms().saturating_sub(last);
    if elapsed >= VIEW_SETTLE_TIMEOUT_MS {
        // Safety timeout elapsed — clear the gate AND discard any pending
        // capture request. The timeout exists for non-career views where
        // SetupCommandSelectStart never fires; any queued capture from an
        // Apply hook is stale and would read during asset transitions.
        VIEW_SETTLE_PENDING.store(false, AtomicOrdering::Release);
        CAPTURE_REQUESTED.store(false, AtomicOrdering::Release);
        hlog_warn!(target: "settle-diag", "view-settle gate TIMEOUT after {elapsed}ms — auto-cleared + capture request discarded");
        return false;
    }
    true
}

/// True while a command sequence is in flight (reads unsafe). Self-clears once the
/// safety deadline passes so a missed resume can't suspend reads permanently.
fn reads_suspended() -> bool {
    let deadline = SUSPEND_DEADLINE_MS.load(AtomicOrdering::Relaxed);
    deadline != 0 && now_ms() < deadline
}

/// Combined gate: no capture may run whenever the Single Mode objects may be
/// unstable (mid view-transition, or a career command sequence is in flight).
///
/// Routes through [`crate::read_gate`] so the hiker property test constrains the
/// real decision point (not a lookalike). Depth is 0/1 from the deadline flag —
/// same open/closed semantics as the fork's suspend/resume bracketing.
fn reads_unsafe() -> bool {
    !crate::read_gate::reads_permitted(
        in_view_transition(),
        i64::from(reads_suspended()),
        view_settle_pending(),
    )
}

// ---------------------------------------------------------------------------
// Event-driven capture scheduling
// ---------------------------------------------------------------------------

/// Held capture request. Set by passive edges, consumed by the main-thread
/// capture callback. Requests made while a gate is shut stay held (coalescing
/// into one), so a settled turn is never dropped just because its edge fired
/// inside an unsafe window.
static CAPTURE_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Whether a main-thread capture callback is currently scheduled/in flight.
static CAPTURE_SCHEDULED: AtomicBool = AtomicBool::new(false);
/// Wall-clock (ms) when the in-flight callback was scheduled; drives the
/// staleness watchdog so a callback scheduled but never run to completion
/// can't wedge `CAPTURE_SCHEDULED` true and freeze capturing.
static SCHEDULED_SINCE_MS: AtomicU64 = AtomicU64::new(0);

/// If a scheduled capture hasn't completed within this window, treat it as lost
/// and allow a fresh one to be scheduled.
const SCHEDULE_STALE_MS: u64 = 5000;

/// A career command was submitted (training / rest / outing / infirmary /
/// race...). Shuts the command gate idempotently; the matching settle edge
/// reopens it.
pub(crate) fn enter_command() {
    suspend_reads();
}

/// The command-select screen finished rebuilding (the original
/// `SetupCommandSelectStart*` already ran): the turn has settled. Reopen the
/// command gate, clear the view-settle gate, and hold a capture request for
/// the pump.
pub(crate) fn command_select_settled() {
    let was_pending = VIEW_SETTLE_PENDING.swap(false, AtomicOrdering::AcqRel);
    resume_reads();
    if was_pending {
        let held_ms = now_ms().saturating_sub(LAST_VIEW_CHANGE_MS.load(AtomicOrdering::Relaxed));
        hlog_info!(target: "settle-diag", "view-settle gate CLEARED by SetupCommandSelectStart (held {held_ms}ms)");
    }
    request_capture();
}



/// Hold/coalesce a capture request. Never reads IL2CPP, never blocks — safe
/// from any thread. The pump schedules it once both gates are open.
pub(crate) fn request_capture() {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        return;
    }
    CAPTURE_REQUESTED.store(true, AtomicOrdering::Release);
}

/// Decide whether the pump should schedule the capture callback now, claiming
/// the schedule slot when it does. Atomic bookkeeping only (plus the gate
/// check) — the actual scheduling side effect stays in [`tick`] so this
/// decision point is deterministic under test.
fn take_schedule_slot(now: u64) -> bool {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        return false;
    }
    if !CAPTURE_REQUESTED.load(AtomicOrdering::Acquire) {
        return false;
    }
    // Both crash-safety gates must be open before we even schedule; the
    // callback rechecks on the main thread (defense in depth).
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

/// Per-frame atomic pump called from the host frame callback. Schedules the
/// held capture request onto the Unity main thread once all three gates are open.
/// No IL2CPP access and no career read happens here — a per-frame pump is
/// allowed, a per-frame career read is not.
///
/// Captures are event-driven only (settled-turn hooks + view-change edges).
/// Periodic polling is intentionally absent: IL2CPP reads take ~80ms and asset
/// unloading can start on another thread mid-read, so timer-based captures
/// race use-after-free regardless of gate checks at schedule time.
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
    // Defense in depth: a capture scheduled just before a view change or a
    // command submit can still be dispatched inside the unsafe window. Bail
    // before any IL2CPP read touches teardown-time objects — the request stays
    // held, and the pump reschedules once the gates reopen.
    if reads_unsafe() {
        let vt = in_view_transition();
        let cs = reads_suspended();
        let vs = view_settle_pending();
        hlog_debug!(target: "settle-diag",
            "capture DEFERRED — gates: view_cooldown={vt} cmd_suspended={cs} settle_pending={vs}");
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
//   hook=<name>      SendCommandAsync | CommonSendCommandAsync |
//                    SetupCommandSelectStart | SetupCommandSelectStartStepTurn |
//                    ViewChange
//   phase=<p>        before_original | after_original | event
//   reason=<r>       command_submit | command_select_settled | view_change
//   view_gate=<g>    open | cooldown   (view-transition gate at log time)
//   cmd_gate=<g>     open | suspended  (command-suspend gate at log time)
//   permitted=<0|1>  combined read_gate::reads_permitted at log time
//   turn=<i32|na>    GetCurrentTurn — read ONLY when permitted=1 on a
//                    post-original settle edge; na means "not read"
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
    view_cooldown_active: bool,
    cmd_suspended: bool,
    settle_pending: bool,
    permitted: bool,
    turn: Option<i32>,
    view_id: Option<i32>,
    thread: &str,
) -> String {
    let view_gate = if view_cooldown_active { "cooldown" } else { "open" };
    let cmd_gate = if cmd_suspended { "suspended" } else { "open" };
    let settle_gate_str = if settle_pending { "pending" } else { "settled" };
    let turn = turn.map_or_else(|| "na".to_owned(), |t| t.to_string());
    let view_id = view_id.map_or_else(|| "na".to_owned(), |v| v.to_string());
    format!(
        "seq={seq} t_ms={t_ms} hook={hook} phase={phase} reason={reason} view_gate={view_gate} cmd_gate={cmd_gate} settle_gate={settle_gate_str} permitted={} turn={turn} view_id={view_id} thread={thread}",
        u8::from(permitted)
    )
}

/// Log one settled-turn diagnostic edge.
///
/// `try_read_turn` may be true only on post-original settle edges running on the
/// Unity main thread (the `SetupCommandSelectStart*` hooks after the original
/// returned). Even then the turn is read strictly behind the same two-gate check
/// production reads use — an edge inside an unsafe window logs `turn=na` instead
/// of reading.
pub(crate) fn diag_settle_edge(
    hook: &'static str,
    phase: &'static str,
    reason: &'static str,
    view_id: Option<i32>,
    try_read_turn: bool,
) {
    let seq = DIAG_SEQ.fetch_add(1, AtomicOrdering::Relaxed);
    let view_cooldown_active = in_view_transition();
    let cmd_suspended = reads_suspended();
    let settle_pending = view_settle_pending();
    let permitted = crate::read_gate::reads_permitted(view_cooldown_active, i64::from(cmd_suspended), settle_pending);
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
        view_cooldown_active,
        cmd_suspended,
        settle_pending,
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
    LAST_VIEW_CHANGE_MS.store(0, AtomicOrdering::Release);
    SUSPEND_DEADLINE_MS.store(0, AtomicOrdering::Release);
    VIEW_SETTLE_PENDING.store(false, AtomicOrdering::Release);
    disarm_view_poll();
    if let Ok(mut guard) = EPOCH.lock() {
        *guard = None;
    }
    reset_career_state();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes tests that touch the module's global gate/schedule state.
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    /// Reset every global this module owns so each test starts from a clean,
    /// non-shutdown state (some tests flip `SHUTTING_DOWN`).
    fn reset_state() {
        SHUTTING_DOWN.store(false, AtomicOrdering::Release);
        CAPTURE_REQUESTED.store(false, AtomicOrdering::Release);
        CAPTURE_SCHEDULED.store(false, AtomicOrdering::Release);
        SCHEDULED_SINCE_MS.store(0, AtomicOrdering::Release);
        LAST_VIEW_CHANGE_MS.store(0, AtomicOrdering::Release);
        SUSPEND_DEADLINE_MS.store(0, AtomicOrdering::Release);
        VIEW_SETTLE_PENDING.store(false, AtomicOrdering::Release);
        VIEW_POLL_ARMED.store(false, AtomicOrdering::Release);
        *EPOCH.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }

    fn lock() -> std::sync::MutexGuard<'static, ()> {
        let guard = TEST_GUARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_state();
        guard
    }

    #[test]
    fn transition_gate_open_only_within_cooldown() {
        // No view change observed yet → never a transition.
        assert!(!is_in_transition(10_000, 0, VIEW_TRANSITION_COOLDOWN_MS));
        // Just changed → suspended.
        assert!(is_in_transition(10_000, 10_000, VIEW_TRANSITION_COOLDOWN_MS));
        // Inside the cooldown → still suspended.
        assert!(is_in_transition(11_999, 10_000, VIEW_TRANSITION_COOLDOWN_MS));
        // Exactly at / past the cooldown → reads resume.
        assert!(!is_in_transition(12_000, 10_000, VIEW_TRANSITION_COOLDOWN_MS));
        assert!(!is_in_transition(20_000, 10_000, VIEW_TRANSITION_COOLDOWN_MS));
    }

    #[test]
    fn requests_coalesce_into_one_schedule_slot() {
        let _guard = lock();
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
        enter_command(); // command gate shuts
        request_capture();
        assert!(!take_schedule_slot(now_ms()), "no scheduling while a gate is shut");
        assert!(
            CAPTURE_REQUESTED.load(AtomicOrdering::Acquire),
            "the request is retained, not dropped"
        );
        command_select_settled(); // gate reopens (also re-requests)
        assert!(take_schedule_slot(now_ms()), "pump schedules once the gates open");
    }

    #[test]
    fn lost_schedule_slot_is_reclaimed_after_staleness_window() {
        let _guard = lock();
        request_capture();
        let t0 = now_ms();
        assert!(take_schedule_slot(t0));
        // The callback never completed. Within the window: coalesce.
        assert!(!take_schedule_slot(t0 + SCHEDULE_STALE_MS - 1));
        // Past the window: reclaim so capturing cannot wedge forever.
        assert!(take_schedule_slot(t0 + SCHEDULE_STALE_MS));
    }

    #[test]
    fn double_submit_is_idempotent_and_settle_reopens_gate() {
        let _guard = lock();
        assert!(!reads_currently_unsafe(), "both gates open initially");
        enter_command();
        enter_command(); // double submit merely refreshes the deadline
        assert!(reads_currently_unsafe(), "command gate shut");
        command_select_settled();
        assert!(!reads_currently_unsafe(), "settle edge reopens the gate");
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
    fn view_change_event_updates_timestamp_and_holds_request() {
        let _guard = lock();
        assert_eq!(last_view_change_ms(), 0);
        // Subscribe the same handler the plugin uses, then dispatch VIEW_CHANGE
        // on the services bus (same path as SceneManager hook → event bus).
        let _ = crate::hooks::subscribe_events();
        honse_services::dispatch_view_change(42);
        assert!(last_view_change_ms() > 0, "VIEW_CHANGE must update LAST_VIEW_CHANGE_MS");
        assert!(
            CAPTURE_REQUESTED.load(AtomicOrdering::Acquire),
            "a view change holds a capture request for the settled state"
        );
        assert!(
            !take_schedule_slot(now_ms()),
            "the cooldown gate holds the request until the transition settles"
        );
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
            false,
            false,
            false,
            true,
            Some(12),
            None,
            "ThreadId(1)",
        );
        assert_eq!(
            line,
            "seq=7 t_ms=1234 hook=SetupCommandSelectStart phase=after_original reason=command_select_settled view_gate=open cmd_gate=open settle_gate=settled permitted=1 turn=12 view_id=na thread=ThreadId(1)"
        );
    }

    #[test]
    fn diag_line_unsafe_window_shows_closed_gates_and_no_turn() {
        let line = format_diag_line(
            8,
            1300,
            "SendCommandAsync",
            "before_original",
            "command_submit",
            true,
            true,
            false,
            false,
            None,
            None,
            "ThreadId(1)",
        );
        assert_eq!(
            line,
            "seq=8 t_ms=1300 hook=SendCommandAsync phase=before_original reason=command_submit view_gate=cooldown cmd_gate=suspended settle_gate=settled permitted=0 turn=na view_id=na thread=ThreadId(1)"
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
            true,
            false,
            true,
            false,
            None,
            Some(1101),
            "ThreadId(2)",
        );
        assert_eq!(
            line,
            "seq=9 t_ms=1400 hook=ViewChange phase=event reason=view_change view_gate=cooldown cmd_gate=open settle_gate=pending permitted=0 turn=na view_id=1101 thread=ThreadId(2)"
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
    fn suspend_resume_round_trip_closes_then_opens_gate() {
        let _guard = lock();
        assert!(!reads_currently_unsafe(), "both gates open → reads safe");

        crate::suspend_reads_for_command();
        assert!(reads_currently_unsafe(), "suspend must close the gate");

        crate::resume_reads_on_command_select();
        assert!(!reads_currently_unsafe(), "resume must re-open the gate");
    }
}
