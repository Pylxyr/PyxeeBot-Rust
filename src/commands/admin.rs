use poise::serenity_prelude::{Role, RoleId};

use super::helpers::require_dj;
use crate::bot::Context;

/// Toggle 24/7 mode (stay connected even when the channel empties).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn stay(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    let data = ctx.data();
    let current = data.db.get_stay_connected(guild_id.get()).await;
    let new_value = !current;
    data.db
        .set_stay_connected(guild_id.get(), new_value, &data.config.default_prefix)
        .await?;
    data.player_for(guild_id)
        .await
        .set_stay_connected(new_value);
    let state = if new_value { "enabled" } else { "disabled" };
    ctx.say(format!("24/7 mode {state}.")).await?;
    Ok(())
}

/// Set the DJ role for this server.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn setdj(ctx: Context<'_>, role: Role) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    let data = ctx.data();
    data.db
        .set_dj_role_id(
            guild_id.get(),
            Some(role.id.get()),
            &data.config.default_prefix,
        )
        .await?;
    ctx.say(format!("DJ role set to {}.", role.name)).await?;
    Ok(())
}

/// Clear the DJ role for this server.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn cleardj(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    let data = ctx.data();
    data.db
        .set_dj_role_id(guild_id.get(), None, &data.config.default_prefix)
        .await?;
    ctx.say("DJ role cleared.").await?;
    Ok(())
}

/// Show the current DJ role for this server.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn dj(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    match ctx.data().db.get_dj_role_id(guild_id.get()).await {
        Some(role_id) => {
            ctx.say(format!("DJ role: <@&{}>", RoleId::new(role_id)))
                .await?
        }
        None => {
            ctx.say("No DJ role set — anyone with Manage Channels can use DJ commands.")
                .await?
        }
    };
    Ok(())
}

/// Change the command prefix for this server.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn setprefix(ctx: Context<'_>, prefix: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    if prefix.trim().is_empty() || prefix.contains(' ') || prefix.len() > 8 {
        ctx.say("Prefix must be 1-8 characters with no spaces.")
            .await?;
        return Ok(());
    }
    ctx.data()
        .db
        .set_prefix(guild_id.get(), prefix.trim())
        .await?;
    ctx.say(format!("Prefix set to `{}`.", prefix.trim()))
        .await?;
    Ok(())
}

/// Show bot status and basic stats.
#[poise::command(prefix_command, slash_command)]
pub async fn stats(ctx: Context<'_>) -> anyhow::Result<()> {
    let guild_count = ctx.serenity_context().cache.guild_count();
    ctx.say(format!(
        "Serving {guild_count} server(s). Prefix: `{}` (default).",
        ctx.data().config.default_prefix
    ))
    .await?;
    Ok(())
}
