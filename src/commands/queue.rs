use super::helpers::require_dj;
use crate::bot::Context;

/// Show the current queue.
#[poise::command(prefix_command, slash_command, guild_only, aliases("q"))]
pub async fn queue(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let snapshot = ctx.data().player_for(guild_id).snapshot();

    if snapshot.current.is_none() && snapshot.queue.is_empty() {
        ctx.say("Queue is empty.").await?;
        return Ok(());
    }

    let mut lines = Vec::new();
    if let Some(current) = &snapshot.current {
        lines.push(format!(
            "**Now playing:** {} ({})",
            current.escaped_title(),
            current.duration_label()
        ));
    }
    for (i, track) in snapshot.queue.iter().take(15).enumerate() {
        lines.push(format!(
            "`{}.` {} ({})",
            i + 1,
            track.escaped_title(),
            track.duration_label()
        ));
    }
    if snapshot.queue.len() > 15 {
        lines.push(format!("...and {} more.", snapshot.queue.len() - 15));
    }
    ctx.say(lines.join("\n")).await?;
    Ok(())
}

/// Clear the queue (keeps the current track playing).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn clear(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    let n = ctx.data().player_for(guild_id).clear_queue().await;
    ctx.say(format!("Cleared {n} track(s) from the queue."))
        .await?;
    Ok(())
}

/// Shuffle the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn shuffle(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    ctx.data().player_for(guild_id).shuffle();
    ctx.say("Queue shuffled.").await?;
    Ok(())
}

/// Move a track from one position to another (1-indexed).
#[poise::command(prefix_command, slash_command, guild_only, rename = "move")]
pub async fn move_track_cmd(ctx: Context<'_>, from: usize, to: usize) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if !require_dj(ctx).await? {
        return Ok(());
    }
    if from == 0 || to == 0 {
        ctx.say("Positions are 1-indexed — the first track in queue is position 1.")
            .await?;
        return Ok(());
    }
    let ok = ctx
        .data()
        .player_for(guild_id)
        .move_track(from - 1, to - 1)
        .await;
    if ok {
        ctx.say(format!("Moved track {from} to position {to}."))
            .await?;
    } else {
        ctx.say("Invalid position.").await?;
    }
    Ok(())
}

/// Remove a track from the queue by position (1-indexed).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn remove(ctx: Context<'_>, position: usize) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    if position == 0 {
        ctx.say("Positions are 1-indexed — the first track in queue is position 1.")
            .await?;
        return Ok(());
    }
    let player = ctx.data().player_for(guild_id);
    let snapshot = player.snapshot();
    let is_requester = snapshot
        .queue
        .get(position - 1)
        .is_some_and(|t| t.requester_id == ctx.author().id.get());
    if !is_requester && !super::helpers::is_dj(ctx).await {
        ctx.say("Only the requester or a DJ can remove this track.")
            .await?;
        return Ok(());
    }
    match player.remove_track(position - 1).await {
        Some(track) => {
            ctx.say(format!("Removed **{}**.", track.escaped_title()))
                .await?
        }
        None => ctx.say("Invalid position.").await?,
    };
    Ok(())
}

/// Show recently played tracks (this session only).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn history(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let snapshot = ctx.data().player_for(guild_id).snapshot();
    if snapshot.history.is_empty() {
        ctx.say("No history yet this session.").await?;
        return Ok(());
    }
    let lines: Vec<String> = snapshot
        .history
        .iter()
        .rev()
        .take(15)
        .enumerate()
        .map(|(i, t)| format!("`{}.` {}", i + 1, t.escaped_title()))
        .collect();
    ctx.say(lines.join("\n")).await?;
    Ok(())
}

/// Show the most-played tracks for this server, all-time.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn toptracks(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let rows = ctx.data().db.get_top_played(guild_id.get(), 10).await?;
    if rows.is_empty() {
        ctx.say("No play history yet.").await?;
        return Ok(());
    }
    let lines: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "`{}.` {} — {} play(s)",
                i + 1,
                crate::models::escape_markdown(&r.title),
                r.play_count
            )
        })
        .collect();
    ctx.say(lines.join("\n")).await?;
    Ok(())
}

/// Show the top track requestors for this server, all-time.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn toprequestors(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let rows = ctx.data().db.get_top_requesters(guild_id.get(), 10).await?;
    if rows.is_empty() {
        ctx.say("No play history yet.").await?;
        return Ok(());
    }
    let lines: Vec<String> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "`{}.` <@{}> — {} request(s)",
                i + 1,
                r.requester_id,
                r.request_count
            )
        })
        .collect();
    ctx.say(lines.join("\n")).await?;
    Ok(())
}
