use crate::bot::Context;

/// Queue a run of tracks by artists similar to the one you name (needs
/// LASTFM_API_KEY configured).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn vibe(ctx: Context<'_>, #[rest] artist: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let author_id = ctx.author().id.get();

    let data = ctx.data();
    let Some(lastfm) = &data.lastfm else {
        ctx.say("Vibe mode needs a Last.fm API key configured on this bot.")
            .await?;
        return Ok(());
    };

    let Some(channel_id) = ctx.serenity_context().cache.guild(guild_id).and_then(|g| {
        g.voice_states
            .get(&ctx.author().id)
            .and_then(|vs| vs.channel_id)
    }) else {
        ctx.say("Join a voice channel first.").await?;
        return Ok(());
    };

    let handle = ctx
        .say(format!("Building a vibe around `{artist}`..."))
        .await?;

    let similar = match lastfm.similar_artists(&artist, 6).await {
        Ok(a) if !a.is_empty() => a,
        Ok(_) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content(format!("No similar artists found for `{artist}`.")),
                )
                .await?;
            return Ok(());
        }
        Err(e) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("Last.fm lookup failed: {e}")),
                )
                .await?;
            return Ok(());
        }
    };

    let player = data.player_for(guild_id);
    let mut queued = Vec::new();
    for similar_artist in &similar {
        match data.extractor.search(similar_artist, author_id, true).await {
            Ok(tracks) if !tracks.is_empty() => {
                let track = tracks.into_iter().next().unwrap();
                let title = track.escaped_title();
                if player.play(track, false, channel_id).await.is_ok() {
                    queued.push(title);
                }
            }
            _ => continue,
        }
    }

    if queued.is_empty() {
        handle
            .edit(
                ctx,
                poise::CreateReply::default()
                    .content("Couldn't find playable tracks for this vibe."),
            )
            .await?;
    } else {
        let list = queued
            .iter()
            .map(|t| format!("• {t}"))
            .collect::<Vec<_>>()
            .join("\n");
        handle
            .edit(
                ctx,
                poise::CreateReply::default()
                    .content(format!("Queued {} track(s):\n{list}", queued.len())),
            )
            .await?;
    }
    Ok(())
}

/// Toggle autoplay (queues a similar track instead of going idle).
///
/// Note: this persists the setting, but the player does not yet act on it —
/// automatically queuing a similar track when the queue empties needs wiring
/// into the player actor, which isn't connected in this delivery.
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn autoplay(ctx: Context<'_>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let data = ctx.data();
    let current = data.db.get_autoplay(guild_id.get()).await;
    let new_value = !current;
    data.db
        .set_autoplay(guild_id.get(), new_value, &data.config.default_prefix)
        .await?;
    let state = if new_value { "enabled" } else { "disabled" };
    ctx.say(format!("Autoplay {state}.")).await?;
    Ok(())
}
