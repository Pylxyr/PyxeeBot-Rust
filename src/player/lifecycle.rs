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
    songbird
        .join(guild_id, channel_id)
        .await
        .map_err(|e| BotError::Voice(e.to_string()))
}

pub async fn disconnect(songbird: &Songbird, guild_id: GuildId) -> Result<()> {
    songbird
        .remove(guild_id)
        .await
        .map_err(|e| BotError::Voice(e.to_string()))
}
