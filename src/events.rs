use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{self as serenity, FullEvent};
use poise::FrameworkContext;

use crate::bot::BotData;

pub async fn handle_event(
    ctx: &serenity::Context,
    event: &FullEvent,
    _framework: FrameworkContext<'_, Arc<BotData>, anyhow::Error>,
    data: &Arc<BotData>,
) -> Result<(), anyhow::Error> {
    if let FullEvent::VoiceStateUpdate { old, new } = event {
        handle_voice_state_update(ctx, data, old.as_ref(), new).await;
    }
    Ok(())
}

async fn handle_voice_state_update(
    ctx: &serenity::Context,
    data: &Arc<BotData>,
    old: Option<&serenity::VoiceState>,
    new: &serenity::VoiceState,
) {
    let Some(guild_id) = new.guild_id else { return };
    let bot_id = ctx.cache.current_user().id;

    if new.user_id == bot_id {
        if new.channel_id.is_none() {
            // Force-kicked (or a clean !leave, which is harmless to re-check
            // here since stay_connected will be false in that case). This is
            // the exact regression fix from the Python bug hunt: only try to
            // rejoin if stay_connected is actually on.
            let Some(old_channel) = old.and_then(|o| o.channel_id) else {
                return;
            };
            let Some(player) = data.players.get(&guild_id).map(|p| p.clone()) else {
                return;
            };
            if !player.snapshot().stay_connected {
                return;
            }
            let data = data.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(5)).await;
                if let Some(player) = data.players.get(&guild_id).map(|p| p.clone()) {
                    if player.snapshot().stay_connected {
                        player.rejoin(old_channel);
                    }
                }
            });
        }
        return;
    }

    // A different member's voice state changed. Only relevant if it affects
    // the channel our bot is currently sitting in.
    let Some(player) = data.players.get(&guild_id).map(|p| p.clone()) else {
        return;
    };
    let Some(our_channel) = ctx
        .cache
        .guild(guild_id)
        .and_then(|g| g.voice_states.get(&bot_id).and_then(|vs| vs.channel_id))
    else {
        return;
    };

    let affects_our_channel =
        new.channel_id == Some(our_channel) || old.and_then(|o| o.channel_id) == Some(our_channel);
    if !affects_our_channel {
        return;
    }

    // Simplification: this excludes only our own bot from the human count,
    // not other bot accounts that might be sitting in the same channel.
    let has_humans = ctx.cache.guild(guild_id).is_some_and(|g| {
        g.voice_states
            .values()
            .any(|vs| vs.channel_id == Some(our_channel) && vs.user_id != bot_id)
    });

    if has_humans {
        player.cancel_empty_disconnect();
    } else {
        player.schedule_empty_disconnect();
    }
}
