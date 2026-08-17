use std::sync::Arc;

use crate::bot::Context;

#[poise::command(prefix_command, slash_command, guild_only, aliases("s"))]
pub async fn search(ctx: Context<'_>, #[rest] query: String) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let handle = ctx.say(format!("Searching for `{query}`...")).await?;
    let author_id = ctx.author().id.get();

    let fetch_count = crate::components::SEARCH_PAGE_SIZE * crate::components::SEARCH_MAX_PAGES;
    let results = match ctx
        .data()
        .extractor
        .search(&query, author_id, fetch_count)
        .await
    {
        Ok(r) => r,
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

    if results.is_empty() {
        handle
            .edit(
                ctx,
                poise::CreateReply::default().content("No results found."),
            )
            .await?;
        return Ok(());
    }

    let results = Arc::new(results);
    ctx.data().recent_searches.insert(guild_id, results.clone());
    let content = crate::components::search_results_content(Some(&query), &results, 0);
    let menu = crate::components::search_select_menu(&results, 0);

    handle
        .edit(
            ctx,
            poise::CreateReply::default()
                .content(content)
                .components(menu),
        )
        .await?;
    Ok(())
}
