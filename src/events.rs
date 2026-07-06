use std::sync::Arc;
use std::time::Duration;

use poise::serenity_prelude::{
    self as serenity, ComponentInteraction, CreateInteractionResponse, EditInteractionResponse,
    FullEvent, Interaction,
};
use poise::FrameworkContext;

use crate::bot::BotData;
use crate::components;

pub async fn handle_event(
    ctx: &serenity::Context,
    event: &FullEvent,
    _framework: FrameworkContext<'_, Arc<BotData>, anyhow::Error>,
    data: &Arc<BotData>,
) -> Result<(), anyhow::Error> {
    match event {
        FullEvent::VoiceStateUpdate { old, new } => {
            handle_voice_state_update(ctx, data, old.as_ref(), new).await;
        }
        FullEvent::InteractionCreate { interaction } => {
            if let Interaction::Component(component) = interaction {
                handle_component_interaction(ctx, data, component).await;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn handle_component_interaction(
    ctx: &serenity::Context,
    data: &Arc<BotData>,
    interaction: &ComponentInteraction,
) {
    let Some(guild_id) = interaction.guild_id else {
        return;
    };
    let custom_id = interaction.data.custom_id.as_str();

    match custom_id {
        components::NP_PAUSE | components::NP_SKIP | components::NP_LOOP => {
            let Some(player) = data.players.get(&guild_id).map(|p| p.clone()) else {
                return;
            };
            match custom_id {
                components::NP_PAUSE => {
                    if player.snapshot().is_paused {
                        player.resume();
                    } else {
                        player.pause();
                    }
                }
                components::NP_SKIP => player.skip(),
                components::NP_LOOP => {
                    player.cycle_loop_mode().await;
                }
                _ => unreachable!(),
            }
            let snapshot = player.snapshot();
            let content = components::now_playing_content(&snapshot);
            let buttons = components::now_playing_buttons(&snapshot);
            let _ = interaction
                .create_response(ctx, components::update_response(content, buttons))
                .await;
        }
        components::SEARCH_PICK => handle_search_pick(ctx, data, interaction, guild_id).await,
        _ => {}
    }
}

async fn handle_search_pick(
    ctx: &serenity::Context,
    data: &Arc<BotData>,
    interaction: &ComponentInteraction,
    guild_id: serenity::GuildId,
) {
    let Some(idx) = components::selected_index(interaction) else {
        return;
    };
    let Some(results) = data.search_debug.get(&guild_id) else {
        let _ = interaction
            .create_response(
                ctx,
                components::update_response("That search has expired.", Vec::new()),
            )
            .await;
        return;
    };
    let Some((track, _)) = results.get(idx) else {
        return;
    };

    let user_id = interaction.user.id;
    let channel_id = ctx
        .cache
        .guild(guild_id)
        .and_then(|g| g.voice_states.get(&user_id).and_then(|vs| vs.channel_id));
    let Some(channel_id) = channel_id else {
        let _ = interaction
            .create_response(
                ctx,
                components::update_response("Join a voice channel first.", Vec::new()),
            )
            .await;
        return;
    };

    let mut track = track.clone();
    track.requester_id = user_id.get();
    let title = track.escaped_title();

    // Component interactions must be acknowledged within 3 seconds. The
    // play() call below does search resolution and a voice connect, both of
    // which routinely take longer than that — so acknowledge first (no
    // loading indicator shown to the user) and edit the response once done.
    let _ = interaction
        .create_response(ctx, CreateInteractionResponse::Acknowledge)
        .await;

    let player = data.player_for(guild_id).await;
    let content = match player.play(track, false, channel_id).await {
        Ok(outcome) if outcome.now_playing => format!("Now playing: **{title}**"),
        Ok(outcome) => format!("Queued **{title}** — position {}.", outcome.position),
        Err(e) => format!("Error: {e}"),
    };
    let _ = interaction
        .edit_response(
            ctx,
            EditInteractionResponse::new()
                .content(content)
                .components(Vec::new()),
        )
        .await;
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
