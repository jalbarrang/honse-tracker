//! Background publisher: a bounded queue drained by one sender thread.
//!
//! `publish()` is called from the Unity main thread and from race hooks, so it
//! must never block. It encodes + enqueues onto a `SyncSender` (drop-on-full) and
//! returns immediately. The sender thread performs the blocking HTTP POSTs and
//! applies a backoff window after a failure so a dead backend isn't hammered.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::Mutex;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::{Config, Endpoint};
use crate::transport;

/// Queue depth. Career snapshots are ~2s apart; race-live is 10Hz. 64 absorbs
/// bursts without unbounded memory if the backend stalls.
const QUEUE_CAP: usize = 64;
/// After a failed POST, drop everything for this long before retrying.
const BACKOFF: Duration = Duration::from_secs(2);
const CONTENT_TYPE: &str = "application/x-protobuf";

/// Counter of envelopes dropped because the queue was full (diagnostics).
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// A ready-to-send, pre-encoded protobuf body.
type Job = Vec<u8>;

struct Inner {
    tx: SyncSender<Job>,
    handle: Option<JoinHandle<()>>,
}

static PUBLISHER: Mutex<Option<Inner>> = Mutex::new(None);

/// Whether a backoff window started at `since` is still active at `now`.
#[must_use]
pub fn in_backoff(since: Option<Instant>, now: Instant, window: Duration) -> bool {
    matches!(since, Some(t) if now.duration_since(t) < window)
}

/// Start the sender thread for `endpoint`. Idempotent-ish: replaces any existing
/// publisher (callers should `stop()` first in practice).
pub fn start(endpoint: Endpoint) {
    let (tx, rx) = sync_channel::<Job>(QUEUE_CAP);
    let handle = std::thread::Builder::new()
        .name("hachimi-telemetry".to_string())
        .spawn(move || sender_loop(&endpoint, &rx))
        .ok();
    let mut guard = PUBLISHER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    *guard = Some(Inner { tx, handle });
}

/// Enqueue an already-encoded body. Never blocks; drops on a full queue.
pub fn enqueue(body: Job) {
    let guard = PUBLISHER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(inner) = guard.as_ref() {
        match inner.tx.try_send(body) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                DROPPED.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

/// Stop the sender thread and join it (closes the channel first).
pub fn stop() {
    let inner = {
        let mut guard = PUBLISHER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        guard.take()
    };
    if let Some(mut inner) = inner {
        // Dropping the only sender disconnects the channel; the loop then exits.
        drop(std::mem::replace(&mut inner.tx, sync_channel::<Job>(1).0));
        if let Some(h) = inner.handle.take() {
            let _ = h.join();
        }
    }
}

/// Number of envelopes dropped so far (full queue).
#[must_use]
pub fn dropped_count() -> u64 {
    DROPPED.load(Ordering::Relaxed)
}

fn sender_loop(endpoint: &Endpoint, rx: &Receiver<Job>) {
    let mut backoff_since: Option<Instant> = None;
    while let Ok(body) = rx.recv() {
        if in_backoff(backoff_since, Instant::now(), BACKOFF) {
            continue; // drop while backing off
        }
        match transport::post(endpoint, CONTENT_TYPE, &body) {
            Ok(()) => backoff_since = None,
            Err(_) => backoff_since = Some(Instant::now()),
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
    use super::*;

    #[test]
    fn backoff_active_within_window() {
        let now = Instant::now();
        assert!(in_backoff(Some(now), now, BACKOFF));
        assert!(!in_backoff(None, now, BACKOFF));
    }

    #[test]
    fn backoff_expires_after_window() {
        let start = Instant::now();
        let later = start + Duration::from_secs(3);
        assert!(!in_backoff(Some(start), later, BACKOFF));
    }

    #[test]
    fn enqueue_without_start_is_noop() {
        // No publisher started: should not panic, just drop silently.
        stop();
        enqueue(vec![1, 2, 3]);
    }
}
