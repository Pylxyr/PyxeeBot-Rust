use crate::bot::Context;

/// Search for a track and show the top results (use !play to queue one).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn search(ctx: Context<'_>, #[rest] query: String) -> anyhow::Result<()> {
    let handle = ctx.say(format!("Searching for `{query}`...")).await?;
    let author_id = ctx.author().id.get();

    let tracks = match ctx.data().extractor.search(&query, author_id, false).await {
        Ok(t) => t,
        Err(e) => {
            handle
                .edit(
                    ctx,
                    poise::CreateReply::default().content(format!("Search failed: {e}")),
                )
                .await?;
            return Ok(());
        }
    };

    if tracks.is_empty() {
        handle
            .edit(
                ctx,
                poise::CreateReply::default().content("No results found."),
            )
            .await?;
        return Ok(());
    }

    let lines: Vec<String> = tracks
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, t)| {
            format!(
                "`{}.` {} ({}) — {}",
                i + 1,
                t.escaped_title(),
                t.duration_label(),
                t.escaped_uploader()
            )
        })
        .collect();

    handle
        .edit(
            ctx,
            poise::CreateReply::default().content(format!(
                "Top results for `{query}`:\n{}\n\nUse `!play <title>` to queue one.",
                lines.join("\n")
            )),
        )
        .await?;
    Ok(())
}
