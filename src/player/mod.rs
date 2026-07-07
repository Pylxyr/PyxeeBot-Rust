mod actor;
mod lifecycle;
mod queue;
mod snapshot;

use std::sync::Arc;

use poise::serenity_prelude::{ChannelId, GuildId};
use songbird::Songbird;
use tokio::sync::{oneshot, watch};

pub use actor::PlayOutcome;
pub use queue::PlayerState;
pub use snapshot::PlayerSnapshot;

use crate::config::Config;
use crate::errors::{BotError, Result};
use crate::extraction::Extractor;
use crate::lastfm::LastFmClient;
use crate::models::{LoopMode, Track};

use actor::{PlayerActor, PlayerCommand};

/// Public handle to a guild's player actor. Cheap to clone (wraps an
/// unbounded sender and a watch receiver); the actual state lives entirely
/// inside the spawned actor task.
pub struct GuildPlayer {
    tx: tokio::sync::mpsc::UnboundedSender<PlayerCommand>,
    snapshot_rx: watch::Receiver<PlayerSnapshot>,
}

impl GuildPlayer {
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
    ) -> Arc<Self> {
        let (tx, snapshot_rx) = PlayerActor::spawn(
            guild_id,
            songbird,
            extractor,
            http_client,
            lastfm,
            config,
            stay_connected,
            autoplay,
        );
        Arc::new(Self { tx, snapshot_rx })
    }

    pub fn snapshot(&self) -> PlayerSnapshot {
        self.snapshot_rx.borrow().clone()
    }

    fn send(&self, cmd: PlayerCommand) {
        let _ = self.tx.send(cmd);
    }

    pub async fn play(
        &self,
        track: Track,
        front: bool,
        channel_id: ChannelId,
    ) -> Result<PlayOutcome> {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::Play {
            track,
            front,
            channel_id,
            reply,
        });
        rx.await
            .map_err(|_| BotError::Voice("player actor is gone".to_owned()))?
    }

    pub fn skip(&self) {
        self.send(PlayerCommand::Skip);
    }

    pub fn stop(&self) {
        self.send(PlayerCommand::Stop);
    }

    pub fn pause(&self) {
        self.send(PlayerCommand::Pause);
    }

    pub fn resume(&self) {
        self.send(PlayerCommand::Resume);
    }

    pub async fn leave(&self) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::Leave { reply });
        rx.await
            .map_err(|_| BotError::Voice("player actor is gone".to_owned()))?
    }

    pub async fn connect(&self, channel_id: ChannelId) -> Result<()> {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::Connect { channel_id, reply });
        rx.await
            .map_err(|_| BotError::Voice("player actor is gone".to_owned()))?
    }

    pub fn set_stay_connected(&self, enabled: bool) {
        self.send(PlayerCommand::SetStay(enabled));
    }

    pub fn set_autoplay(&self, enabled: bool) {
        self.send(PlayerCommand::SetAutoplay(enabled));
    }

    pub async fn cycle_loop_mode(&self) -> LoopMode {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::CycleLoop { reply });
        rx.await.unwrap_or_default()
    }

    pub async fn previous(&self) -> bool {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::Previous { reply });
        rx.await.unwrap_or(false)
    }

    pub async fn clear_queue(&self) -> usize {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::ClearQueue { reply });
        rx.await.unwrap_or(0)
    }

    pub fn shuffle(&self) {
        self.send(PlayerCommand::Shuffle);
    }

    pub async fn remove_track(&self, position: usize) -> Option<Track> {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::RemoveTrack { position, reply });
        rx.await.ok().flatten()
    }

    pub async fn move_track(&self, from: usize, to: usize) -> bool {
        let (reply, rx) = oneshot::channel();
        self.send(PlayerCommand::MoveTrack { from, to, reply });
        rx.await.unwrap_or(false)
    }

    pub fn schedule_empty_disconnect(&self) {
        self.send(PlayerCommand::ScheduleEmptyDisconnect);
    }

    pub fn cancel_empty_disconnect(&self) {
        self.send(PlayerCommand::CancelEmptyDisconnect);
    }

    pub fn rejoin(&self, channel_id: ChannelId) {
        self.send(PlayerCommand::Rejoin { channel_id });
    }

    pub fn shutdown(&self) {
        self.send(PlayerCommand::Shutdown);
    }
}
