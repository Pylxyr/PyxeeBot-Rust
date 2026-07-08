use crate::bot::Context;
use crate::db::QueueEntryRef;
use crate::models::Track;

/// Manage saved playlists.
#[poise::command(
    prefix_command,
    slash_command,
    guild_only,
    subcommands("save", "load", "list", "show", "delete")
)]
pub async fn playlist(ctx: Context<'_>) -> anyhow::Result<()> {
    ctx.say("Usage: `!playlist save|load|list|show|delete <name>`")
        .await?;
    Ok(())
}

/// Save the current queue as a named playlist.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn save(ctx: Context<'_>, name: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let snapshot = ctx.data().player_for(guild_id).await.snapshot();

    let mut all_tracks: Vec<&Track> = Vec::new();
    if let Some(current) = &snapshot.current {
        all_tracks.push(current);
    }
    all_tracks.extend(snapshot.queue.iter());

    if all_tracks.is_empty() {
        ctx.say("Queue is empty — nothing to save.").await?;
        return Ok(());
    }

    let entries: Vec<QueueEntryRef> = all_tracks
        .iter()
        .map(|t| QueueEntryRef {
            query: &t.query,
            title: &t.title,
            webpage_url: &t.webpage_url,
            requester_id: t.requester_id,
        })
        .collect();

    ctx.data()
        .db
        .save_playlist(guild_id.get(), &name, ctx.author().id.get(), &entries)
        .await?;
    ctx.say(format!(
        "Saved **{}** track(s) as playlist `{name}`.",
        entries.len()
    ))
    .await?;
    Ok(())
}

/// Load a saved playlist into the queue.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn load(ctx: Context<'_>, name: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let author_id = ctx.author().id.get();

    let entries = ctx
        .data()
        .db
        .get_playlist_entries(guild_id.get(), &name)
        .await?;
    if entries.is_empty() {
        ctx.say(format!("No playlist named `{name}` found."))
            .await?;
        return Ok(());
    }

    let Some(channel_id) = ctx.serenity_context().cache.guild(guild_id).and_then(|g| {
        g.voice_states
            .get(&ctx.author().id)
            .and_then(|vs| vs.channel_id)
    }) else {
        ctx.say("Join a voice channel first.").await?;
        return Ok(());
    };

    let player = ctx.data().player_for(guild_id).await;
    let mut queued = 0usize;
    for entry in &entries {
        let track = Track {
            title: entry.title.clone(),
            webpage_url: entry.webpage_url.clone(),
            uploader: String::new(),
            duration: 0,
            requester_id: author_id,
            query: entry.query.clone(),
            thumbnail_url: String::new(),
            tags: Vec::new(),
            acodec: String::new(),
            abr: 0.0,
        };
        if player
            .play(track, false, channel_id)
            .await
            .is_ok_and(|o| !o.failed)
        {
            queued += 1;
        }
    }
    ctx.say(format!("Queued {queued} track(s) from `{name}`."))
        .await?;
    Ok(())
}

/// List saved playlists for this server.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn list(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let playlists = ctx.data().db.list_playlists(guild_id.get()).await?;
    if playlists.is_empty() {
        ctx.say("No saved playlists yet.").await?;
        return Ok(());
    }
    let lines: Vec<String> = playlists
        .iter()
        .map(|p| format!("`{}` — {} track(s)", p.name, p.track_count))
        .collect();
    ctx.say(lines.join("\n")).await?;
    Ok(())
}

/// Show the tracks in a saved playlist.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn show(ctx: Context<'_>, name: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let entries = ctx
        .data()
        .db
        .get_playlist_entries(guild_id.get(), &name)
        .await?;
    if entries.is_empty() {
        ctx.say(format!("No playlist named `{name}` found."))
            .await?;
        return Ok(());
    }
    let lines: Vec<String> = entries
        .iter()
        .take(20)
        .enumerate()
        .map(|(i, e)| format!("`{}.` {}", i + 1, crate::models::escape_markdown(&e.title)))
        .collect();
    ctx.say(lines.join("\n")).await?;
    Ok(())
}

/// Delete a saved playlist.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn delete(ctx: Context<'_>, name: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let deleted = ctx.data().db.delete_playlist(guild_id.get(), &name).await?;
    if deleted {
        ctx.say(format!("Deleted playlist `{name}`.")).await?;
    } else {
        ctx.say(format!("No playlist named `{name}` found."))
            .await?;
    }
    Ok(())
}
