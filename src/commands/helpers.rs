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
