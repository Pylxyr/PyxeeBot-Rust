use std::sync::Arc;

use poise::serenity_prelude::{ChannelId, GuildId};
use songbird::{Call, Songbird};
use tokio::sync::Mutex;

use crate::errors::{BotError, Result};

/// Joins (or switches to) the given voice channel. Songbird's manager
/// transparently reuses an existing `Call` or creates one as needed, and
/// switches channel if already connected elsewhere in this guild — there is
/// no separate "stale client" cleanup step required here.
pub async fn connect(
    songbird: &Songbird,
    guild_id: GuildId,
    channel_id: ChannelId,
) -> Result<Arc<Mutex<Call>>> {
    tracing::info!(guild_id = %guild_id, channel_id = %channel_id, "lifecycle::connect: joining");
    let start = std::time::Instant::now();
    let result = songbird
        .join(guild_id, channel_id)
        .await
        .map_err(|e| BotError::Voice(e.to_string()));
    match &result {
        Ok(_) => {
            tracing::info!(guild_id = %guild_id, elapsed = ?start.elapsed(), "lifecycle::connect: joined")
        }
        Err(e) => {
            tracing::error!(guild_id = %guild_id, elapsed = ?start.elapsed(), error = %e, "lifecycle::connect: failed")
        }
    }
    result
}

pub async fn disconnect(songbird: &Songbird, guild_id: GuildId) -> Result<()> {
    tracing::info!(guild_id = %guild_id, "lifecycle::disconnect: leaving");
    songbird
        .remove(guild_id)
        .await
        .map_err(|e| BotError::Voice(e.to_string()))
}
