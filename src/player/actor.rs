use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{ChannelId, GuildId};
use songbird::events::{Event, EventContext, EventHandler as SongbirdEventHandler, TrackEvent};
use songbird::input::{HttpRequest, Input};
use songbird::tracks::TrackHandle;
use songbird::{Call, Songbird};
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio::task::{AbortHandle, JoinHandle};

use crate::config::Config;
use crate::db::Database;
use crate::errors::{BotError, Result};
use crate::extraction::Extractor;
use crate::lastfm::LastFmClient;
use crate::models::{LoopMode, Track};

use super::lifecycle;
use super::queue::PlayerState;
use super::snapshot::PlayerSnapshot;

#[derive(Debug)]
pub struct PlayOutcome {
    pub position: usize,
    pub now_playing: bool,
    /// True if the whole queue (including what was just added) failed to resolve.
    pub failed: bool,
}

pub enum PlayerCommand {
    Play {
        track: Track,
        front: bool,
        channel_id: ChannelId,
        reply: oneshot::Sender<Result<PlayOutcome>>,
    },
    Skip,
    Stop,
    Pause {
        reply: oneshot::Sender<()>,
    },
    Resume {
        reply: oneshot::Sender<()>,
    },
    SetVolume {
        volume: u8,
        reply: oneshot::Sender<()>,
    },
    Leave {
        reply: oneshot::Sender<Result<bool>>,
    },
    Connect {
        channel_id: ChannelId,
        reply: oneshot::Sender<Result<()>>,
    },
    SetStay(bool),
    SetAutoplay(bool),
    CycleLoop {
        reply: oneshot::Sender<LoopMode>,
    },
    Previous {
        reply: oneshot::Sender<bool>,
    },
    ClearQueue {
        reply: oneshot::Sender<usize>,
    },
    Shuffle,
    RemoveTrack {
        position: usize,
        requester_id: u64,
        is_dj: bool,
        reply: oneshot::Sender<super::queue::RemoveOutcome>,
    },
    MoveTrack {
        from: usize,
        to: usize,
        reply: oneshot::Sender<bool>,
    },
    /// Reconnects without touching queue/playback state — used after a force-kick with stay_connected on.
    Rejoin {
        channel_id: ChannelId,
    },
    /// Moved channels without disconnecting (e.g. dragged) — songbird already followed, this just syncs.
    SyncChannel(ChannelId),
    /// Normal track end; ignored if the generation no longer matches (already superseded, e.g. a skip).
    TrackEnded(u64),
    /// Same generation check as TrackEnded, but for a playback failure — logged distinctly, not requeued.
    TrackErrored(u64, Option<String>),
    ScheduleEmptyDisconnect,
    CancelEmptyDisconnect,
    IdleTimeout,
    EmptyTimeout,
    Shutdown,
    ResolveDone {
        generation: u64,
        result: Result<crate::extraction::ResolvedInfo>,
    },
    /// Background autoplay search result; `None` means nothing playable turned up.
    AutoplayReady(Option<Track>),
}

struct TrackEndNotifier {
    tx: mpsc::UnboundedSender<PlayerCommand>,
    generation: u64,
    errored: bool,
}

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackEndNotifier {
    async fn act(&self, ctx: &EventContext<'_>) -> Option<Event> {
        let cmd = if self.errored {
            // Debug-formatted, not matched: songbird's error enums are non-exhaustive and version-specific.
            let reason = match ctx {
                EventContext::Track(states) => {
                    states.first().map(|(state, _)| format!("{:?}", state.playing))
                }
                _ => None,
            };
            PlayerCommand::TrackErrored(self.generation, reason)
        } else {
            PlayerCommand::TrackEnded(self.generation)
        };
        let _ = self.tx.send(cmd);
        None
    }
}

pub struct PlayerActor {
    guild_id: GuildId,
    songbird: Arc<Songbird>,
    extractor: Arc<Extractor>,
    http_client: reqwest::Client,
    lastfm: Option<LastFmClient>,
    config: Arc<Config>,
    db: Arc<Database>,
    state: PlayerState,
    call: Option<Arc<AsyncMutex<Call>>>,
    channel_id: Option<ChannelId>,
    current_handle: Option<TrackHandle>,
    current_generation: u64,
    rx: mpsc::UnboundedReceiver<PlayerCommand>,
    self_tx: mpsc::UnboundedSender<PlayerCommand>,
    snapshot_tx: watch::Sender<PlayerSnapshot>,
    is_paused: bool,
    idle_timer: Option<JoinHandle<()>>,
    empty_timer: Option<JoinHandle<()>>,
    track_started_at: Option<std::time::Instant>,
    paused_since: Option<std::time::Instant>,
    paused_total: std::time::Duration,
    last_snapshot_hash: Option<u64>,
    retried_current_track: bool,
    /// Dedups songbird's paired Error+End fire, since Skip clears current_handle before TrackEnded arrives.
    last_finished_generation: Option<u64>,
    /// Reply for a `!play` into an empty queue, waiting on the background resolve (see handle_play).
    pending_play_reply: Option<(oneshot::Sender<Result<PlayOutcome>>, usize)>,
    prefetch_task: Option<AbortHandle>,
    /// 0-200 — songbird's volume lives on the track, so it's reapplied on every track start, not set once.
    volume: u8,
}

impl PlayerActor {
    #[allow(clippy::too_many_arguments)]
    pub fn spawn(
        guild_id: GuildId,
        songbird: Arc<Songbird>,
        extractor: Arc<Extractor>,
        http_client: reqwest::Client,
        lastfm: Option<LastFmClient>,
        config: Arc<Config>,
        db: Arc<Database>,
        stay_connected: bool,
        autoplay: bool,
        volume: u8,
    ) -> (
        mpsc::UnboundedSender<PlayerCommand>,
        watch::Receiver<PlayerSnapshot>,
    ) {
        let (tx, rx) = mpsc::unbounded_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(PlayerSnapshot::default());
        let max_queue_size = config.max_queue_size;
        let actor = PlayerActor {
            guild_id,
            songbird,
            extractor,
            http_client,
            lastfm,
            config,
            db,
            state: PlayerState::new(max_queue_size, stay_connected, autoplay),
            call: None,
            channel_id: None,
            current_handle: None,
            current_generation: 0,
            rx,
            self_tx: tx.clone(),
            snapshot_tx,
            is_paused: false,
            idle_timer: None,
            empty_timer: None,
            track_started_at: None,
            paused_since: None,
            paused_total: std::time::Duration::ZERO,
            last_snapshot_hash: None,
            retried_current_track: false,
            last_finished_generation: None,
            pending_play_reply: None,
            prefetch_task: None,
            volume,
        };
        tokio::spawn(actor.run());
        (tx, snapshot_rx)
    }

    async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            let is_shutdown = matches!(cmd, PlayerCommand::Shutdown);
            self.handle(cmd).await;
            self.publish_snapshot();
            self.persist_queue_snapshot();
            if is_shutdown {
                break;
            }
        }
    }

    /// Fire-and-forget; no-ops when the hash is unchanged.
    fn persist_queue_snapshot(&mut self) {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        for t in self.state.current.iter().chain(self.state.queue.iter()) {
            t.query.hash(&mut hasher);
            t.title.hash(&mut hasher);
            t.webpage_url.hash(&mut hasher);
            t.requester_id.hash(&mut hasher);
        }
        let hash = hasher.finish();
        if self.last_snapshot_hash == Some(hash) {
            return;
        }
        self.last_snapshot_hash = Some(hash);

        let db = self.db.clone();
        let guild_id = self.guild_id.get();
        let mut queued: Vec<Track> = Vec::with_capacity(self.state.queue.len() + 1);
        if let Some(current) = &self.state.current {
            queued.push(current.clone());
        }
        queued.extend(self.state.queue.iter().cloned());
        tokio::spawn(async move {
            let refs: Vec<crate::db::QueueEntryRef> = queued
                .iter()
                .map(|t| crate::db::QueueEntryRef {
                    query: &t.query,
                    title: &t.title,
                    webpage_url: &t.webpage_url,
                    requester_id: t.requester_id,
                })
                .collect();
            if let Err(e) = db.save_queue_snapshot(guild_id, &refs).await {
                tracing::warn!(guild_id = guild_id, error = %e, "persist_queue_snapshot: failed to save");
            }
        });
    }

    /// `None` clears it (e.g. on `!leave`).
    fn persist_last_voice_channel(&self, channel_id: Option<ChannelId>) {
        let db = self.db.clone();
        let guild_id = self.guild_id.get();
        let default_prefix = self.config.default_prefix.clone();
        let channel_id = channel_id.map(|c| c.get());
        tokio::spawn(async move {
            if let Err(e) = db
                .set_last_voice_channel(guild_id, channel_id, &default_prefix)
                .await
            {
                tracing::warn!(guild_id = guild_id, error = %e, "failed to persist last voice channel");
            }
        });
    }

    fn elapsed_secs(&self) -> i64 {
        let Some(started) = self.track_started_at else {
            return 0;
        };
        let now = std::time::Instant::now();
        let paused_extra = self
            .paused_since
            .map(|since| now.duration_since(since))
            .unwrap_or_default();
        now.duration_since(started)
            .saturating_sub(self.paused_total + paused_extra)
            .as_secs() as i64
    }

    fn publish_snapshot(&self) {
        let is_connected = self.call.is_some();
        let _ = self.snapshot_tx.send(self.state.to_snapshot(
            is_connected,
            self.is_paused,
            self.elapsed_secs(),
            self.channel_id,
            self.volume,
        ));
    }

    async fn handle(&mut self, cmd: PlayerCommand) {
        match cmd {
            PlayerCommand::Play {
                track,
                front,
                channel_id,
                reply,
            } => {
                self.handle_play(track, front, channel_id, reply).await;
            }
            PlayerCommand::Skip => {
                self.resolve_pending_play(false, false);
                if let Some(handle) = self.current_handle.take() {
                    let _ = handle.stop();
                } else if self.state.current.is_some() {
                    // Still resolving, no songbird handle yet — invalidate and advance ourselves.
                    self.current_generation += 1;
                    self.discard_current_and_advance();
                }
                self.is_paused = false;
            }
            PlayerCommand::Stop => {
                self.resolve_pending_play(false, false);
                self.state.clear();
                self.state.current = None;
                self.cancel_timers();
                self.current_generation += 1;
                if let Some(handle) = self.prefetch_task.take() {
                    handle.abort();
                }
                if let Some(handle) = self.current_handle.take() {
                    let _ = handle.stop();
                }
                self.is_paused = false;
                self.track_started_at = None;
                self.paused_since = None;
                self.arm_idle_timer();
            }
            PlayerCommand::Pause { reply } => {
                if let Some(handle) = &self.current_handle {
                    let _ = handle.pause();
                    self.is_paused = true;
                    self.paused_since = Some(std::time::Instant::now());
                }
                self.publish_snapshot();
                let _ = reply.send(());
            }
            PlayerCommand::Resume { reply } => {
                if let Some(handle) = &self.current_handle {
                    let _ = handle.play();
                    self.is_paused = false;
                    if let Some(since) = self.paused_since.take() {
                        self.paused_total += since.elapsed();
                    }
                }
                self.publish_snapshot();
                let _ = reply.send(());
            }
            PlayerCommand::SetVolume { volume, reply } => {
                self.volume = volume;
                if let Some(handle) = &self.current_handle {
                    let _ = handle.set_volume(f32::from(volume) / 100.0);
                }
                let db = self.db.clone();
                let guild_id = self.guild_id.get();
                let default_prefix = self.config.default_prefix.clone();
                tokio::spawn(async move {
                    if let Err(e) = db.set_volume(guild_id, volume, &default_prefix).await {
                        tracing::warn!(guild_id, error = %e, "failed to persist volume");
                    }
                });
                self.publish_snapshot();
                let _ = reply.send(());
            }
            PlayerCommand::Leave { reply } => {
                self.resolve_pending_play(false, false);
                self.cancel_timers();
                self.current_generation += 1;
                if let Some(handle) = self.prefetch_task.take() {
                    handle.abort();
                }
                if let Some(handle) = self.current_handle.take() {
                    let _ = handle.stop();
                }
                self.state.clear();
                self.state.current = None;
                self.is_paused = false;
                self.track_started_at = None;
                self.paused_since = None;
                let result = lifecycle::disconnect(&self.songbird, self.guild_id).await;
                self.call = None;
                self.channel_id = None;
                self.persist_last_voice_channel(None);
                let _ = reply.send(result);
            }
            PlayerCommand::Connect { channel_id, reply } => {
                let outcome = lifecycle::connect(&self.songbird, self.guild_id, channel_id).await;
                let result = match outcome {
                    Ok(call) => {
                        self.call = Some(call);
                        self.channel_id = Some(channel_id);
                        self.persist_last_voice_channel(Some(channel_id));
                        Ok(())
                    }
                    Err(e) => Err(e),
                };
                let _ = reply.send(result);
            }
            PlayerCommand::SetStay(enabled) => {
                self.state.stay_connected = enabled;
                if !enabled && self.state.should_disconnect_when_idle() {
                    self.arm_idle_timer();
                }
            }
            PlayerCommand::SetAutoplay(enabled) => {
                self.state.autoplay = enabled;
            }
            PlayerCommand::CycleLoop { reply } => {
                self.state.loop_mode = self.state.loop_mode.cycle();
                let _ = reply.send(self.state.loop_mode);
            }
            PlayerCommand::Previous { reply } => {
                let ok = self.state.play_previous();
                if ok {
                    if let Some(handle) = self.current_handle.take() {
                        let _ = handle.stop();
                    }
                    // Background resolve, like every other path — avoids blocking this guild's loop.
                    self.retried_current_track = false;
                    self.spawn_resolve_for_current();
                }
                let _ = reply.send(ok);
            }
            PlayerCommand::ClearQueue { reply } => {
                let n = self.state.clear();
                let _ = reply.send(n);
            }
            PlayerCommand::Shuffle => {
                self.state.shuffle();
                self.spawn_prefetch();
            }
            PlayerCommand::RemoveTrack {
                position,
                requester_id,
                is_dj,
                reply,
            } => {
                let outcome = self.state.remove_if_allowed(position, requester_id, is_dj);
                if matches!(outcome, super::queue::RemoveOutcome::Removed(_)) {
                    self.spawn_prefetch();
                }
                let _ = reply.send(outcome);
            }
            PlayerCommand::MoveTrack { from, to, reply } => {
                let ok = self.state.move_track(from, to);
                if ok {
                    self.spawn_prefetch();
                }
                let _ = reply.send(ok);
            }
            PlayerCommand::Rejoin { channel_id } => {
                if !self.state.stay_connected {
                    return;
                }
                match lifecycle::connect(&self.songbird, self.guild_id, channel_id).await {
                    Ok(call) => {
                        self.call = Some(call);
                        self.channel_id = Some(channel_id);
                    }
                    Err(e) => {
                        tracing::warn!(guild_id = %self.guild_id, error = %e, "rejoin failed");
                    }
                }
            }
            PlayerCommand::SyncChannel(channel_id) => {
                if self.channel_id != Some(channel_id) {
                    self.channel_id = Some(channel_id);
                    self.persist_last_voice_channel(Some(channel_id));
                }
            }
            PlayerCommand::TrackEnded(generation) => {
                self.handle_track_finished(generation, None).await;
            }
            PlayerCommand::TrackErrored(generation, reason) => {
                self.handle_track_finished(generation, Some(reason.unwrap_or_else(|| "unknown error".to_owned()))).await;
            }
            PlayerCommand::ScheduleEmptyDisconnect => {
                if self.empty_timer.is_none() {
                    let tx = self.self_tx.clone();
                    let timeout = Duration::from_secs(self.config.empty_channel_timeout_secs);
                    self.empty_timer = Some(tokio::spawn(async move {
                        tokio::time::sleep(timeout).await;
                        let _ = tx.send(PlayerCommand::EmptyTimeout);
                    }));
                }
            }
            PlayerCommand::CancelEmptyDisconnect => {
                if let Some(handle) = self.empty_timer.take() {
                    handle.abort();
                }
            }
            PlayerCommand::IdleTimeout => {
                self.idle_timer = None;
                if self.state.should_disconnect_when_idle() {
                    let _ = lifecycle::disconnect(&self.songbird, self.guild_id).await;
                    self.call = None;
                    self.channel_id = None;
                    self.persist_last_voice_channel(None);
                }
            }
            PlayerCommand::EmptyTimeout => {
                self.empty_timer = None;
                // Regression fix: honour stay_connected before disconnecting on an empty channel.
                if self.state.should_disconnect_when_empty() {
                    self.resolve_pending_play(false, false);
                    self.current_generation += 1;
                    if let Some(handle) = self.prefetch_task.take() {
                        handle.abort();
                    }
                    if let Some(handle) = self.current_handle.take() {
                        let _ = handle.stop();
                    }
                    self.state.clear();
                    self.state.current = None;
                    self.is_paused = false;
                    self.cancel_idle_timer();
                    let _ = lifecycle::disconnect(&self.songbird, self.guild_id).await;
                    self.call = None;
                    self.channel_id = None;
                    self.persist_last_voice_channel(None);
                }
            }
            PlayerCommand::Shutdown => {
                self.cancel_timers();
                let _ = lifecycle::disconnect(&self.songbird, self.guild_id).await;
            }
            PlayerCommand::ResolveDone { generation, result } => {
                self.handle_resolve_done(generation, result).await;
            }
            PlayerCommand::AutoplayReady(track) => {
                self.handle_autoplay_ready(track);
            }
        }
    }

    async fn handle_play(
        &mut self,
        track: Track,
        front: bool,
        channel_id: ChannelId,
        reply: oneshot::Sender<Result<PlayOutcome>>,
    ) {
        tracing::info!(guild_id = %self.guild_id, title = %track.title, front, "handle_play: received");

        if self.state.is_full() {
            tracing::warn!(guild_id = %self.guild_id, "handle_play: queue is full");
            let _ = reply.send(Err(BotError::QueueFull));
            return;
        }

        if self.config.max_queue_size_per_user > 0
            && self.state.user_queue_count(track.requester_id) >= self.config.max_queue_size_per_user
        {
            tracing::info!(guild_id = %self.guild_id, requester_id = track.requester_id, "handle_play: per-user queue cap reached");
            let _ = reply.send(Err(BotError::UserQueueFull));
            return;
        }

        let was_idle = self.state.current.is_none();

        if front {
            self.state.push_front(track);
        } else {
            self.state.push_back(track);
        }
        let position = if front { 1 } else { self.state.queue.len() };

        if was_idle {
            tracing::info!(guild_id = %self.guild_id, "handle_play: nothing currently playing, advancing queue");
            self.cancel_idle_timer();
            self.retried_current_track = false;
            if self.state.advance().is_none() {
                let _ = reply.send(Ok(PlayOutcome {
                    position,
                    now_playing: false,
                    failed: true,
                }));
                tracing::info!(guild_id = %self.guild_id, "handle_play: done");
                return;
            }
            self.pending_play_reply = Some((reply, position));
            self.spawn_resolve_for_current();

            if let Err(e) = self.ensure_connected(channel_id).await {
                tracing::warn!(guild_id = %self.guild_id, "handle_play: voice connect failed, discarding in-flight resolve");
                self.current_generation += 1;
                self.state.current = None;
                if let Some((reply, _)) = self.pending_play_reply.take() {
                    let _ = reply.send(Err(e));
                }
            }
        } else {
            if let Err(e) = self.ensure_connected(channel_id).await {
                let _ = reply.send(Err(e));
                return;
            }
            self.spawn_prefetch();
            let _ = reply.send(Ok(PlayOutcome {
                position,
                now_playing: false,
                failed: false,
            }));
        }

        tracing::info!(guild_id = %self.guild_id, "handle_play: done");
    }

    /// Connects or switches channels as needed; a no-op if already there.
    async fn ensure_connected(&mut self, channel_id: ChannelId) -> Result<()> {
        if self.call.is_some() && self.channel_id == Some(channel_id) {
            return Ok(());
        }
        tracing::info!(guild_id = %self.guild_id, channel_id = %channel_id, "handle_play: connecting to voice channel");
        let connect_start = std::time::Instant::now();
        let call = lifecycle::connect(&self.songbird, self.guild_id, channel_id).await?;
        tracing::info!(guild_id = %self.guild_id, elapsed = ?connect_start.elapsed(), "handle_play: voice connect finished");
        self.call = Some(call);
        self.channel_id = Some(channel_id);
        self.persist_last_voice_channel(Some(channel_id));
        Ok(())
    }

    /// Answers a `!play` waiting on the first track's background resolve, if one is pending.
    fn resolve_pending_play(&mut self, now_playing: bool, failed: bool) {
        if let Some((reply, position)) = self.pending_play_reply.take() {
            let _ = reply.send(Ok(PlayOutcome {
                position,
                now_playing,
                failed,
            }));
        }
    }

    /// Shared by TrackEnded/TrackErrored; an errored track isn't requeued for loop mode.
    async fn handle_track_finished(&mut self, generation: u64, error_reason: Option<String>) {
        if generation != self.current_generation
            || self.last_finished_generation == Some(generation)
        {
            return;
        }
        self.last_finished_generation = Some(generation);
        self.current_handle = None;
        self.is_paused = false;
        let finished = self.state.current.clone();
        self.extractor.record_playback_outcome(error_reason.is_none());
        if let Some(reason) = &error_reason {
            if let Some(t) = &finished {
                tracing::warn!(guild_id = %self.guild_id, title = %t.title, reason = %reason, "track errored during playback");
            }
            self.handle_resolve_failure().await;
        } else {
            if let Some(f) = finished.clone() {
                self.state.requeue_finished(f);
            }
            self.advance_and_resolve_next();
        }
        self.maybe_autoplay(finished);
    }

    /// Retries once with a cache-bypassing resolve; on a second failure, drops it and moves on.
    async fn handle_resolve_failure(&mut self) {
        if !self.retried_current_track {
            self.retried_current_track = true;
            if let Some(t) = self.state.current.clone() {
                self.extractor.invalidate_stream(&t.webpage_url).await;
                tracing::info!(guild_id = %self.guild_id, title = %t.title, "retrying once with a fresh resolve");
                self.spawn_resolve_for_current();
            }
        } else {
            tracing::warn!(guild_id = %self.guild_id, "already retried this track once, discarding and moving on");
            self.discard_current_and_advance();
        }
    }

    /// Nothing to do if something already landed in `current`; the search runs in the background.
    fn maybe_autoplay(&mut self, seed: Option<Track>) {
        if self.state.current.is_some() {
            return;
        }
        if self.state.autoplay && self.lastfm.is_some() {
            if let Some(seed) = seed {
                self.spawn_autoplay_search(seed);
                return;
            }
        }
        if self.state.should_disconnect_when_idle() {
            self.arm_idle_timer();
        }
    }

    /// Runs the Last.fm + yt-dlp search off the actor task; reports back via `AutoplayReady`.
    fn spawn_autoplay_search(&mut self, seed: Track) {
        let Some(lastfm) = self.lastfm.clone() else { return };
        let extractor = self.extractor.clone();
        let self_tx = self.self_tx.clone();
        let guild_id = self.guild_id;
        tokio::spawn(async move {
            let track = find_autoplay_track(&lastfm, &extractor, &seed, guild_id).await;
            let _ = self_tx.send(PlayerCommand::AutoplayReady(track));
        });
    }

    /// Queues the found track, unless autoplay/the call ended while the search ran.
    fn handle_autoplay_ready(&mut self, track: Option<Track>) {
        if let Some(track) = track.filter(|_| self.state.autoplay && self.call.is_some()) {
            let was_idle = self.state.current.is_none();
            self.state.push_back(track);
            if was_idle {
                self.advance_and_resolve_next();
            }
            return;
        }
        if self.state.current.is_none() && self.state.should_disconnect_when_idle() {
            self.arm_idle_timer();
        }
    }

    /// Pops the next queued track and starts resolving it in the background, without waiting.
    fn advance_and_resolve_next(&mut self) {
        self.retried_current_track = false;
        if self.state.advance().is_some() {
            self.spawn_resolve_for_current();
        } else {
            tracing::info!(guild_id = %self.guild_id, "queue exhausted, nothing playing");
        }
    }

    /// Like `advance_and_resolve_next`, but for a track abandoned before it ever played.
    fn discard_current_and_advance(&mut self) {
        self.state.current = self.state.pop_front();
        self.retried_current_track = false;
        if self.state.current.is_some() {
            self.spawn_resolve_for_current();
        } else {
            tracing::info!(guild_id = %self.guild_id, "queue exhausted, nothing playing");
            self.resolve_pending_play(false, true);
        }
    }

    /// Backgrounds the resolve and reports via `ResolveDone` — inline would freeze !skip/!stop.
    fn spawn_resolve_for_current(&mut self) {
        let Some(track) = self.state.current.clone() else {
            return;
        };
        self.cancel_idle_timer();
        // Don't let a stale prefetch compete with this resolve.
        if let Some(handle) = self.prefetch_task.take() {
            handle.abort();
        }
        self.current_generation += 1;
        let generation = self.current_generation;
        let extractor = self.extractor.clone();
        let self_tx = self.self_tx.clone();
        let guild_id = self.guild_id;
        let title = track.title.clone();
        // Not "android" — it only ever gave us HLS-only formats.
        let client_override = self.retried_current_track.then_some("tv");
        tracing::info!(%guild_id, %title, generation, retry = self.retried_current_track, "spawn_resolve_for_current: resolving in background");
        tokio::spawn(async move {
            let result = extractor.resolve_stream(&track, client_override).await;
            let _ = self_tx.send(PlayerCommand::ResolveDone { generation, result });
        });
    }

    /// Starts playback on a successful resolve, else routes into the same retry/give-up path.
    async fn handle_resolve_done(
        &mut self,
        generation: u64,
        result: Result<crate::extraction::ResolvedInfo>,
    ) {
        if generation != self.current_generation {
            return;
        }
        let Some(track) = self.state.current.clone() else {
            return;
        };
        let seed = Some(track.clone());
        match result {
            Ok(resolved) => match self.finish_starting_track(track, resolved, generation).await {
                Ok(()) => {
                    self.resolve_pending_play(true, false);
                    self.spawn_prefetch();
                    return;
                }
                Err(e) => {
                    tracing::warn!(guild_id = %self.guild_id, error = %e, "handle_resolve_done: failed to start playback");
                    self.handle_resolve_failure().await;
                }
            },
            Err(e) => {
                tracing::warn!(guild_id = %self.guild_id, error = %e, "handle_resolve_done: resolve failed");
                self.handle_resolve_failure().await;
            }
        }
        self.maybe_autoplay(seed);
    }

    /// Hands a resolved stream to songbird — shared by every playback-start path now.
    async fn finish_starting_track(
        &mut self,
        track: Track,
        resolved: crate::extraction::ResolvedInfo,
        generation: u64,
    ) -> Result<()> {
        let Some(call) = self.call.clone() else {
            tracing::warn!(guild_id = %self.guild_id, "finish_starting_track: no active voice call");
            return Err(BotError::NotInVoiceChannel);
        };

        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in &resolved.headers {
            let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else {
                tracing::warn!(guild_id = %self.guild_id, header = %name, "finish_starting_track: skipping header with invalid name from yt-dlp");
                continue;
            };
            let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) else {
                tracing::warn!(guild_id = %self.guild_id, header = %name, "finish_starting_track: skipping header with invalid value from yt-dlp");
                continue;
            };
            headers.insert(header_name, header_value);
        }
        tracing::info!(
            guild_id = %self.guild_id,
            header_count = headers.len(),
            content_length = ?resolved.content_length,
            "finish_starting_track: built HttpRequest with headers/content_length from yt-dlp",
        );

        let input: Input = HttpRequest {
            client: self.http_client.clone(),
            request: resolved.stream_url,
            headers,
            content_length: resolved.content_length,
        }
        .into();
        tracing::info!(guild_id = %self.guild_id, generation, "finish_starting_track: handing input to songbird");
        let handle = {
            let mut call_guard = call.lock().await;
            call_guard.play_only_input(input)
        };
        let end_notifier = TrackEndNotifier {
            tx: self.self_tx.clone(),
            generation,
            errored: false,
        };
        let _ = handle.add_event(Event::Track(TrackEvent::End), end_notifier);
        let error_notifier = TrackEndNotifier {
            tx: self.self_tx.clone(),
            generation,
            errored: true,
        };
        let _ = handle.add_event(Event::Track(TrackEvent::Error), error_notifier);
        let _ = handle.set_volume(f32::from(self.volume) / 100.0);

        self.current_handle = Some(handle);
        self.is_paused = false;
        self.track_started_at = Some(std::time::Instant::now());
        self.paused_since = None;
        self.paused_total = std::time::Duration::ZERO;
        tracing::info!(guild_id = %self.guild_id, title = %track.title, "finish_starting_track: playback started");

        let db = self.db.clone();
        let guild_id = self.guild_id.get();
        let title = track.title.clone();
        let webpage_url = track.webpage_url.clone();
        let requester_id = track.requester_id;
        let duration = track.duration;
        tokio::spawn(async move {
            if let Err(e) = db
                .add_play_history(guild_id, &title, &webpage_url, requester_id, duration)
                .await
            {
                tracing::warn!(guild_id = guild_id, error = %e, "finish_starting_track: failed to record play history");
            }
        });

        Ok(())
    }

    /// Sequential, not parallel — parallel extraction competes with live Opus encoding for the same core.
    fn spawn_prefetch(&mut self) {
        if let Some(handle) = self.prefetch_task.take() {
            handle.abort();
        }
        let count = self.config.ytdlp_prefetch_count;
        if count == 0 {
            return;
        }
        let tracks: Vec<Track> = self.state.queue.iter().take(count).cloned().collect();
        let extractor = self.extractor.clone();
        let guild_id = self.guild_id;
        let join_handle = tokio::spawn(async move {
            for track in tracks {
                let title = track.title.clone();
                match extractor.try_resolve_stream(&track).await {
                    Some(Ok(_)) => tracing::info!(%guild_id, %title, "spawn_prefetch: resolved"),
                    Some(Err(e)) => {
                        tracing::warn!(%guild_id, %title, error = %e, "spawn_prefetch: failed")
                    }
                    None => {
                        tracing::debug!(%guild_id, %title, "spawn_prefetch: skipped, extractor busy — will resolve on-demand instead")
                    }
                }
            }
        });
        self.prefetch_task = Some(join_handle.abort_handle());
    }

    fn arm_idle_timer(&mut self) {
        if self.idle_timer.is_some() {
            return;
        }
        let tx = self.self_tx.clone();
        let timeout = Duration::from_secs(self.config.idle_timeout_secs);
        self.idle_timer = Some(tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = tx.send(PlayerCommand::IdleTimeout);
        }));
    }

    fn cancel_idle_timer(&mut self) {
        if let Some(handle) = self.idle_timer.take() {
            handle.abort();
        }
    }

    fn cancel_timers(&mut self) {
        self.cancel_idle_timer();
        if let Some(handle) = self.empty_timer.take() {
            handle.abort();
        }
    }
}

/// Strips YouTube's auto-generated "Artist - Topic" channel suffix for Last.fm's lookup.
fn clean_artist_name(uploader: &str) -> String {
    uploader.trim_end_matches(" - Topic").trim().to_owned()
}

/// Free function, not a method — lets the search run entirely off the actor task.
async fn find_autoplay_track(
    lastfm: &LastFmClient,
    extractor: &Extractor,
    seed: &Track,
    guild_id: GuildId,
) -> Option<Track> {
    let seed_artist = clean_artist_name(&seed.uploader);
    if seed_artist.is_empty() {
        return None;
    }
    tracing::info!(%guild_id, seed_artist = %seed_artist, "autoplay: querying Last.fm");
    let similar = match lastfm.similar_artists(&seed_artist, 5).await {
        Ok(a) => a,
        Err(e) => {
            tracing::warn!(%guild_id, error = %e, "autoplay: Last.fm lookup failed");
            return None;
        }
    };
    for artist in similar {
        if let Ok(Some(track)) = extractor.search_top(&artist, seed.requester_id).await {
            tracing::info!(%guild_id, %artist, title = %track.title, "autoplay: found a candidate");
            return Some(track);
        }
    }
    tracing::info!(%guild_id, "autoplay: nothing playable found");
    None
}
