//! Persisted overlay presentation prefs (currently just the content zoom).
//!
//! The zoom is a uniform multiplier applied to the overlay's font + spacing so
//! the whole panel scales. It is an explicit user setting (slider) rather than
//! being derived from the window size, which would feed back into the panel's
//! auto-sizing and grow unbounded.

use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) const MIN_ZOOM: f32 = 0.4;
pub(crate) const MAX_ZOOM: f32 = 2.5;
const DEFAULT_ZOOM: f32 = 1.0;

/// Committed zoom: what the overlay actually renders at.
static ZOOM: AtomicU32 = AtomicU32::new(DEFAULT_ZOOM.to_bits());
/// In-flight slider value while the user is dragging. Kept separate so the panel
/// (and the slider widget itself) only rescale once, on release — dragging never
/// resizes the slider under the cursor.
static PENDING_ZOOM: AtomicU32 = AtomicU32::new(DEFAULT_ZOOM.to_bits());

/// Current (committed) overlay content zoom — used for rendering.
pub(crate) fn zoom() -> f32 {
    f32::from_bits(ZOOM.load(Ordering::Relaxed))
}

/// Set the committed zoom *and* the pending value (clamped). Used on config load.
pub(crate) fn set_zoom(value: f32) {
    let bits = value.clamp(MIN_ZOOM, MAX_ZOOM).to_bits();
    ZOOM.store(bits, Ordering::Relaxed);
    PENDING_ZOOM.store(bits, Ordering::Relaxed);
}

/// The slider's in-flight value.
pub(crate) fn pending_zoom() -> f32 {
    f32::from_bits(PENDING_ZOOM.load(Ordering::Relaxed))
}

/// Update the in-flight value while dragging (does not affect rendering).
pub(crate) fn set_pending_zoom(value: f32) {
    PENDING_ZOOM.store(value.clamp(MIN_ZOOM, MAX_ZOOM).to_bits(), Ordering::Relaxed);
}

/// Commit the pending value to the live zoom (call on slider release).
pub(crate) fn commit_zoom() {
    ZOOM.store(PENDING_ZOOM.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// Default zoom for fresh configs.
pub(crate) fn default_zoom() -> f32 {
    DEFAULT_ZOOM
}
