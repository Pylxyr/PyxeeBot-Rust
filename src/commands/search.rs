use std::sync::Arc;

use crate::bot::Context;

/// Search for a track and show the top results (use !play to queue one).
// Note: no "s" alias here — it collided with `skip`'s "s" alias (only one
// of the two was ever reachable via `!s`, depending on registration order).
// Skip kept it since it's the more frequently used of the two.
#[poise::command(prefix_command, slash_command, guild_only)]
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
        .search_with_debug(&query, author_id, false, fetch_count)
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

    ctx.data()
        .search_debug
        .insert(guild_id, Arc::new(results.clone()));
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

/// Explain why a `!search` result ranked where it did (defaults to the top result).
#[poise::command(prefix_command, slash_command, guild_only)]
pub async fn why(ctx: Context<'_>, position: Option<usize>) -> anyhow::Result<()> {
    let Some(guild_id) = ctx.guild_id() else {
        return Ok(());
    };
    let Some(results) = ctx.data().search_debug.get(&guild_id) else {
        ctx.say("No recent search to explain — run `!search` first.")
            .await?;
        return Ok(());
    };

    let idx = position.unwrap_or(1).saturating_sub(1);
    let Some((track, bd)) = results.get(idx) else {
        ctx.say(format!("No result at position {}.", position.unwrap_or(1)))
            .await?;
        return Ok(());
    };

    let factors: &[(&str, f64)] = &[
        ("fuzzy ratio", bd.ratio),
        ("metadata ratio", bd.metadata_ratio),
        ("title overlap", bd.title_overlap),
        ("uploader overlap", bd.uploader_overlap),
        ("anchor match", bd.anchor_score),
        ("artist match bonus", bd.artist_match_bonus),
        ("strong uploader bonus", bd.strong_uploader_bonus),
        ("topic bonus", bd.topic_bonus),
        ("uploader preference bonus", bd.uploader_pref_bonus),
        ("artist completion bonus", bd.artist_completion_bonus),
        ("title/uploader synergy", bd.title_uploader_synergy),
        ("preferred phrase bonus", bd.preferred_bonus),
        ("duration bonus", bd.duration_bonus),
        ("JP original bonus", bd.jp_original_bonus),
        ("view count bonus", bd.view_bonus),
        ("verified channel bonus", bd.verified_bonus),
        ("recency bonus", bd.recency_bonus),
        ("discouraged penalty", -bd.discouraged_penalty),
    ];

    let lines: Vec<String> = factors
        .iter()
        .copied()
        .filter(|(_, v)| *v != 0.0)
        .map(|(label, v)| format!("`{v:+.3}`  {label}"))
        .collect();

    let breakdown = if lines.is_empty() {
        "No scoring factors contributed.".to_owned()
    } else {
        lines.join("\n")
    };

    ctx.say(format!(
        "**{}** — final score `{:.3}`\n{breakdown}",
        track.escaped_title(),
        bd.final_score
    ))
    .await?;
    Ok(())
}
