use crate::bot::Context;

/// Sliding window size for per-guild !vibe song history — see vibe_history on BotData.
const VIBE_HISTORY_CAP: usize = 50;
/// How many similar tracks/artists to try; actual concurrency is capped by YTDLP_CURATION_CONCURRENCY.
const VIBE_CANDIDATE_COUNT: usize = 4;

/// Normalizes a query into a dedup key, so casing/spacing variants count as the same song.
fn vibe_history_key(query: &str) -> String {
    query.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Queue similar tracks by artist/song. Requires LASTFM_API_KEY.
#[poise::command(prefix_command, slash_command, guild_only, aliases("vb"))]
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

    // "Artist + song" first (typed like !play), falling back to artist-level similarity.
    let mut queries: Vec<String> = Vec::new();
    if let Ok(Some((resolved_artist, resolved_track))) = lastfm.resolve_track(&artist).await {
        if let Ok(similar) = lastfm
            .similar_tracks(&resolved_artist, &resolved_track, VIBE_CANDIDATE_COUNT)
            .await
        {
            queries = similar
                .into_iter()
                .map(|(a, t)| format!("{a} {t}"))
                .collect();
        }
    }

    if queries.is_empty() {
        queries = match lastfm.similar_artists(&artist, VIBE_CANDIDATE_COUNT).await {
            Ok(a) if !a.is_empty() => a,
            Ok(_) => {
                let _ = handle
                    .edit(
                        ctx,
                        poise::CreateReply::default().content(format!(
                            "No similar artists or tracks found for `{artist}`."
                        )),
                    )
                    .await;
                return Ok(());
            }
            Err(e) => {
                let _ = handle
                    .edit(
                        ctx,
                        poise::CreateReply::default()
                            .content(format!("Last.fm lookup failed: {e}")),
                    )
                    .await;
                return Ok(());
            }
        };
    }

    // Prefer songs outside recent !vibe history; reuse one only if every candidate repeats.
    {
        let seen = data.vibe_history.get(&guild_id);
        let fresh: Vec<String> = queries
            .iter()
            .filter(|q| {
                seen.as_ref()
                    .is_none_or(|h| !h.contains(&vibe_history_key(q)))
            })
            .cloned()
            .collect();
        if !fresh.is_empty() {
            queries = fresh;
        }
    }

    let player = data.player_for(guild_id).await;

    let mut queued = Vec::new();
    // Searches run concurrently; Extractor's own search_semaphore still caps actual yt-dlp processes.
    let mut search_tasks = Vec::with_capacity(queries.len());
    for query in &queries {
        let extractor = data.extractor.clone();
        let query = query.clone();
        search_tasks.push(tokio::spawn(async move {
            extractor.search(&query, author_id, true).await.ok()
        }));
    }
    for (query, task) in queries.iter().zip(search_tasks) {
        let Ok(Some(tracks)) = task.await else {
            continue;
        };
        let Some(track) = tracks.into_iter().next() else {
            continue;
        };
        let title = track.escaped_title();
        if player.play(track, false, channel_id).await.is_ok_and(|o| !o.failed) {
            queued.push(title);
            let mut history = data.vibe_history.entry(guild_id).or_default();
            let key = vibe_history_key(query);
            history.retain(|k| k != &key);
            history.push_back(key);
            while history.len() > VIBE_HISTORY_CAP {
                history.pop_front();
            }
        }
    }

    if queued.is_empty() {
        let _ = handle
            .edit(
                ctx,
                poise::CreateReply::default()
                    .content("Couldn't find playable tracks for this vibe."),
            )
            .await;
    } else {
        let list = queued
            .iter()
            .map(|t| format!("• {t}"))
            .collect::<Vec<_>>()
            .join("\n");
        let _ = handle
            .edit(
                ctx,
                poise::CreateReply::default()
                    .content(format!("Queued {} track(s):\n{list}", queued.len())),
            )
            .await;
    }
    Ok(())
}

/// Toggle autoplay (queues a similar track instead of going idle).
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
    data.player_for(guild_id).await.set_autoplay(new_value);
    let state = if new_value { "enabled" } else { "disabled" };
    ctx.say(format!("Autoplay {state}.")).await?;
    Ok(())
}
