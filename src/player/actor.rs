use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{ChannelId, GuildId};
use songbird::events::{Event, EventContext, EventHandler as SongbirdEventHandler, TrackEvent};
use songbird::input::{HttpRequest, Input};
use songbird::tracks::TrackHandle;
use songbird::{Call, Songbird};
use tokio::sync::{mpsc, oneshot, watch, Mutex as AsyncMutex};
use tokio::task::JoinHandle;

use crate::config::Config;
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
    /// True if nothing could actually be played — the whole queue (including
    /// what was just added) was tried and every candidate failed to resolve.
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
    Pause,
    Resume,
    Leave {
        reply: oneshot::Sender<Result<()>>,
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
        reply: oneshot::Sender<Option<Track>>,
    },
    MoveTrack {
        from: usize,
        to: usize,
        reply: oneshot::Sender<bool>,
    },
    /// Reconnects to a channel without touching queue/playback state. Used
    /// when stay_connected is on and the bot was force-kicked.
    Rejoin {
        channel_id: ChannelId,
    },
    /// Fired by the songbird EventHandler when a track ends, carrying the
    /// generation number it was registered under. If that no longer matches
    /// `current_generation`, this track was already superseded by a manual
    /// skip/previous/stop — the signal is stale and ignored.
    TrackEnded(u64),
    ScheduleEmptyDisconnect,
    CancelEmptyDisconnect,
    IdleTimeout,
    EmptyTimeout,
    Shutdown,
}

struct TrackEndNotifier {
    tx: mpsc::UnboundedSender<PlayerCommand>,
    generation: u64,
}

#[async_trait::async_trait]
impl SongbirdEventHandler for TrackEndNotifier {
    async fn act(&self, _ctx: &EventContext<'_>) -> Option<Event> {
        let _ = self.tx.send(PlayerCommand::TrackEnded(self.generation));
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
        stay_connected: bool,
        autoplay: bool,
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
        };
        tokio::spawn(actor.run());
        (tx, snapshot_rx)
    }

    async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            let is_shutdown = matches!(cmd, PlayerCommand::Shutdown);
            self.handle(cmd).await;
            self.publish_snapshot();
            if is_shutdown {
                break;
            }
        }
    }

    fn publish_snapshot(&self) {
        let is_connected = self.call.is_some();
        let _ = self
            .snapshot_tx
            .send(self.state.to_snapshot(is_connected, self.is_paused));
    }

    async fn handle(&mut self, cmd: PlayerCommand) {
        match cmd {
            PlayerCommand::Play {
                track,
                front,
                channel_id,
                reply,
            } => {
                let result = self.handle_play(track, front, channel_id).await;
                let _ = reply.send(result);
            }
            PlayerCommand::Skip => {
                if let Some(handle) = self.current_handle.take() {
                    let _ = handle.stop();
                }
                self.is_paused = false;
            }
            PlayerCommand::Stop => {
                self.state.clear();
                self.state.current = None;
                self.cancel_timers();
                if let Some(handle) = self.current_handle.take() {
                    let _ = handle.stop();
                }
                self.is_paused = false;
                self.arm_idle_timer();
            }
            PlayerCommand::Pause => {
                if let Some(handle) = &self.current_handle {
                    let _ = handle.pause();
                    self.is_paused = true;
                }
            }
            PlayerCommand::Resume => {
                if let Some(handle) = &self.current_handle {
                    let _ = handle.play();
                    self.is_paused = false;
                }
            }
            PlayerCommand::Leave { reply } => {
                self.cancel_timers();
                if let Some(handle) = self.current_handle.take() {
                    let _ = handle.stop();
                }
                self.state.clear();
                self.state.current = None;
                self.is_paused = false;
                let result = lifecycle::disconnect(&self.songbird, self.guild_id).await;
                self.call = None;
                self.channel_id = None;
                let _ = reply.send(result);
            }
            PlayerCommand::Connect { channel_id, reply } => {
                let outcome = lifecycle::connect(&self.songbird, self.guild_id, channel_id).await;
                let result = match outcome {
                    Ok(call) => {
                        self.call = Some(call);
                        self.channel_id = Some(channel_id);
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
                    self.cancel_idle_timer();
                    if let Some(handle) = self.current_handle.take() {
                        let _ = handle.stop();
                    }
                    if let Some(track) = self.state.current.clone() {
                        if let Err(e) = self.play_track(track).await {
                            tracing::warn!(guild_id = %self.guild_id, error = %e, "failed to play previous track");
                        }
                    }
                }
                let _ = reply.send(ok);
            }
            PlayerCommand::ClearQueue { reply } => {
                let n = self.state.clear();
                let _ = reply.send(n);
            }
            PlayerCommand::Shuffle => {
                self.state.shuffle();
            }
            PlayerCommand::RemoveTrack { position, reply } => {
                let removed = self.state.remove(position);
                let _ = reply.send(removed);
            }
            PlayerCommand::MoveTrack { from, to, reply } => {
                let ok = self.state.move_track(from, to);
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
            PlayerCommand::TrackEnded(generation) => {
                if generation != self.current_generation {
                    return;
                }
                self.current_handle = None;
                self.is_paused = false;
                let finished = self.state.current.clone();
                if let Some(finished) = finished.clone() {
                    self.state.requeue_finished(finished);
                }
                if let Err(e) = self.advance_and_play().await {
                    tracing::warn!(guild_id = %self.guild_id, error = %e, "failed to advance queue");
                }
                if self.state.current.is_none() {
                    if self.state.autoplay {
                        if let Some(seed) = &finished {
                            if let Err(e) = self.try_autoplay(seed).await {
                                tracing::warn!(guild_id = %self.guild_id, error = %e, "autoplay failed");
                            }
                        }
                    }
                    if self.state.current.is_none() && self.state.should_disconnect_when_idle() {
                        self.arm_idle_timer();
                    }
                }
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
                }
            }
            PlayerCommand::EmptyTimeout => {
                self.empty_timer = None;
                // The exact regression fix from the Python bug hunt: honour
                // stay_connected before disconnecting on an empty channel.
                if self.state.should_disconnect_when_empty(false) {
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
                }
            }
            PlayerCommand::Shutdown => {
                self.cancel_timers();
            }
        }
    }

    async fn handle_play(
        &mut self,
        track: Track,
        front: bool,
        channel_id: ChannelId,
    ) -> Result<PlayOutcome> {
        tracing::info!(guild_id = %self.guild_id, title = %track.title, front, "handle_play: received");

        if self.call.is_none() || self.channel_id != Some(channel_id) {
            tracing::info!(guild_id = %self.guild_id, channel_id = %channel_id, "handle_play: connecting to voice channel");
            let connect_start = std::time::Instant::now();
            let call = lifecycle::connect(&self.songbird, self.guild_id, channel_id).await?;
            tracing::info!(guild_id = %self.guild_id, elapsed = ?connect_start.elapsed(), "handle_play: voice connect finished");
            self.call = Some(call);
            self.channel_id = Some(channel_id);
        }

        if self.state.is_full() {
            tracing::warn!(guild_id = %self.guild_id, "handle_play: queue is full");
            return Err(BotError::QueueFull);
        }

        if front {
            self.state.push_front(track);
        } else {
            self.state.push_back(track);
        }

        let mut now_playing = false;
        let mut failed = false;
        if self.state.current.is_none() {
            tracing::info!(guild_id = %self.guild_id, "handle_play: nothing currently playing, advancing queue");
            self.cancel_idle_timer();
            now_playing = self.advance_and_play().await?;
            failed = !now_playing;
        }

        tracing::info!(guild_id = %self.guild_id, now_playing, failed, "handle_play: done");
        Ok(PlayOutcome {
            position: self.state.queue.len(),
            now_playing,
            failed,
        })
    }

    /// Pulls the next track off the queue and plays it, skipping over any
    /// track that fails to resolve (e.g. a dead link) rather than getting
    /// stuck. Returns true if something actually started playing, false if
    /// the queue was exhausted without success.
    async fn advance_and_play(&mut self) -> Result<bool> {
        loop {
            match self.state.advance() {
                Some(track) => {
                    let title = track.title.clone();
                    tracing::info!(guild_id = %self.guild_id, title = %title, "advance_and_play: trying track");
                    match self.play_track(track).await {
                        Ok(()) => {
                            tracing::info!(guild_id = %self.guild_id, title = %title, "advance_and_play: playing");
                            return Ok(true);
                        }
                        Err(e) => {
                            tracing::warn!(guild_id = %self.guild_id, title = %title, error = %e, "advance_and_play: failed to play track, skipping");
                            continue;
                        }
                    }
                }
                None => {
                    tracing::info!(guild_id = %self.guild_id, "advance_and_play: queue exhausted, nothing playing");
                    return Ok(false);
                }
            }
        }
    }

    async fn play_track(&mut self, track: Track) -> Result<()> {
        tracing::info!(guild_id = %self.guild_id, title = %track.title, url = %track.webpage_url, "play_track: resolving stream");
        let resolve_start = std::time::Instant::now();
        let resolved = self.extractor.resolve_stream(&track).await?;
        tracing::info!(
            guild_id = %self.guild_id,
            title = %track.title,
            elapsed = ?resolve_start.elapsed(),
            stream_url_len = resolved.stream_url.len(),
            acodec = %resolved.acodec,
            "play_track: stream resolved",
        );

        let Some(call) = self.call.clone() else {
            tracing::warn!(guild_id = %self.guild_id, "play_track: no active voice call");
            return Err(BotError::NotInVoiceChannel);
        };

        self.cancel_idle_timer();
        self.current_generation += 1;
        let generation = self.current_generation;

        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in &resolved.headers {
            let Ok(header_name) = reqwest::header::HeaderName::from_bytes(name.as_bytes()) else {
                tracing::warn!(guild_id = %self.guild_id, header = %name, "play_track: skipping header with invalid name from yt-dlp");
                continue;
            };
            let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) else {
                tracing::warn!(guild_id = %self.guild_id, header = %name, "play_track: skipping header with invalid value from yt-dlp");
                continue;
            };
            headers.insert(header_name, header_value);
        }
        tracing::info!(
            guild_id = %self.guild_id,
            header_count = headers.len(),
            content_length = ?resolved.content_length,
            "play_track: built HttpRequest with headers/content_length from yt-dlp",
        );

        let input: Input = HttpRequest {
            client: self.http_client.clone(),
            request: resolved.stream_url,
            headers,
            content_length: resolved.content_length,
        }
        .into();
        tracing::info!(guild_id = %self.guild_id, generation, "play_track: handing input to songbird");
        let handle = {
            let mut call_guard = call.lock().await;
            call_guard.play_only_input(input)
        };
        let notifier = TrackEndNotifier {
            tx: self.self_tx.clone(),
            generation,
        };
        let _ = handle.add_event(Event::Track(TrackEvent::End), notifier);

        self.current_handle = Some(handle);
        self.is_paused = false;
        tracing::info!(guild_id = %self.guild_id, title = %track.title, "play_track: playback started");
        Ok(())
    }

    /// Autoplay: finds an artist similar to the just-finished track via
    /// Last.fm, searches for a track by them, and plays it. A no-op if
    /// Last.fm isn't configured or nothing playable turns up — the caller
    /// falls back to arming the idle timer in that case.
    async fn try_autoplay(&mut self, seed: &Track) -> Result<()> {
        let Some(lastfm) = self.lastfm.clone() else {
            return Ok(());
        };
        let seed_artist = clean_artist_name(&seed.uploader);
        if seed_artist.is_empty() {
            return Ok(());
        }
        tracing::info!(guild_id = %self.guild_id, seed_artist = %seed_artist, "try_autoplay: querying Last.fm");

        let similar = lastfm
            .similar_artists(&seed_artist, 5)
            .await
            .map_err(|e| BotError::Voice(e.to_string()))?;
        tracing::info!(guild_id = %self.guild_id, count = similar.len(), "try_autoplay: got similar artists");

        for artist in similar {
            if let Ok(tracks) = self
                .extractor
                .search(&artist, seed.requester_id, true)
                .await
            {
                if let Some(track) = tracks.into_iter().next() {
                    tracing::info!(guild_id = %self.guild_id, artist = %artist, title = %track.title, "try_autoplay: queuing");
                    self.state.push_back(track);
                    self.advance_and_play().await?;
                    return Ok(());
                }
            }
        }
        tracing::info!(guild_id = %self.guild_id, "try_autoplay: nothing playable found");
        Ok(())
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

/// Strips common YouTube auto-generated channel suffixes (e.g. the
/// "Artist - Topic" pattern from YouTube Music) so the name reads as a
/// plain artist for Last.fm's lookup.
fn clean_artist_name(uploader: &str) -> String {
    uploader.trim_end_matches(" - Topic").trim().to_owned()
}
