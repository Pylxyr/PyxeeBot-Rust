use poise::serenity_prelude::ChannelId;

use crate::models::{LoopMode, Track};

#[derive(Debug, Clone, Default)]
pub struct PlayerSnapshot {
    pub current: Option<Track>,
    pub queue: Vec<Track>,
    pub history: Vec<Track>,
    pub loop_mode: LoopMode,
    pub stay_connected: bool,
    pub is_paused: bool,
    pub is_connected: bool,
    pub channel_id: Option<ChannelId>,
    pub total_duration_secs: i64,
    pub elapsed_secs: i64,
}
