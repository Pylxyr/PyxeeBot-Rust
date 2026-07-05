use std::collections::VecDeque;

use rand::seq::SliceRandom;

use super::snapshot::PlayerSnapshot;
use crate::models::{LoopMode, Track};

const MAX_HISTORY: usize = 50;

pub struct PlayerState {
    pub queue: VecDeque<Track>,
    pub history: Vec<Track>,
    pub current: Option<Track>,
    pub loop_mode: LoopMode,
    pub stay_connected: bool,
    pub total_duration: i64,
    pub max_queue_size: usize,
}

impl PlayerState {
    pub fn new(max_queue_size: usize) -> Self {
        Self {
            queue: VecDeque::new(),
            history: Vec::new(),
            current: None,
            loop_mode: LoopMode::Off,
            stay_connected: false,
            total_duration: 0,
            max_queue_size,
        }
    }

    pub fn is_full(&self) -> bool {
        self.queue.len() >= self.max_queue_size
    }

    pub fn user_queue_count(&self, requester_id: u64) -> usize {
        self.queue
            .iter()
            .filter(|t| t.requester_id == requester_id)
            .count()
    }

    /// Appends to the back, evicting the front if already at capacity —
    /// mirrors Python's `collections.deque(maxlen=N)` semantics for `append`.
    pub fn push_back(&mut self, track: Track) {
        if self.queue.len() >= self.max_queue_size {
            if let Some(evicted) = self.queue.pop_front() {
                self.total_duration -= evicted.duration;
            }
        }
        self.total_duration += track.duration;
        self.queue.push_back(track);
    }

    /// Prepends to the front, evicting the back if already at capacity —
    /// mirrors `deque.appendleft` semantics. This eviction path is exactly
    /// what the Python version originally missed for `!replay` and loop
    /// mode, silently drifting `total_duration` out of sync with the queue.
    pub fn push_front(&mut self, track: Track) {
        if self.queue.len() >= self.max_queue_size {
            if let Some(evicted) = self.queue.pop_back() {
                self.total_duration -= evicted.duration;
            }
        }
        self.total_duration += track.duration;
        self.queue.push_front(track);
    }

    pub fn pop_front(&mut self) -> Option<Track> {
        let track = self.queue.pop_front();
        if let Some(t) = &track {
            self.total_duration -= t.duration;
        }
        track
    }

    /// Moves `current` into (bounded) history and pulls the next track off
    /// the queue as the new `current`.
    pub fn advance(&mut self) -> Option<Track> {
        if let Some(prev) = self.current.take() {
            self.history.push(prev);
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }
        self.current = self.pop_front();
        self.current.clone()
    }

    /// Requeues a just-finished track per loop mode. Both paths go through
    /// `push_front`/`push_back`, so eviction accounting is always correct.
    pub fn requeue_finished(&mut self, track: Track) {
        match self.loop_mode {
            LoopMode::One => self.push_front(track),
            LoopMode::All => self.push_back(track),
            LoopMode::Off => {}
        }
    }

    /// Rewinds playback: the last history entry becomes `current`, and the
    /// previous `current` (if any) goes back on the front of the queue.
    /// Returns false if there is no history to rewind to.
    pub fn play_previous(&mut self) -> bool {
        let Some(previous) = self.history.pop() else {
            return false;
        };
        if let Some(current) = self.current.take() {
            self.push_front(current);
        }
        self.current = Some(previous);
        true
    }

    pub fn clear(&mut self) -> usize {
        let n = self.queue.len();
        self.queue.clear();
        self.total_duration = 0;
        n
    }

    pub fn shuffle(&mut self) {
        self.queue
            .make_contiguous()
            .shuffle(&mut rand::thread_rng());
    }

    pub fn remove(&mut self, position: usize) -> Option<Track> {
        let track = self.queue.remove(position);
        if let Some(t) = &track {
            self.total_duration -= t.duration;
        }
        track
    }

    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        if from >= self.queue.len() || to >= self.queue.len() {
            return false;
        }
        if let Some(track) = self.queue.remove(from) {
            self.queue.insert(to, track);
            true
        } else {
            false
        }
    }

    /// Whether an empty-channel disconnect should proceed. This is the exact
    /// check the Python version was missing — `_disconnect_when_empty` had
    /// no `stay_connected` guard, so `!stay` was ignored once every human
    /// left the channel.
    pub fn should_disconnect_when_empty(&self, has_human_listeners: bool) -> bool {
        !self.stay_connected && !has_human_listeners
    }

    /// Whether an idle disconnect (nothing playing, nothing queued) should
    /// proceed.
    pub fn should_disconnect_when_idle(&self) -> bool {
        !self.stay_connected && self.current.is_none() && self.queue.is_empty()
    }

    pub fn to_snapshot(&self, is_connected: bool, is_paused: bool) -> PlayerSnapshot {
        PlayerSnapshot {
            current: self.current.clone(),
            queue: self.queue.iter().cloned().collect(),
            history: self.history.clone(),
            loop_mode: self.loop_mode,
            stay_connected: self.stay_connected,
            is_paused,
            is_connected,
            total_duration_secs: self.total_duration,
        }
    }
}
