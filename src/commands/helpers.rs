use poise::serenity_prelude::{Member, Permissions};

use crate::bot::Context;

/// DJ = has the configured DJ role, or Manage Channels, or is a bot owner.
/// Mirrors the Python bot's `_require_dj` check.
pub async fn is_dj(ctx: Context<'_>) -> bool {
    let Some(member) = ctx.author_member().await else {
        return false;
    };
    if has_manage_channels(ctx, &member) {
        return true;
    }
    if ctx.framework().options().owners.contains(&ctx.author().id) {
        return true;
    }
    let Some(guild_id) = ctx.guild_id() else {
        return false;
    };
    let Some(dj_role) = ctx.data().db.get_dj_role_id(guild_id.get()).await else {
        return false;
    };
    member.roles.iter().any(|r| r.get() == dj_role)
}

fn has_manage_channels(ctx: Context<'_>, member: &Member) -> bool {
    let Some(guild_id) = ctx.guild_id() else {
        return false;
    };
    let channel_id = ctx.channel_id();
    let Some(guild) = ctx.serenity_context().cache.guild(guild_id) else {
        return false;
    };
    let Some(channel) = guild.channels.get(&channel_id) else {
        return false;
    };
    guild
        .user_permissions_in(channel, member)
        .contains(Permissions::MANAGE_CHANNELS)
}

pub async fn require_dj(ctx: Context<'_>) -> anyhow::Result<bool> {
    if is_dj(ctx).await {
        Ok(true)
    } else {
        ctx.say("You need the DJ role or Manage Channels permission for this.")
            .await?;
        Ok(false)
    }
}

/// Restricts playback-control commands (skip/stop/pause/resume/previous/
/// loop) to users actually in the bot's voice channel, so someone in an
/// unrelated text or voice channel can't disrupt a session they're not
/// part of. DJs are exempt, same as `require_dj` lets them act from
/// anywhere. If the bot isn't connected to a channel at all, there's
/// nothing to be "wrong" about, so this allows the action through (whatever
/// the command does in that case, e.g. reporting nothing is playing, is
/// its own concern).
pub async fn require_same_voice_channel(ctx: Context<'_>) -> anyhow::Result<bool> {
    if is_dj(ctx).await {
        return Ok(true);
    }
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(true);
    };
    let bot_channel = ctx.data().player_for(guild_id).await.snapshot().channel_id;
    let Some(bot_channel) = bot_channel else {
        return Ok(true);
    };
    let user_channel = ctx
        .serenity_context()
        .cache
        .guild(guild_id)
        .and_then(|g| g.voice_states.get(&ctx.author().id).and_then(|vs| vs.channel_id));
    if user_channel == Some(bot_channel) {
        Ok(true)
    } else {
        ctx.say("You need to be in the same voice channel as the bot for this.")
            .await?;
        Ok(false)
    }
}
