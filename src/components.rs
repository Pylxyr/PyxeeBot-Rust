use poise::serenity_prelude::{
    ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, CreateActionRow, CreateButton,
    CreateInteractionResponse, CreateInteractionResponseMessage, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption,
};

use crate::models::Track;
use crate::player::PlayerSnapshot;
use crate::scoring::ScoreBreakdown;

pub const NP_PAUSE: &str = "np:pause";
pub const NP_SKIP: &str = "np:skip";
pub const NP_LOOP: &str = "np:loop";
pub const SEARCH_PICK: &str = "search:pick";

pub fn now_playing_content(snapshot: &PlayerSnapshot) -> String {
    match &snapshot.current {
        Some(track) => {
            let state = if snapshot.is_paused {
                "Paused"
            } else {
                "Now playing"
            };
            format!(
                "{state}: **{}** ({}) — requested by <@{}>\nLoop: {}",
                track.escaped_title(),
                track.duration_label(),
                track.requester_id,
                snapshot.loop_mode.label(),
            )
        }
        None => "Nothing is playing right now.".to_owned(),
    }
}

pub fn now_playing_buttons(snapshot: &PlayerSnapshot) -> Vec<CreateActionRow> {
    let pause_label = if snapshot.is_paused {
        "Resume"
    } else {
        "Pause"
    };
    let disabled = snapshot.current.is_none();
    vec![CreateActionRow::Buttons(vec![
        CreateButton::new(NP_PAUSE)
            .label(pause_label)
            .style(ButtonStyle::Secondary)
            .disabled(disabled),
        CreateButton::new(NP_SKIP)
            .label("Skip")
            .style(ButtonStyle::Secondary)
            .disabled(disabled),
        CreateButton::new(NP_LOOP)
            .label("Loop")
            .style(ButtonStyle::Secondary),
    ])]
}

pub fn search_select_menu(results: &[(Track, ScoreBreakdown)]) -> Vec<CreateActionRow> {
    let options: Vec<CreateSelectMenuOption> = results
        .iter()
        .take(5)
        .enumerate()
        .map(|(i, (t, _))| CreateSelectMenuOption::new(t.escaped_title(), i.to_string()))
        .collect();
    vec![CreateActionRow::SelectMenu(CreateSelectMenu::new(
        SEARCH_PICK,
        CreateSelectMenuKind::String { options },
    ))]
}

/// Builds the response that updates the interacted-with message in place —
/// the standard "acknowledge + edit" pattern for component interactions.
pub fn update_response(
    content: impl Into<String>,
    components: Vec<CreateActionRow>,
) -> CreateInteractionResponse {
    CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .content(content.into())
            .components(components),
    )
}

pub fn selected_index(interaction: &ComponentInteraction) -> Option<usize> {
    match &interaction.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values.first()?.parse().ok(),
        _ => None,
    }
}
