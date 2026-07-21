use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{ChannelId, EditMessage, Http, MessageId};

use crate::bot::Context;
use crate::player::GuildPlayer;

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
    play_or_queue(ctx, query, false).await
}


#[poise::command(prefix_command, slash_command, guild_only, aliases("pn"))]
pub async fn playnext(ctx: Context<'_>, #[rest] query: String) -> anyhow::Result<()> {
    play_or_queue(ctx, query, true).await
}

async fn play_or_queue(ctx: Context<'_>, query: String, front: bool) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let author_id = ctx.author().id;
    let Some(channel_id) = voice_channel_of(ctx, guild_id) else {
        ctx.say("Join a voice channel first.").await?;
        return Ok(());
    };

    tracing::info!(guild_id = %guild_id, user = %author_id, query = %query, front, "!play: received");
    let data = ctx.data();

    let search_start = std::time::Instant::now();
    let trimmed = query.trim();
    let is_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    let say_fut = ctx.say(format!("Searching for `{query}`..."));
    let resolve_fut = async {
        if is_url {
            data.extractor.extract_url(trimmed, author_id.get(), false).await
        } else {
            data.extractor.search(&query, author_id.get(), false).await
        }
    };
    // Sending the "Searching..." message and running the actual search are
    // independent, so overlap them instead of paying the Discord API
    // round-trip before the search even starts.
    let (handle, resolve_result) = tokio::join!(say_fut, resolve_fut);
    let handle = handle?;
    let tracks = match resolve_result {
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
    let result = player.play(track, front, channel_id).await;
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
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
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
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
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
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
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
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
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
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
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
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
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
    let player = ctx.data().player_for(guild_id).await;
    let snapshot = player.snapshot();
    let content = crate::components::now_playing_content(&snapshot);
    let buttons = crate::components::now_playing_buttons(&snapshot);
    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(content)
                .components(buttons),
        )
        .await?;

    if ctx.data().config.np_auto_refresh {
        if let Ok(message) = reply.message().await {
            let http = ctx.serenity_context().http.clone();
            let channel_id = ctx.channel_id();
            let message_id = message.id;
            let interval_secs = u64::from(ctx.data().config.np_auto_refresh_interval);
            tokio::spawn(refresh_now_playing(
                http,
                channel_id,
                message_id,
                player,
                interval_secs,
            ));
        }
    }
    Ok(())
}

/// Background loop for `NP_AUTO_REFRESH`: edits the `!nowplaying` message in
/// place every `interval_secs` until nothing is playing anymore, the message
/// is gone (edit fails), or a generous max duration is hit — whichever comes
/// first. Runs detached from the original command's context/lifetime, which
/// is why it takes an owned `Arc<Http>` instead of a poise `Context`.
async fn refresh_now_playing(
    http: Arc<Http>,
    channel_id: ChannelId,
    message_id: MessageId,
    player: Arc<GuildPlayer>,
    interval_secs: u64,
) {
    const MAX_REFRESH_SECS: u64 = 2 * 60 * 60;
    let interval_secs = interval_secs.max(1);
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));
    ticker.tick().await; // first tick fires immediately; the sent message is already fresh

    let mut elapsed = 0u64;
    loop {
        ticker.tick().await;
        elapsed += interval_secs;
        if elapsed > MAX_REFRESH_SECS {
            break;
        }

        let snapshot = player.snapshot();
        if snapshot.current.is_none() {
            break;
        }
        let content = crate::components::now_playing_content(&snapshot);
        let buttons = crate::components::now_playing_buttons(&snapshot);
        let edit = EditMessage::new().content(content).components(buttons);
        if channel_id.edit_message(&http, message_id, edit).await.is_err() {
            break;
        }
    }
}
