//! Unified cache for overlay career data.
//!
//! All IL2CPP reads run on the Unity main thread on a ~2s cadence (or immediately
//! when tracking starts). The render thread only clones from [`CACHE`].

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering as AtomicOrdering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// Only the real (non-harness) refresh path schedules work on the host main thread.
#[cfg(not(feature = "dev-harness"))]
use crate::compat::Sdk;

use crate::deck_bonuses;
use crate::memory_reader::{
    self, AcquiredSkillInfo, CareerSnapshot, EvaluationInfo, FiredEvent,
};
use crate::skill_shop::{self, SkillShopEntry};

/// Refresh interval while memory tracking is on (milliseconds).
pub const AUTO_REFRESH_INTERVAL_MS: u64 = 500;

#[derive(Default)]
struct OverlayCache {
    snapshot: Option<CareerSnapshot>,
    skills: Vec<AcquiredSkillInfo>,
    evaluations: Vec<EvaluationInfo>,
    skill_shop: Vec<SkillShopEntry>,
    skill_points: Option<i32>,
    /// Equipped `(deck slot, support_card_id)` map, captured once per career.
    support_ids: Vec<(i32, i32)>,
}

static CACHE: Mutex<OverlayCache> = Mutex::new(OverlayCache {
    snapshot: None,
    skills: Vec::new(),
    evaluations: Vec::new(),
    skill_shop: Vec::new(),
    skill_points: None,
    support_ids: Vec::new(),
});
static PENDING: AtomicBool = AtomicBool::new(false);
/// Wall-clock (ms) when the in-flight refresh was scheduled; drives the staleness
/// watchdog so a callback scheduled but never run to completion can't wedge
/// `PENDING` true and freeze the overlay on "Loading career data".
static PENDING_SINCE_MS: AtomicU64 = AtomicU64::new(0);
static LAST_REFRESH_MS: AtomicU64 = AtomicU64::new(0);
static SHUTTING_DOWN: AtomicBool = AtomicBool::new(false);
static CHARACTER_READY: AtomicBool = AtomicBool::new(false);

/// If a scheduled refresh hasn't completed within this window, treat it as lost
/// and allow a fresh one to be scheduled.
const PENDING_STALE_MS: u64 = 5000;

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

/// Record that the game changed view. Called from the tracker's `VIEW_CHANGE`
/// subscription (see `hooks.rs`). Suspends reads for [`VIEW_TRANSITION_COOLDOWN_MS`].
pub fn note_view_change() {
    LAST_VIEW_CHANGE_MS.store(now_ms(), AtomicOrdering::Relaxed);
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
/// and crashes the game. The command-submit hooks call [`suspend_reads`]; the
/// command-select rebuild hooks call [`resume_reads`]. `0` = not suspended.
static SUSPEND_DEADLINE_MS: AtomicU64 = AtomicU64::new(0);

/// Safety ceiling: if the command-select "resume" signal is somehow missed, reads
/// auto-resume after this long so the overlay can't wedge on stale data forever.
/// Generously covers a full (un-skipped) training animation + asset reload.
const SUSPEND_MAX_MS: u64 = 30_000;

/// Suspend IL2CPP reads until the command-select screen is rebuilt (or the safety
/// deadline elapses). Called from the career command-submit IL2CPP hooks.
pub(crate) fn suspend_reads() {
    SUSPEND_DEADLINE_MS.store(now_ms().saturating_add(SUSPEND_MAX_MS), AtomicOrdering::Relaxed);
}

/// Resume IL2CPP reads. Called from the command-select rebuild IL2CPP hooks once
/// the Single Mode objects are freshly built and safe to read again.
pub(crate) fn resume_reads() {
    SUSPEND_DEADLINE_MS.store(0, AtomicOrdering::Relaxed);
}

/// True while a command sequence is in flight (reads unsafe). Self-clears once the
/// safety deadline passes so a missed resume can't suspend reads permanently.
fn reads_suspended() -> bool {
    let deadline = SUSPEND_DEADLINE_MS.load(AtomicOrdering::Relaxed);
    deadline != 0 && now_ms() < deadline
}

/// Combined gate: skip a refresh whenever the Single Mode objects may be unstable
/// (mid view-transition, or a career command sequence is in flight).
///
/// Routes through [`crate::read_gate`] so the hiker property test constrains the
/// real decision point (not a lookalike). Depth is 0/1 from the deadline flag —
/// same open/closed semantics as the fork's suspend/resume bracketing.
fn reads_unsafe() -> bool {
    !crate::read_gate::reads_permitted(in_view_transition(), i64::from(reads_suspended()))
}

pub(crate) fn character_ready() -> bool {
    CHARACTER_READY.load(AtomicOrdering::Relaxed)
}

pub(crate) fn reset_career_state() {
    CHARACTER_READY.store(false, AtomicOrdering::Relaxed);
    EVAL_DIAG_LOGGED.store(false, AtomicOrdering::Relaxed);
    crate::bond_progress::clear();
    deck_bonuses::clear();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = OverlayCache::default();
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Whether an auto-refresh should be requested (pure logic for tests).
#[must_use]
pub fn should_auto_refresh(elapsed_ms: u64, interval_ms: u64) -> bool {
    elapsed_ms >= interval_ms
}

fn elapsed_since_last_refresh_ms() -> u64 {
    let last = LAST_REFRESH_MS.load(AtomicOrdering::Relaxed);
    if last == 0 {
        return u64::MAX;
    }
    now_ms().saturating_sub(last)
}

fn schedule_refresh() {
    // The desktop dev-harness has no SDK / Unity main thread; data is injected once
    // via `set_test_data`. Never schedule a real IL2CPP refresh there.
    #[cfg(feature = "dev-harness")]
    {
        return;
    }
    #[cfg(not(feature = "dev-harness"))]
    {
        schedule_refresh_inner();
    }
}

#[cfg(not(feature = "dev-harness"))]
fn schedule_refresh_inner() {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        return;
    }
    // Suspend scheduling while the Single Mode objects may be mid-teardown (view
    // transition or an in-flight career command sequence): reading them races a
    // use-after-free (crashes the game). The throttled loop retries once safe.
    if reads_unsafe() {
        return;
    }
    if PENDING.swap(true, AtomicOrdering::AcqRel) {
        // Already pending: coalesce, unless the in-flight refresh looks lost
        // (scheduled long ago, never completed) — then fall through to reschedule.
        let since = PENDING_SINCE_MS.load(AtomicOrdering::Relaxed);
        if since == 0 || now_ms().saturating_sub(since) < PENDING_STALE_MS {
            return;
        }
    }
    PENDING_SINCE_MS.store(now_ms(), AtomicOrdering::Relaxed);
    Sdk::get().schedule_on_main_thread(refresh_cache_cb);
}

extern "C" fn refresh_cache_cb() {
    if SHUTTING_DOWN.load(AtomicOrdering::Acquire) {
        PENDING.store(false, AtomicOrdering::Release);
        return;
    }
    // Run the (panic-prone) IL2CPP reads + telemetry behind a catch so a single
    // bad frame can never unwind across this `extern "C"` boundary nor wedge the
    // refresh loop. If we didn't reset PENDING here, a panic mid-callback would
    // leave PENDING stuck `true`, blocking every future refresh and freezing the
    // overlay on "Loading career data\u2026" (see the get_Character-null career-start
    // window). PENDING/LAST_REFRESH are always restored, panic or not.
    // Defense in depth: a refresh scheduled just before a view change can still be
    // dispatched mid-transition. Bail before any IL2CPP read touches teardown-time
    // objects; the next throttled tick retries after the cooldown.
    if reads_unsafe() {
        PENDING.store(false, AtomicOrdering::Release);
        LAST_REFRESH_MS.store(now_ms(), AtomicOrdering::Relaxed);
        return;
    }
    if let Err(e) = std::panic::catch_unwind(refresh_cache_inner) {
        let msg = e
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| e.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic>".to_string());
        hlog_error!("refresh_cache_cb PANICKED: {msg} \u{2014} overlay refresh recovered for next frame");
    }
    LAST_REFRESH_MS.store(now_ms(), AtomicOrdering::Relaxed);
    PENDING.store(false, AtomicOrdering::Release);
}

fn refresh_cache_inner() {
    // Tracking is fully manual: only the user's Start/Stop control (menu button or
    // hotkey) toggles `TRACKING`. No automatic start/stop — a manual stop sticks.
    if !memory_reader::TRACKING.load(AtomicOrdering::Relaxed) {
        return;
    }

    let Some(chara) = memory_reader::get_chara_ptr() else {
        // Tracking on but Character not ready yet (career setup window) — wait.
        CHARACTER_READY.store(false, AtomicOrdering::Relaxed);
        return;
    };
    CHARACTER_READY.store(true, AtomicOrdering::Relaxed);

    let mut snapshot = memory_reader::read_snapshot();
    let is_playing = snapshot.as_ref().is_some_and(|s| s.is_playing);
    if !is_playing {
        // Not in a career (e.g. left to the lobby). Leave tracking as the user set
        // it; just skip publishing until a career is active again.
        return;
    }

    let skills = memory_reader::read_acquired_skills();
    let evaluations = memory_reader::read_evaluations();
    let skill_points = skill_shop::read_skill_points();
    let skill_shop = skill_shop::read_skill_shop();

    // Equipped support-card ids: re-read every refresh (pure ObscuredInt field reads,
    // no Convert). Cheap, and avoids stale deck mapping when the game keeps SingleMode
    // "playing" across a career -> new-career transition.
    let support_ids = if is_playing {
        memory_reader::read_equipped_support_ids()
    } else {
        Vec::new()
    };
    // Deck change (new career / reshuffled deck) invalidates per-career progress and
    // the once-per-career deck-bonus capture. Detect by comparing to the prior deck.
    if is_playing {
        let prev = CACHE.lock().ok().map(|g| g.support_ids.clone()).unwrap_or_default();
        // Require both non-empty so a transient empty read can't wipe progress mid-career.
        if !prev.is_empty() && !support_ids.is_empty() && prev != support_ids {
            crate::bond_progress::clear();
            deck_bonuses::clear(); // re-captured below via try_capture
            EVAL_DIAG_LOGGED.store(false, AtomicOrdering::Relaxed);
        }
    }
    // Fired-event history: re-read each refresh (read-only; grows over the career).
    let fired_events = if is_playing {
        memory_reader::read_fired_events()
    } else {
        Vec::new()
    };
    // Accumulate observed events into per-career progress (auto counter).
    if is_playing {
        crate::bond_progress::observe(&support_ids, &fired_events);
    } else {
        crate::bond_progress::clear();
    }
    if is_playing {
        deck_bonuses::try_capture(chara);
        // Self-computed evaluation estimate from stats + skills + aptitudes.
        if let Some(s) = snapshot.as_mut() {
            let stats = [s.speed, s.stamina, s.power, s.guts, s.wiz];
            s.evaluation_value =
                crate::evaluation::compute(stats, &s.aptitudes, s.star, &skills);
        }
        log_career_diagnostic(&evaluations, &support_ids, &fired_events);
    } else {
        deck_bonuses::clear();
        EVAL_DIAG_LOGGED.store(false, AtomicOrdering::Relaxed);
    }

    // Player-reserved races (the in-game agenda) for telemetry only — not cached,
    // since the overlay UI does not surface it. Cheap POD reads, career-gated.
    let reserved_races = if is_playing {
        memory_reader::read_reserved_races()
    } else {
        Vec::new()
    };

    // Populate the overlay cache FIRST, so the UI always has fresh data even if
    // the side-channel telemetry below panics. Clone the bits telemetry needs.
    let snap_for_pub = snapshot.clone();
    if let Ok(mut guard) = CACHE.lock() {
        guard.snapshot = snapshot;
        guard.skills = skills.clone();
        guard.evaluations = evaluations.clone();
        guard.skill_shop = skill_shop.clone();
        guard.skill_points = skill_points;
        guard.support_ids = support_ids.clone();
    }

    // Side-channel telemetry (no-op when disabled). Runs after the cache store so a
    // telemetry failure can't stall the overlay; the outer catch_unwind contains it.
    crate::telemetry::publish(
        snap_for_pub.as_ref(),
        &skills,
        &evaluations,
        &skill_shop,
        skill_points,
        &support_ids,
        &reserved_races,
    );
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
        let name =
            crate::gametora_data::support_card_name(*support_id as i64).unwrap_or("?");
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

/// Throttled auto-refresh (call from render thread each overlay frame).
pub fn maybe_request_refresh() {
    // Manual only: refresh solely while the user has tracking on. Stopped = silent.
    if !memory_reader::TRACKING.load(AtomicOrdering::Relaxed) {
        return;
    }
    if !should_auto_refresh(elapsed_since_last_refresh_ms(), AUTO_REFRESH_INTERVAL_MS) {
        return;
    }
    schedule_refresh();
}

/// Immediate refresh when tracking starts — bypasses interval, still coalesced.
pub fn request_refresh_immediate() {
    if !memory_reader::TRACKING.load(AtomicOrdering::Relaxed) {
        return;
    }
    schedule_refresh();
}

pub fn snapshot() -> Option<CareerSnapshot> {
    CACHE.lock().ok().and_then(|g| g.snapshot.clone())
}

#[allow(dead_code)]
pub fn skills() -> Vec<AcquiredSkillInfo> {
    CACHE.lock().ok().map(|g| g.skills.clone()).unwrap_or_default()
}

pub fn evaluations() -> Vec<EvaluationInfo> {
    CACHE.lock().ok().map(|g| g.evaluations.clone()).unwrap_or_default()
}

/// Equipped `(deck slot, support_card_id)` pairs for the active career.
pub fn equipped_support_ids() -> Vec<(i32, i32)> {
    CACHE.lock().ok().map(|g| g.support_ids.clone()).unwrap_or_default()
}

pub fn skill_shop() -> Vec<SkillShopEntry> {
    CACHE.lock().ok().map(|g| g.skill_shop.clone()).unwrap_or_default()
}

pub fn skill_points() -> Option<i32> {
    CACHE.lock().ok().and_then(|g| g.skill_points)
}

/// Stop scheduling refreshes and bail out of any in-flight main-thread callback.
/// Call from the plugin `SHUTDOWN` handler before the host frees the DLL.
pub fn shutdown() {
    SHUTTING_DOWN.store(true, AtomicOrdering::Release);
    PENDING.store(false, AtomicOrdering::Release);
    PENDING_SINCE_MS.store(0, AtomicOrdering::Release);
    LAST_REFRESH_MS.store(0, AtomicOrdering::Release);
    LAST_VIEW_CHANGE_MS.store(0, AtomicOrdering::Release);
    SUSPEND_DEADLINE_MS.store(0, AtomicOrdering::Release);
    reset_career_state();
}

/// Inject fully-formed overlay data for the desktop dev-harness, bypassing all
/// IL2CPP reads. The overlay's `snapshot()/skills()/evaluations()/...` accessors
/// then return exactly this, so the UI can be rendered in a plain eframe window.
#[cfg(feature = "dev-harness")]
#[allow(clippy::too_many_arguments)]
pub(crate) fn set_test_data(
    snapshot: CareerSnapshot,
    skills: Vec<AcquiredSkillInfo>,
    evaluations: Vec<EvaluationInfo>,
    skill_shop: Vec<SkillShopEntry>,
    skill_points: Option<i32>,
    support_ids: Vec<(i32, i32)>,
) {
    if let Ok(mut guard) = CACHE.lock() {
        guard.snapshot = Some(snapshot);
        guard.skills = skills;
        guard.evaluations = evaluations;
        guard.skill_shop = skill_shop;
        guard.skill_points = skill_points;
        guard.support_ids = support_ids;
    }
    // Mark a refresh as just-completed so the throttle never tries to schedule one.
    LAST_REFRESH_MS.store(now_ms(), AtomicOrdering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_auto_refresh_respects_interval() {
        assert!(!should_auto_refresh(0, AUTO_REFRESH_INTERVAL_MS));
        assert!(!should_auto_refresh(499, AUTO_REFRESH_INTERVAL_MS));
        assert!(should_auto_refresh(500, AUTO_REFRESH_INTERVAL_MS));
        assert!(should_auto_refresh(3000, AUTO_REFRESH_INTERVAL_MS));
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
    fn shutdown_blocks_refresh_scheduling() {
        shutdown();
        schedule_refresh();
        assert!(!PENDING.load(AtomicOrdering::Relaxed));
    }
}
