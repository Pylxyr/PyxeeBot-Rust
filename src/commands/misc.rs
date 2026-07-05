use crate::bot::Context;

/// Check whether the bot is responsive.
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
