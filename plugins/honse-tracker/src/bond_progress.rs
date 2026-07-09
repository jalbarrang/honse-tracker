//! Per-career support-card event-chain progress.
//!
//! The game keeps no persistent "events seen" tally we can read at an arbitrary
//! moment (`StoryInfoListDic` is transient), so we accumulate it ourselves: each
//! refresh we observe the fired events currently in that dict and bump the
//! matching deck card's counter once. Users can also adjust a card's count with
//! the +/- buttons (e.g. to seed a career already in progress). Career-scoped;
//! cleared when the career ends.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::gametora_data;
use crate::memory_reader::FiredEvent;

#[derive(Default)]
struct Progress {
    /// Fired `(event_id, story_id)` already counted this career (dedupe).
    seen: HashSet<(i32, i32)>,
    /// support_id -> events seen (auto + manual), clamped to the card's max.
    count: HashMap<i64, u32>,
}

static STATE: Mutex<Option<Progress>> = Mutex::new(None);

/// Reset on career end.
pub fn clear() {
    if let Ok(mut g) = STATE.lock() {
        *g = None;
    }
}

/// Observe the currently-playing fired events and bump the matching deck card's
/// counter once per newly-seen event. `deck` is `(slot, support_id)` pairs.
pub fn observe(deck: &[(i32, i32)], fired: &[FiredEvent]) {
    if fired.is_empty() {
        return;
    }
    let Ok(mut guard) = STATE.lock() else {
        return;
    };
    let p = guard.get_or_insert_with(Progress::default);
    for e in fired {
        if !p.seen.insert((e.event_id, e.story_id)) {
            continue; // already counted this career
        }
        for (_slot, support_id) in deck {
            let sid = *support_id as i64;
            if sid <= 0 {
                continue;
            }
            let matches = gametora_data::chain_event_keys(sid).iter().any(|(eid, stid)| {
                (*eid != 0 && *eid as i32 == e.event_id) || (*stid != 0 && *stid as i32 == e.story_id)
            });
            if matches {
                let max = gametora_data::max_chain_steps(sid).unwrap_or(0);
                let c = p.count.entry(sid).or_insert(0);
                *c = (*c + 1).min(max);
                break; // an event belongs to at most one card
            }
        }
    }
}

/// Events seen for a card (auto + manual).
pub fn count(support_id: i64) -> u32 {
    STATE
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|p| p.count.get(&support_id).copied()))
        .unwrap_or(0)
}

/// Manually nudge a card's counter by `delta`, clamped to `0..=max`.
pub fn adjust(support_id: i64, delta: i32, max: u32) {
    let Ok(mut guard) = STATE.lock() else {
        return;
    };
    let p = guard.get_or_insert_with(Progress::default);
    let c = p.count.entry(support_id).or_insert(0);
    *c = (*c as i32 + delta).clamp(0, max as i32) as u32;
}
