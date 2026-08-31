//! Mouse state, from the window procedure to the overlay frame.
//!
//! [`crate::input_block`] sees the mouse messages; [`crate::overlay`] needs
//! them a frame later, on the render thread. This is the queue between them,
//! and the one flag going back the other way — whether the overlay is
//! currently claiming the pointer.
//!
//! # Positions are client pixels
//!
//! Nothing here scales. The window's client area and the swapchain's backbuffer
//! are not always the same size (resolution scaling), and only the frame knows
//! both, so conversion happens there.
//!
//! # Recording is not consuming
//!
//! Every mouse message is recorded. Only the ones arriving while
//! [`is_capturing`] is set are dropped before the game sees them, and that flag
//! is only ever set by a frame that found the pointer over an interactive
//! panel. An overlay with nothing to click never takes a click.

use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// One mouse event as the window procedure saw it, in client pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Moved(egui::Pos2),
    Button {
        pos: egui::Pos2,
        button: egui::PointerButton,
        pressed: bool,
    },
    /// Wheel notches, positive away from the user.
    Wheel(f32),
    /// The pointer left the window — egui must stop hovering.
    Gone,
}

/// Events held before the next frame drains them.
///
/// Bounded, because nothing drains this while the overlay is not painting: a
/// minimised game would otherwise queue mouse moves until it ran out of memory.
const MAX_QUEUED: usize = 256;

static QUEUE: Lazy<Mutex<Vec<PointerEvent>>> = Lazy::new(|| Mutex::new(Vec::new()));
static CAPTURING: AtomicBool = AtomicBool::new(false);

/// Record an event for the next frame.
///
/// Consecutive moves collapse into the latest one: egui only needs where the
/// pointer *is*, and a fast mouse can outpace the frame rate several times
/// over.
pub fn push(event: PointerEvent) {
    let mut queue = QUEUE.lock();
    if let (PointerEvent::Moved(_), Some(PointerEvent::Moved(_))) = (&event, queue.last()) {
        queue.pop();
    }
    if queue.len() >= MAX_QUEUED {
        queue.remove(0);
    }
    queue.push(event);
}

/// Take everything recorded since the last frame.
#[must_use]
pub fn take() -> Vec<PointerEvent> {
    std::mem::take(&mut *QUEUE.lock())
}

/// Whether the overlay is claiming the pointer, so clicks belong to it rather
/// than to the game.
#[must_use]
pub fn is_capturing() -> bool {
    CAPTURING.load(Ordering::Acquire)
}

/// Set by the frame, once it knows where the pointer landed.
pub fn set_capturing(capturing: bool) {
    CAPTURING.store(capturing, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32) -> PointerEvent {
        PointerEvent::Moved(egui::pos2(x, 0.0))
    }

    #[test]
    fn consecutive_moves_collapse_to_the_latest() {
        let _ = take();
        push(at(1.0));
        push(at(2.0));
        push(at(3.0));
        assert_eq!(take(), vec![at(3.0)], "only the final position matters");
    }

    #[test]
    fn a_click_between_moves_keeps_both_moves() {
        let _ = take();
        let click = PointerEvent::Button {
            pos: egui::pos2(1.0, 1.0),
            button: egui::PointerButton::Primary,
            pressed: true,
        };
        push(at(1.0));
        push(click);
        push(at(2.0));
        assert_eq!(take(), vec![at(1.0), click, at(2.0)], "a click is not a move");
    }

    #[test]
    fn the_queue_is_bounded() {
        let _ = take();
        for i in 0..(MAX_QUEUED + 50) {
            push(PointerEvent::Wheel(i as f32));
        }
        assert_eq!(take().len(), MAX_QUEUED, "oldest events are dropped, not memory");
    }
}
