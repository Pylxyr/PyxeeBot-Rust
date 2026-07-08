use poise::serenity_prelude::ChannelId;

use crate::bot::Context;

fn voice_channel_of(
    ctx: Context<'_>,
    guild_id: poise::serenity_prelude::GuildId,
) -> Option<ChannelId> {
    ctx.serenity_context()
        .cache
        .guild(guild_id)?
        .voice_states
        .get(&ctx.author().id)
        .and_then(|vs| vs.channel_id)
}

/// Join your current voice channel.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn join(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let Some(channel_id) = voice_channel_of(ctx, guild_id) else {
        ctx.say("Join a voice channel first.").await?;
        return Ok(());
    };
    let player = ctx.data().player_for(guild_id).await;
    match player.connect(channel_id).await {
        Ok(()) => ctx.say(format!("Joined <#{channel_id}>.")).await?,
        Err(e) => ctx.say(format!("Couldn't join: {e}")).await?,
    };
    Ok(())
}

/// Leave the voice channel and clear the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn leave(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let player = ctx.data().player_for(guild_id).await;
    match player.leave().await {
        Ok(()) => ctx.say("Left the voice channel.").await?,
        Err(e) => ctx.say(format!("Couldn't leave: {e}")).await?,
    };
    Ok(())
}

/// Search and play (or queue) a track.
#[poise::command(prefix_command, slash_command, guild_only, aliases("p"))]
pub async fn play(ctx: Context<'_>, #[rest] query: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let author_id = ctx.author().id;
    let Some(channel_id) = voice_channel_of(ctx, guild_id) else {
        ctx.say("Join a voice channel first.").await?;
        return Ok(());
    };

    tracing::info!(guild_id = %guild_id, user = %author_id, query = %query, "!play: received");
    let handle = ctx.say(format!("Searching for `{query}`...")).await?;
    let data = ctx.data();

    let search_start = std::time::Instant::now();
    let tracks = match data.extractor.search(&query, author_id.get(), false).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(guild_id = %guild_id, query = %query, elapsed = ?search_start.elapsed(), error = %e, "!play: search failed");
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("Search failed: {e}")),
                )
                .await?;
            return Ok(());
        }
    };
    tracing::info!(guild_id = %guild_id, query = %query, elapsed = ?search_start.elapsed(), results = tracks.len(), "!play: search finished");

    let Some(track) = tracks.into_iter().next() else {
        tracing::info!(guild_id = %guild_id, query = %query, "!play: no results");
        handle
            .edit(
                ctx,
                poise::CreateReply::default().content("No results found."),
            )
            .await?;
        return Ok(());
    };

    let player = data.player_for(guild_id).await;
    let title = track.escaped_title();
    tracing::info!(guild_id = %guild_id, title = %track.title, url = %track.webpage_url, "!play: track selected, calling player.play");
    let play_start = std::time::Instant::now();
    let result = player.play(track, false, channel_id).await;
    tracing::info!(guild_id = %guild_id, elapsed = ?play_start.elapsed(), ok = result.is_ok(), "!play: player.play returned");

    match result {
        Ok(outcome) if outcome.failed => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!(
                        "Couldn't play **{title}** — check the bot logs for details."
                    )),
                )
                .await?;
        }
        Ok(outcome) if outcome.now_playing => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("Now playing: **{title}**")),
                )
                .await?;
        }
        Ok(outcome) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!(
                        "Queued **{title}** — position {}.",
                        outcome.position
                    )),
                )
                .await?;
        }
        Err(e) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("Error: {e}")),
                )
                .await?;
        }
    }
    Ok(())
}

/// Skip the current track.
#[poise::command(prefix_command, slash_command, guild_only, aliases("s"))]
pub async fn skip(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    ctx.data().player_for(guild_id).await.skip();
    ctx.say("Skipped.").await?;
    Ok(())
}

/// Stop playback and clear the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn stop(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    ctx.data().player_for(guild_id).await.stop();
    ctx.say("Stopped and cleared the queue.").await?;
    Ok(())
}

/// Pause the current track.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn pause(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    ctx.data().player_for(guild_id).await.pause();
    ctx.say("Paused.").await?;
    Ok(())
}

/// Resume the current track.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn resume(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    ctx.data().player_for(guild_id).await.resume();
    ctx.say("Resumed.").await?;
    Ok(())
}

/// Go back to the previous track.
#[poise::command(prefix_command, slash_command, guild_only, aliases("prev"))]
pub async fn previous(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let ok = ctx.data().player_for(guild_id).await.previous().await;
    if ok {
        ctx.say("Playing the previous track.").await?;
    } else {
        ctx.say("No previous track.").await?;
    }
    Ok(())
}

/// Cycle loop mode: off -> single track -> entire queue -> off.
#[poise::command(
    prefix_command,
    slash_command,
    guild_only,
    rename = "loop",
    aliases("repeat")
)]
pub async fn loop_cmd(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let mode = ctx
        .data()
        .player_for(guild_id)
        .await
        .cycle_loop_mode()
        .await;
    ctx.say(format!("Loop mode: {}", mode.label())).await?;
    Ok(())
}

/// Show what's currently playing.
#[poise::command(prefix_command, slash_command, guild_only, aliases("np"))]
pub async fn nowplaying(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let snapshot = ctx.data().player_for(guild_id).await.snapshot();
    let content = crate::components::now_playing_content(&snapshot);
    let buttons = crate::components::now_playing_buttons(&snapshot);
    ctx.send(
        poise::CreateReply::default()
            .content(content)
            .components(buttons),
    )
    .await?;
    Ok(())
}
