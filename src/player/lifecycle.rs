use std::sync::Arc;

use poise::serenity_prelude::{ChannelId, GuildId};
use songbird::{Call, Songbird};
use tokio::sync::Mutex;

use crate::errors::{BotError, Result};

pub async fn connect(
    songbird: &Songbird,
    guild_id: GuildId,
    channel_id: ChannelId,
    opus_bitrate_kbps: u32,
) -> Result<Arc<Mutex<Call>>> {
    tracing::info!(guild_id = %guild_id, channel_id = %channel_id, "lifecycle::connect: joining");
    let start = std::time::Instant::now();
    let result = songbird
        .join(guild_id, channel_id)
        .await
        .map_err(|e| BotError::Voice(e.to_string()));

    if let Ok(call) = &result {
        call.lock()
            .await
            .set_bitrate(songbird::driver::Bitrate::Bits(
                opus_bitrate_kbps as i32 * 1000,
            ));
    }
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

pub async fn disconnect(songbird: &Songbird, guild_id: GuildId) -> Result<bool> {
    if songbird.get(guild_id).is_none() {
        tracing::info!(guild_id = %guild_id, "lifecycle::disconnect: nothing to leave");
        return Ok(false);
    }
    tracing::info!(guild_id = %guild_id, "lifecycle::disconnect: leaving");
    songbird
        .remove(guild_id)
        .await
        .map_err(|e| BotError::Voice(e.to_string()))?;
    Ok(true)
}
