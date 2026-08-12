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

/// True only for `list=` without `v=` — not a video URL that incidentally carries `list=`.
fn is_playlist_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    lower.contains("list=") && !lower.contains("v=")
}

async fn play_playlist(
    ctx: Context<'_>,
    guild_id: poise::serenity_prelude::GuildId,
    author_id: poise::serenity_prelude::UserId,
    channel_id: ChannelId,
    url: &str,
    front: bool,
) -> anyhow::Result<()> {
    let data = ctx.data();
    tracing::info!(guild_id = %guild_id, user = %author_id, url = %url, "!play: playlist URL detected");
    let handle = ctx.say("Reading playlist...").await?;

    let limit = data.config.max_playlist_size;
    let tracks = match data.extractor.extract_playlist(url, author_id.get(), limit).await {
        Ok(t) => t,
        Err(e) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("Couldn't read playlist: {e}")),
                )
                .await?;
            return Ok(());
        }
    };
    if tracks.is_empty() {
        handle
            .edit(
                ctx,
                poise::CreateReply::default().content("Playlist is empty or unavailable."),
            )
            .await?;
        return Ok(());
    }
    let total = tracks.len();
    let _ = handle
        .edit(
            ctx,
            poise::CreateReply::default()
                .content(format!("Found {total} tracks, adding to queue...")),
        )
        .await;

    // Reversed for !playnext — pushing to the front one at a time would reverse the order.
    let ordered: Vec<_> = if front {
        tracks.into_iter().rev().collect()
    } else {
        tracks
    };

    let player = data.player_for(guild_id).await;
    let mut queued = 0usize;
    let mut stopped_early = false;
    for track in ordered {
        match player.play(track, front, channel_id).await {
            Ok(outcome) => {
                if !outcome.failed {
                    queued += 1;
                }
            }
            Err(_) => {
                stopped_early = true;
                break;
            }
        }
    }

    let mut msg = format!("Added **{queued}** of {total} tracks from the playlist.");
    if stopped_early {
        msg.push_str(" Stopped early — the queue is full.");
    }
    handle
        .edit(ctx, poise::CreateReply::default().content(msg))
        .await?;
    Ok(())
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

/// Aborts any live !nowplaying refresher and clears vote-skip tallies — shared by !stop and !leave.
fn clear_guild_side_state(data: &crate::bot::BotData, guild_id: poise::serenity_prelude::GuildId) {
    if let Some((_, handle)) = data.np_refreshers.remove(&guild_id) {
        handle.abort();
    }
    data.skip_votes.remove(&guild_id);
}

/// Leave the voice channel and clear the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn leave(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
    let player = ctx.data().player_for(guild_id).await;
    let result = player.leave().await;
    clear_guild_side_state(ctx.data(), guild_id);
    match result {
        Ok(true) => ctx.say("Left the voice channel.").await?,
        Ok(false) => ctx.say("Not currently in a voice channel.").await?,
        Err(e) => ctx.say(format!("Couldn't leave: {e}")).await?,
    };
    Ok(())
}

/// Search and play (or queue) a track.
#[poise::command(prefix_command, slash_command, guild_only, aliases("p"))]
pub async fn play(ctx: Context<'_>, #[rest] query: String) -> anyhow::Result<()> {
    play_or_queue(ctx, query, false).await
}

/// Queue a track at the front of the queue.
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

    let trimmed = query.trim().to_owned();
    let looks_like_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    if looks_like_url && is_playlist_url(&trimmed) {
        return play_playlist(ctx, guild_id, author_id, channel_id, &trimmed, front).await;
    }

    tracing::info!(guild_id = %guild_id, user = %author_id, query = %query, front, "!play: received");
    let data = ctx.data();

    let search_start = std::time::Instant::now();
    let trimmed = query.trim();
    let is_url = trimmed.starts_with("http://") || trimmed.starts_with("https://");
    let say_fut = ctx.say(format!("Searching for `{query}`..."));
    let resolve_fut = async {
        if is_url {
            data.extractor
                .extract_url(trimmed, author_id.get(), false)
                .await
                .map(|v| v.into_iter().next())
        } else {
            data.extractor.search_top(&query, author_id.get()).await
        }
    };
    // Overlap the reply with the search instead of doing them in series.
    let (handle, resolve_result) = tokio::join!(say_fut, resolve_fut);
    let handle = handle?;
    let track = match resolve_result {
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
    tracing::info!(guild_id = %guild_id, query = %query, elapsed = ?search_start.elapsed(), found = track.is_some(), "!play: search finished");

    let Some(track) = track else {
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
    let needs_connect = {
        let snapshot = player.snapshot();
        !snapshot.is_connected || snapshot.channel_id != Some(channel_id)
    };
    let status = if needs_connect {
        format!("Found **{title}** — connecting + resolving...")
    } else {
        format!("Found **{title}**, loading...")
    };
    let found_edit_fut = handle.edit(ctx, poise::CreateReply::default().content(status));
    let (_, result) = tokio::join!(found_edit_fut, player.play(track, front, channel_id));
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

/// Skip the current track. DJs only — use `!voteskip` otherwise.
#[poise::command(prefix_command, slash_command, guild_only, aliases("s"))]
pub async fn skip(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !super::helpers::require_dj(ctx).await? {
        return Ok(());
    }
    ctx.data().player_for(guild_id).await.skip();
    ctx.say("Skipped.").await?;
    Ok(())
}

/// Vote to skip the current track. DJs skip immediately without a vote.
#[poise::command(prefix_command, slash_command, guild_only, aliases("vsk"))]
pub async fn voteskip(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let player = ctx.data().player_for(guild_id).await;
    let snapshot = player.snapshot();
    let Some(current) = snapshot.current.clone() else {
        ctx.say("Nothing is playing.").await?;
        return Ok(());
    };

    if super::helpers::is_dj(ctx).await {
        player.skip();
        ctx.say("Skipped.").await?;
        return Ok(());
    }

    if !crate::permissions::in_same_voice_channel(
        &ctx.serenity_context().cache,
        guild_id,
        snapshot.channel_id,
        ctx.author().id,
    ) {
        ctx.say("You need to be in the same voice channel as the bot to vote.")
            .await?;
        return Ok(());
    }
    let Some(bot_channel) = snapshot.channel_id else {
        ctx.say("Not connected to a voice channel.").await?;
        return Ok(());
    };
    let bot_id = ctx.serenity_context().cache.current_user().id;
    let listeners: Option<usize> = ctx.serenity_context().cache.guild(guild_id).map(|guild| {
        guild
            .voice_states
            .values()
            .filter(|vs| vs.channel_id == Some(bot_channel) && vs.user_id != bot_id)
            .count()
    });
    let Some(listeners) = listeners else {
        ctx.say("Couldn't read the voice channel.").await?;
        return Ok(());
    };
    let needed = listeners.div_ceil(2).max(1);

    let key = current.webpage_url.clone();
    let (have, passed) = {
        let mut entry = ctx.data().skip_votes.entry(guild_id).or_default();
        if entry.0 != key {
            *entry = (key, std::collections::HashSet::new());
        }
        entry.1.insert(ctx.author().id.get());
        (entry.1.len(), entry.1.len() >= needed)
    };

    if passed {
        ctx.data().skip_votes.remove(&guild_id);
        player.skip();
        ctx.say(format!("Vote to skip passed ({have}/{needed}) — skipped."))
            .await?;
    } else {
        ctx.say(format!("Vote to skip: **{have}/{needed}**.")).await?;
    }
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
    clear_guild_side_state(ctx.data(), guild_id);
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
    ctx.data().player_for(guild_id).await.pause().await;
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
    ctx.data().player_for(guild_id).await.resume().await;
    ctx.say("Resumed.").await?;
    Ok(())
}

/// Show or set the playback volume (0-200).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn volume(ctx: Context<'_>, level: Option<u8>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let player = ctx.data().player_for(guild_id).await;
    let Some(level) = level else {
        let current = player.snapshot().volume;
        ctx.say(format!("Current volume: **{current}%**")).await?;
        return Ok(());
    };
    if !super::helpers::require_same_voice_channel(ctx).await? {
        return Ok(());
    }
    let level = level.min(200);
    player.set_volume(level).await;
    ctx.say(format!("Volume set to **{level}%**.")).await?;
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
    let embed = crate::components::now_playing_embed(&snapshot);
    let buttons = crate::components::now_playing_buttons(&snapshot);
    let reply = ctx
        .send(poise::CreateReply::default().embed(embed).components(buttons))
        .await?;

    if ctx.data().config.np_auto_refresh {
        if let Ok(message) = reply.message().await {
            let http = ctx.serenity_context().http.clone();
            let channel_id = ctx.channel_id();
            let message_id = message.id;
            let interval_secs = u64::from(ctx.data().config.np_auto_refresh_interval);
            let task = tokio::spawn(refresh_now_playing(
                http,
                channel_id,
                message_id,
                player,
                interval_secs,
            ));
            // Replace, don't stack: an earlier refresher for this guild would otherwise leak.
            if let Some(old) = ctx.data().np_refreshers.insert(guild_id, task.abort_handle()) {
                old.abort();
            }
        }
    }
    Ok(())
}

/// Detached loop for NP_AUTO_REFRESH — stops when nothing's playing, an edit fails, or the cap hits.
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
        let embed = crate::components::now_playing_embed(&snapshot);
        let buttons = crate::components::now_playing_buttons(&snapshot);
        let edit = EditMessage::new()
            .content("")
            .embed(embed)
            .components(buttons);
        if channel_id.edit_message(&http, message_id, edit).await.is_err() {
            break;
        }
    }
}
