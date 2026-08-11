use poise::serenity_prelude::{Cache, ChannelId, GuildId, Member, Permissions, UserId};

use crate::db::Database;

/// DJ = configured DJ role, Manage Channels, or bot owner — shared by commands/helpers.rs and events.rs.
pub async fn is_dj(
    db: &Database,
    bot_owners: &[u64],
    cache: &Cache,
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
    member: Option<&Member>,
) -> bool {
    if bot_owners.contains(&user_id.get()) {
        return true;
    }
    if let Some(member) = member {
        if has_manage_channels(cache, guild_id, channel_id, member) {
            return true;
        }
    }
    let Some(dj_role) = db.get_dj_role_id(guild_id.get()).await else {
        return false;
    };
    member.is_some_and(|m| m.roles.iter().any(|r| r.get() == dj_role))
}

fn has_manage_channels(cache: &Cache, guild_id: GuildId, channel_id: ChannelId, member: &Member) -> bool {
    let Some(guild) = cache.guild(guild_id) else {
        return false;
    };
    let Some(channel) = guild.channels.get(&channel_id) else {
        return false;
    };
    guild
        .user_permissions_in(channel, member)
        .contains(Permissions::MANAGE_CHANNELS)
}

/// Vacuously true if the bot isn't connected anywhere — nothing to be "in the same channel" as.
pub fn in_same_voice_channel(
    cache: &Cache,
    guild_id: GuildId,
    bot_channel: Option<ChannelId>,
    user_id: UserId,
) -> bool {
    let Some(bot_channel) = bot_channel else {
        return true;
    };
    let user_channel = cache
        .guild(guild_id)
        .and_then(|g| g.voice_states.get(&user_id).and_then(|vs| vs.channel_id));
    user_channel == Some(bot_channel)
}
