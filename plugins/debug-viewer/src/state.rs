use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

const HISTORY_LIMIT: usize = 16;

static STATE: OnceLock<Mutex<State>> = OnceLock::new();

#[derive(Debug)]
struct State {
    started_at: Instant,
    sequence: u64,
    current_view_id: Option<i32>,
    previous_view_id: Option<i32>,
    history: VecDeque<HistoryEntry>,
}

#[derive(Clone, Copy, Debug)]
pub struct HistoryEntry {
    pub sequence: u64,
    pub view_id: i32,
    pub seconds_since_start: f32,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub sequence: u64,
    pub current_view_id: Option<i32>,
    pub previous_view_id: Option<i32>,
    pub history: Vec<HistoryEntry>,
}

#[derive(Clone, Copy, Debug)]
pub struct ViewUpdate {
    pub sequence: u64,
    pub previous_view_id: Option<i32>,
    pub current_view_id: i32,
}

impl State {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            sequence: 0,
            current_view_id: None,
            previous_view_id: None,
            history: VecDeque::with_capacity(HISTORY_LIMIT),
        }
    }

    fn record_view_change(&mut self, view_id: i32) -> ViewUpdate {
        let previous_view_id = self.current_view_id;

        self.sequence += 1;
        self.previous_view_id = previous_view_id;
        self.current_view_id = Some(view_id);

        if self.history.len() == HISTORY_LIMIT {
            self.history.pop_front();
        }
        self.history.push_back(HistoryEntry {
            sequence: self.sequence,
            view_id,
            seconds_since_start: self.started_at.elapsed().as_secs_f32(),
        });

        ViewUpdate {
            sequence: self.sequence,
            previous_view_id,
            current_view_id: view_id,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            sequence: self.sequence,
            current_view_id: self.current_view_id,
            previous_view_id: self.previous_view_id,
            history: self.history.iter().copied().collect(),
        }
    }

    fn reset(&mut self) {
        self.sequence = 0;
        self.current_view_id = None;
        self.previous_view_id = None;
        self.history.clear();
        self.started_at = Instant::now();
    }
}

pub fn init() {
    let _ = STATE.get_or_init(|| Mutex::new(State::new()));
}

pub fn record_view_change(view_id: i32) -> ViewUpdate {
    let mut state = STATE
        .get_or_init(|| Mutex::new(State::new()))
        .lock()
        .expect("debug-viewer state lock poisoned");
    state.record_view_change(view_id)
}

pub fn snapshot() -> Snapshot {
    let state = STATE
        .get_or_init(|| Mutex::new(State::new()))
        .lock()
        .expect("debug-viewer state lock poisoned");
    state.snapshot()
}

pub fn reset() {
    let mut state = STATE
        .get_or_init(|| Mutex::new(State::new()))
        .lock()
        .expect("debug-viewer state lock poisoned");
    state.reset();
}

#[cfg(test)]
mod tests {
    use super::{record_view_change, reset, snapshot};

    #[test]
    fn records_transitions_with_previous() {
        reset();
        let first = record_view_change(2);
        assert_eq!(first.previous_view_id, None);
        assert_eq!(first.current_view_id, 2);

        let second = record_view_change(400);
        assert_eq!(second.previous_view_id, Some(2));
        assert_eq!(second.current_view_id, 400);

        let snap = snapshot();
        assert_eq!(snap.current_view_id, Some(400));
        assert_eq!(snap.previous_view_id, Some(2));
        assert_eq!(snap.sequence, 2);
        assert_eq!(snap.history.len(), 2);
        reset();
    }
}
