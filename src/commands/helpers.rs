use crate::bot::Context;

pub async fn is_dj(ctx: Context<'_>) -> bool {
    let Some(guild_id) = ctx.guild_id() else {
        return false;
    };
    let member = ctx.author_member().await;
    crate::permissions::is_dj(
        &ctx.data().db,
        &ctx.data().config.bot_owners,
        &ctx.serenity_context().cache,
        guild_id,
        ctx.channel_id(),
        ctx.author().id,
        member.as_deref(),
    )
    .await
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

pub async fn require_same_voice_channel(ctx: Context<'_>) -> anyhow::Result<bool> {
    if is_dj(ctx).await {
        return Ok(true);
    }
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(true);
    };
    let bot_channel = ctx.data().player_for(guild_id).await.snapshot().channel_id;
    if crate::permissions::in_same_voice_channel(
        &ctx.serenity_context().cache,
        guild_id,
        bot_channel,
        ctx.author().id,
    ) {
        Ok(true)
    } else {
        ctx.say("You need to be in the same voice channel as the bot for this.")
            .await?;
        Ok(false)
    }
}
