use std::collections::VecDeque;
use std::sync::Arc;

use poise::serenity_prelude::ChannelId;
use rand::seq::SliceRandom;

use super::snapshot::PlayerSnapshot;
use crate::models::{LoopMode, Track};

const MAX_HISTORY: usize = 50;

#[derive(Debug)]
pub enum RemoveOutcome {
    Removed(Arc<Track>),
    NotFound,
    NotAllowed,
}

// Queue/history/current hold `Arc<Track>` rather than `Track`. `to_snapshot` below
// (and the queue-persistence path in the actor) clone the whole queue on nearly
// every command; with owned `Track`s that was a deep clone of every String field
// in every queued track. With `Arc<Track>` it's a pointer-sized refcount bump per
// track, regardless of queue length.
pub struct PlayerState {
    pub queue: VecDeque<Arc<Track>>,
    pub history: VecDeque<Arc<Track>>,
    pub current: Option<Arc<Track>>,
    pub loop_mode: LoopMode,
    pub stay_connected: bool,
    pub autoplay: bool,
    pub total_duration: i64,
    pub max_queue_size: usize,
    dirty: bool,
}

impl PlayerState {
    pub fn new(max_queue_size: usize, stay_connected: bool, autoplay: bool) -> Self {
        Self {
            queue: VecDeque::new(),
            history: VecDeque::new(),
            current: None,
            loop_mode: LoopMode::Off,
            stay_connected,
            autoplay,
            total_duration: 0,
            max_queue_size,
            dirty: false,
        }
    }

    pub fn take_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.dirty, false)
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

    pub fn push_back(&mut self, track: Arc<Track>) {
        self.dirty = true;
        if self.queue.len() >= self.max_queue_size {
            if let Some(evicted) = self.queue.pop_front() {
                self.total_duration -= evicted.duration;
            }
        }
        self.total_duration += track.duration;
        self.queue.push_back(track);
    }

    pub fn push_front(&mut self, track: Arc<Track>) {
        self.dirty = true;
        if self.queue.len() >= self.max_queue_size {
            if let Some(evicted) = self.queue.pop_back() {
                self.total_duration -= evicted.duration;
            }
        }
        self.total_duration += track.duration;
        self.queue.push_front(track);
    }

    pub fn pop_front(&mut self) -> Option<Arc<Track>> {
        let track = self.queue.pop_front();
        if let Some(t) = &track {
            self.dirty = true;
            self.total_duration -= t.duration;
        }
        track
    }

    pub fn advance(&mut self) -> Option<Arc<Track>> {
        self.dirty = true;
        if let Some(prev) = self.current.take() {
            self.history.push_back(prev);
            if self.history.len() > MAX_HISTORY {
                self.history.pop_front();
            }
        }
        self.current = self.pop_front();
        self.current.clone()
    }

    pub fn discard_current(&mut self) -> Option<Arc<Track>> {
        self.dirty = true;
        self.current = self.pop_front();
        self.current.clone()
    }

    pub fn requeue_finished(&mut self, track: Arc<Track>) {
        match self.loop_mode {
            LoopMode::One => self.push_front(track),
            LoopMode::All => self.push_back(track),
            LoopMode::Off => {}
        }
    }

    pub fn play_previous(&mut self) -> bool {
        let Some(previous) = self.history.pop_back() else {
            return false;
        };
        self.dirty = true;
        if let Some(current) = self.current.take() {
            self.push_front(current);
        }
        self.current = Some(previous);
        true
    }

    pub fn clear(&mut self) -> usize {
        let n = self.queue.len();
        self.dirty = true;
        self.queue.clear();
        self.total_duration = 0;
        n
    }

    pub fn shuffle(&mut self) {
        self.dirty = true;
        self.queue.make_contiguous().shuffle(&mut rand::rng());
    }

    pub fn remove(&mut self, position: usize) -> Option<Arc<Track>> {
        let track = self.queue.remove(position);
        if let Some(t) = &track {
            self.dirty = true;
            self.total_duration -= t.duration;
        }
        track
    }

    pub fn remove_if_allowed(
        &mut self,
        position: usize,
        requester_id: u64,
        is_dj: bool,
    ) -> RemoveOutcome {
        let Some(track) = self.queue.get(position) else {
            return RemoveOutcome::NotFound;
        };
        if !is_dj && track.requester_id != requester_id {
            return RemoveOutcome::NotAllowed;
        }
        match self.remove(position) {
            Some(track) => RemoveOutcome::Removed(track),
            None => RemoveOutcome::NotFound,
        }
    }

    pub fn move_track(&mut self, from: usize, to: usize) -> bool {
        if from >= self.queue.len() || to >= self.queue.len() {
            return false;
        }
        if let Some(track) = self.queue.remove(from) {
            self.dirty = true;
            self.queue.insert(to, track);
            true
        } else {
            false
        }
    }

    pub fn should_disconnect_when_empty(&self) -> bool {
        !self.stay_connected
    }

    pub fn should_disconnect_when_idle(&self) -> bool {
        !self.stay_connected && self.current.is_none() && self.queue.is_empty()
    }

    pub fn to_snapshot(
        &self,
        is_connected: bool,
        is_paused: bool,
        elapsed_secs: i64,
        channel_id: Option<ChannelId>,
        volume: u8,
    ) -> PlayerSnapshot {
        PlayerSnapshot {
            current: self.current.clone(),
            queue: self.queue.iter().cloned().collect(),
            history: self.history.iter().cloned().collect(),
            loop_mode: self.loop_mode,
            stay_connected: self.stay_connected,
            is_paused,
            is_connected,
            channel_id,
            total_duration_secs: self.total_duration,
            elapsed_secs,
            volume,
        }
    }
}
