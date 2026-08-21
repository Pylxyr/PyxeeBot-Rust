use crate::bot::Context;

#[poise::command(prefix_command, slash_command)]
pub async fn ping(ctx: Context<'_>) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    let reply = ctx.say("Pong!").await?;
    let elapsed = start.elapsed();
    reply
        .edit(
            ctx,
            poise::CreateReply::default().content(format!("Pong! `{elapsed:?}`")),
        )
        .await?;
    Ok(())
}

#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn lyrics(ctx: Context<'_>, #[rest] query: Option<String>) -> anyhow::Result<()> {
    let raw_query = match query {
        Some(q) => q,
        None => {
            let Some(guild_id) = ctx.guild_id() else {
                return Ok(());
            };
            let player = ctx.data().player_for(guild_id).await;
            let Some(current) = player.snapshot().current else {
                ctx.say("Nothing is playing, and no search term given.")
                    .await?;
                return Ok(());
            };
            current.title.clone()
        }
    };
    // Strip bracketed clutter either way — a manually-typed query is
    // unaffected if it's already clean, but this is also what saves the
    // no-args fallback (the raw video title) from tags like "【MV】" or
    // "(Official Video)" that would otherwise go straight into the search.
    let query = crate::lyrics::clean_query(&raw_query);

    let handle = ctx.say(format!("Looking up lyrics for `{query}`...")).await?;
    match ctx.data().lyrics.get_lyrics(&query).await {
        Ok(Some((artist, title, lyrics))) => {
            let body: String = if lyrics.chars().count() > 3800 {
                let mut truncated: String = lyrics.chars().take(3800).collect();
                truncated.push_str("\n…(truncated)");
                truncated
            } else {
                lyrics
            };
            let embed = poise::serenity_prelude::CreateEmbed::new()
                .title(format!("{artist} — {title}"))
                .description(body)
                .color(0xF0_A8_68u32);
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content("").embed(embed),
                )
                .await?;
        }
        Ok(None) => {
            // Only suggest adding an artist when the query doesn't already
            // look like it has one — clean_query() deliberately keeps a
            // bare " - " (the Artist - Title convention), so the no-args
            // now-playing fallback usually already includes it. Suggesting
            // `!lyrics MYTH&ROID - STYX HELIX <artist>` when the artist is
            // already sitting right there would just be confusing.
            let hint = if query.contains(" - ") {
                String::new()
            } else {
                let prefix = ctx.prefix();
                format!(
                    " If you only gave a song title, try adding the artist too — e.g. `{prefix}lyrics {query} <artist>`."
                )
            };
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content(format!("No lyrics found for `{query}`.{hint}")),
                )
                .await?;
        }
        Err(e) => {
            tracing::warn!(error = %e, query = %query, "lyrics lookup failed");
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default()
                        .content("Lyrics lookup failed — try again in a bit."),
                )
                .await?;
        }
    }
    Ok(())
}
