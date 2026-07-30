//! Background publisher: a bounded FIFO queue drained by one sender thread.
//!
//! `publish()` is called from the Unity main thread and plugin hooks, so it must never block. It encodes + enqueues onto a `SyncSender` via `try_send` (drop-on-full, counted) and returns immediately. The sender thread performs the blocking HTTP POSTs. A transiently failed POST retains the head job and retries the identical body after a backoff pause; later jobs stay queued in FIFO order and are never drained or dropped by the backoff. A *rejected* POST (4xx — bad token, malformed payload) can never succeed on retry, so the body is dropped and counted instead of retried forever.
//!
//! Durability model: durable at-least-once delivery starts only after the sidecar commits a payload. This in-process queue is process-lifetime best effort — under an unlimited backend outage the head job retries forever (while the process lives), the queue fills, and further enqueues are dropped with the `dropped_queue_full` counter. Shutdown is bounded: it stops accepting new jobs, interrupts any backoff wait, abandons the best-effort backlog, and never retries forever.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::config::{BearerToken, Config, Endpoint};
use crate::transport::{self, PostError};

/// Queue depth. Bounded so a dead backend cannot grow memory without limit, yet
/// sized with at least one full career of headroom: a career settles ~78 turns,
/// so 256 holds a whole career of settled-turn payloads (plus legacy
/// snapshot/extras bursts) more than 3x over before anything is dropped.
const QUEUE_CAP: usize = 256;
/// Pause between retry attempts for the failed head job. Later jobs stay
/// queued (FIFO) while the head backs off; nothing is drained or dropped.
const BACKOFF: Duration = Duration::from_secs(2);
const CONTENT_TYPE: &str = "application/x-protobuf";

/// A ready-to-send, pre-encoded protobuf body. Retained unchanged across
/// retries so at-least-once redelivery carries the identical bytes.
type Job = Vec<u8>;

/// Delivery counters (diagnostics). One global instance backs the public
/// metrics; tests construct their own so assertions stay deterministic.
struct Counters {
    sent: AtomicU64,
    retried: AtomicU64,
    queue_full: AtomicU64,
    disconnected: AtomicU64,
    rejected: AtomicU64,
}

impl Counters {
    const fn new() -> Self {
        Self {
            sent: AtomicU64::new(0),
            retried: AtomicU64::new(0),
            queue_full: AtomicU64::new(0),
            disconnected: AtomicU64::new(0),
            rejected: AtomicU64::new(0),
        }
    }

    fn snapshot(&self) -> Metrics {
        Metrics {
            sent: self.sent.load(Ordering::Relaxed),
            retried: self.retried.load(Ordering::Relaxed),
            dropped_queue_full: self.queue_full.load(Ordering::Relaxed),
            dropped_disconnected: self.disconnected.load(Ordering::Relaxed),
            dropped_rejected: self.rejected.load(Ordering::Relaxed),
        }
    }
}

static COUNTERS: Counters = Counters::new();

/// Point-in-time snapshot of the delivery counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Metrics {
    /// Bodies POSTed successfully (2xx).
    pub sent: u64,
    /// Failed POST attempts that were scheduled for a retry of the same body.
    pub retried: u64,
    /// Enqueues dropped because the bounded queue was full.
    pub dropped_queue_full: u64,
    /// Enqueues dropped because no publisher was running (not started, or
    /// already stopped/disconnected).
    pub dropped_disconnected: u64,
    /// Bodies dropped because the sidecar permanently rejected them (4xx);
    /// retrying an identical rejected body can never succeed.
    pub dropped_rejected: u64,
}

/// Shutdown latch shared with the sender thread. Interrupts a backoff wait
/// immediately so `stop()` never waits out a full backoff window.
#[derive(Default)]
struct StopSignal {
    stopped: Mutex<bool>,
    cvar: Condvar,
}

impl StopSignal {
    fn stop(&self) {
        let mut stopped = self.stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        *stopped = true;
        drop(stopped);
        self.cvar.notify_all();
    }

    fn is_stopped(&self) -> bool {
        *self.stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Block up to `dur` or until stopped, whichever comes first. Returns
    /// whether stop was signaled.
    fn wait_backoff(&self, dur: Duration) -> bool {
        let stopped = self.stopped.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (stopped, _) = self
            .cvar
            .wait_timeout_while(stopped, dur, |stopped| !*stopped)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *stopped
    }
}

struct Inner {
    tx: SyncSender<Job>,
    handle: Option<JoinHandle<()>>,
    stop: Arc<StopSignal>,
}

static PUBLISHER: Mutex<Option<Inner>> = Mutex::new(None);

/// Start the sender thread for `endpoint`, attaching `token` as the bearer
/// credential on every POST. Idempotent-ish: replaces any existing publisher
/// (callers should `stop()` first in practice).
pub fn start(endpoint: Endpoint, token: Option<BearerToken>) {
    let (tx, rx) = sync_channel::<Job>(QUEUE_CAP);
    let stop = Arc::new(StopSignal::default());
    let stop_for_thread = Arc::clone(&stop);
    let handle = std::thread::Builder::new()
        .name("hachimi-telemetry".to_string())
        .spawn(move || {
            let mut post = |body: &[u8]| transport::post(&endpoint, token.as_ref(), CONTENT_TYPE, body);
            run_sender(&rx, &mut post, BACKOFF, &stop_for_thread, &COUNTERS);
        })
        .ok();
    let mut guard = PUBLISHER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(Inner { tx, handle, stop });
}

/// Enqueue an already-encoded body. Never blocks: `try_send` drops on a full
/// queue (counted), and a missing/stopped publisher drops too (counted).
pub fn enqueue(body: Job) {
    let guard = PUBLISHER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match guard.as_ref() {
        Some(inner) => try_enqueue(&inner.tx, body, &COUNTERS),
        None => {
            COUNTERS.disconnected.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Non-blocking send with per-cause drop accounting.
fn try_enqueue(tx: &SyncSender<Job>, body: Job, counters: &Counters) {
    match tx.try_send(body) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            counters.queue_full.fetch_add(1, Ordering::Relaxed);
        }
        Err(TrySendError::Disconnected(_)) => {
            counters.disconnected.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Stop the sender thread and join it. Bounded: the stop signal interrupts any
/// backoff wait, the queue backlog is abandoned (best effort), and at most one
/// already-in-flight POST (itself bounded by the transport timeouts) completes.
pub fn stop() {
    let inner = {
        let mut guard = PUBLISHER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.take()
    };
    if let Some(mut inner) = inner {
        inner.stop.stop();
        // Dropping the only sender disconnects the channel; an idle loop exits.
        drop(std::mem::replace(&mut inner.tx, sync_channel::<Job>(1).0));
        if let Some(h) = inner.handle.take() {
            let _ = h.join();
        }
    }
}

/// Snapshot of the delivery counters.
#[must_use]
pub fn metrics() -> Metrics {
    COUNTERS.snapshot()
}

/// Total envelopes dropped without a send attempt (full queue + no publisher).
#[must_use]
pub fn dropped_count() -> u64 {
    let m = COUNTERS.snapshot();
    m.dropped_queue_full + m.dropped_disconnected
}

/// The sender state machine. Consumes jobs in FIFO order; a transiently failed
/// head job is retained and the identical body is retried after `backoff` —
/// later jobs stay buffered in the bounded channel untouched. A permanently
/// rejected body (4xx) is dropped and counted, because retrying identical
/// rejected bytes can never succeed. Retrying stops only on success, rejection,
/// or shutdown; `stop` also interrupts the backoff wait and abandons the
/// backlog so shutdown stays bounded.
fn run_sender(
    rx: &Receiver<Job>,
    post: &mut dyn FnMut(&[u8]) -> Result<(), PostError>,
    backoff: Duration,
    stop: &StopSignal,
    counters: &Counters,
) {
    while let Ok(body) = rx.recv() {
        if stop.is_stopped() {
            return; // bounded shutdown: abandon the best-effort backlog
        }
        loop {
            match post(&body) {
                Ok(()) => {
                    counters.sent.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                Err(PostError::Rejected(_)) => {
                    counters.rejected.fetch_add(1, Ordering::Relaxed);
                    break; // drop this body; move on to the next job
                }
                Err(PostError::Transient(_)) => {
                    counters.retried.fetch_add(1, Ordering::Relaxed);
                    if stop.wait_backoff(backoff) {
                        return;
                    }
                }
            }
        }
    }
}

/// Resolve the parsed endpoint from a `Config`, or `None` if unusable.
#[must_use]
pub fn endpoint_from(cfg: &Config) -> Option<Endpoint> {
    Endpoint::parse(&cfg.endpoint)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    /// fail/fail/success must POST the identical body three times, and later
    /// jobs must follow in FIFO order untouched by the backoff.
    #[test]
    fn retry_resends_identical_body_then_preserves_fifo_order() {
        let (tx, rx) = sync_channel::<Job>(8);
        tx.send(vec![1]).expect("send");
        tx.send(vec![2]).expect("send");
        tx.send(vec![3]).expect("send");
        drop(tx); // loop exits after the backlog drains

        let mut outcomes: VecDeque<Result<(), PostError>> = VecDeque::from([
            Err(PostError::Transient("down".to_string())),
            Err(PostError::Transient("still down".to_string())),
            Ok(()),
            Ok(()),
            Ok(()),
        ]);
        let mut posted: Vec<Job> = Vec::new();
        let mut post = |body: &[u8]| {
            posted.push(body.to_vec());
            outcomes.pop_front().unwrap_or(Ok(()))
        };
        let counters = Counters::new();
        run_sender(&rx, &mut post, Duration::ZERO, &StopSignal::default(), &counters);

        assert_eq!(posted, vec![vec![1], vec![1], vec![1], vec![2], vec![3]]);
        let m = counters.snapshot();
        assert_eq!(m.sent, 3);
        assert_eq!(m.retried, 2);
        assert_eq!(m.dropped_queue_full, 0);
        assert_eq!(m.dropped_disconnected, 0);
        assert_eq!(m.dropped_rejected, 0);
    }

    /// A 4xx rejection must drop the body (no endless retry of a request the
    /// sidecar will always refuse) and continue with later jobs in order.
    #[test]
    fn rejected_body_is_dropped_not_retried() {
        let (tx, rx) = sync_channel::<Job>(8);
        tx.send(vec![1]).expect("send");
        tx.send(vec![2]).expect("send");
        drop(tx);

        let mut outcomes: VecDeque<Result<(), PostError>> = VecDeque::from([Err(PostError::Rejected(401)), Ok(())]);
        let mut posted: Vec<Job> = Vec::new();
        let mut post = |body: &[u8]| {
            posted.push(body.to_vec());
            outcomes.pop_front().unwrap_or(Ok(()))
        };
        let counters = Counters::new();
        run_sender(&rx, &mut post, Duration::ZERO, &StopSignal::default(), &counters);

        // The rejected body was attempted exactly once, then abandoned.
        assert_eq!(posted, vec![vec![1], vec![2]]);
        let m = counters.snapshot();
        assert_eq!(m.sent, 1);
        assert_eq!(m.retried, 0);
        assert_eq!(m.dropped_rejected, 1);
    }

    /// A full queue drops the new job (never blocks) and counts it correctly;
    /// buffered jobs are untouched.
    #[test]
    fn enqueue_on_full_queue_drops_and_counts() {
        let (tx, rx) = sync_channel::<Job>(1);
        let counters = Counters::new();
        try_enqueue(&tx, vec![1], &counters); // fills the queue
        try_enqueue(&tx, vec![2], &counters); // full: must return immediately, dropped
        let m = counters.snapshot();
        assert_eq!(m.dropped_queue_full, 1);
        assert_eq!(m.dropped_disconnected, 0);
        assert_eq!(rx.try_recv().expect("buffered job"), vec![1]);
        assert!(rx.try_recv().is_err(), "dropped job must not be queued");
    }

    /// A disconnected channel (receiver gone) counts as a disconnected drop.
    #[test]
    fn enqueue_after_disconnect_counts_disconnected() {
        let (tx, rx) = sync_channel::<Job>(1);
        drop(rx);
        let counters = Counters::new();
        try_enqueue(&tx, vec![1], &counters);
        let m = counters.snapshot();
        assert_eq!(m.dropped_disconnected, 1);
        assert_eq!(m.dropped_queue_full, 0);
    }

    /// Shutdown must interrupt an active retry backoff and abandon the backlog
    /// within a fixed sub-second deadline — never wait out the window or retry
    /// forever.
    #[test]
    fn shutdown_interrupts_backoff_within_deadline() {
        let (tx, rx) = sync_channel::<Job>(8);
        tx.send(vec![1]).expect("send");
        tx.send(vec![2]).expect("send"); // backlog that must be abandoned

        let stop = Arc::new(StopSignal::default());
        let stop_for_thread = Arc::clone(&stop);
        let (attempted_tx, attempted_rx) = std::sync::mpsc::channel::<Job>();
        let handle = std::thread::spawn(move || {
            let counters = Counters::new();
            let mut post = |body: &[u8]| {
                attempted_tx.send(body.to_vec()).expect("signal attempt");
                Err(PostError::Transient("backend down".to_string()))
            };
            // A backoff far beyond the deadline: only the stop signal can end it.
            run_sender(&rx, &mut post, Duration::from_secs(60), &stop_for_thread, &counters);
        });

        // Wait until the first attempt failed and the sender entered backoff.
        assert_eq!(attempted_rx.recv().expect("first attempt"), vec![1]);

        let started = std::time::Instant::now();
        stop.stop();
        drop(tx);
        handle.join().expect("sender thread exits");
        assert!(
            started.elapsed() < Duration::from_millis(900),
            "shutdown took {:?}, expected sub-second",
            started.elapsed()
        );
        // The abandoned backlog job was never attempted.
        assert!(attempted_rx.try_recv().is_err(), "no attempt after stop");
    }

    #[test]
    fn enqueue_without_start_is_noop() {
        // No publisher started: should not panic, just drop silently (counted
        // in the global disconnected counter).
        stop();
        enqueue(vec![1, 2, 3]);
    }
}
