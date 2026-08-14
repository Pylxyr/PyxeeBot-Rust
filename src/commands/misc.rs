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
pub async fn lyrics(ctx: Context<'_>, query: Option<String>) -> anyhow::Result<()> {
    let query = match query {
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
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("No lyrics found for `{query}`.")),
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
