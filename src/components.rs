use poise::serenity_prelude::{
    ButtonStyle, ComponentInteraction, ComponentInteractionDataKind, CreateActionRow, CreateButton,
    CreateEmbed, CreateEmbedAuthor, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption,
};

use crate::models::Track;
use crate::player::PlayerSnapshot;
use crate::scoring::ScoreBreakdown;

pub const NP_PAUSE: &str = "np:pause";
pub const NP_SKIP: &str = "np:skip";
pub const NP_LOOP: &str = "np:loop";
pub const NP_CLOSE: &str = "np:close";
pub const SEARCH_PICK: &str = "search:pick";
pub const SEARCH_PAGE_PREFIX: &str = "search:page:";
pub const SEARCH_PAGE_SIZE: usize = 5;
pub const SEARCH_MAX_PAGES: usize = 3;

/// PyxeeBot's brand orange, matching the website.
const BRAND_COLOR: u32 = 0xF0_A8_68;

pub fn now_playing_embed(snapshot: &PlayerSnapshot) -> CreateEmbed {
    let Some(track) = &snapshot.current else {
        return CreateEmbed::new()
            .description("Nothing is playing right now.")
            .color(BRAND_COLOR);
    };
    let state = if snapshot.is_paused {
        "Paused"
    } else {
        "Now Playing"
    };
    let mut embed = CreateEmbed::new()
        .author(CreateEmbedAuthor::new(state))
        .title(track.escaped_title())
        .url(&track.webpage_url)
        .color(BRAND_COLOR)
        .description(format!(
            "{} · requested by <@{}> · Loop: {}",
            track.duration_label(),
            track.requester_id,
            snapshot.loop_mode.label(),
        ));
    if !track.thumbnail_url.is_empty() {
        embed = embed.thumbnail(track.thumbnail_url.clone());
    }
    if !snapshot.queue.is_empty() {
        let up_next = snapshot
            .queue
            .iter()
            .take(2)
            .enumerate()
            .map(|(i, t)| format!("`{}.` {} ({})", i + 1, t.escaped_title(), t.duration_label()))
            .collect::<Vec<_>>()
            .join("\n");
        embed = embed.field("Up next", up_next, false);
    }
    embed
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
        CreateButton::new(NP_CLOSE)
            .label("Close")
            .style(ButtonStyle::Danger),
    ])]
}

/// Builds the message text for one page of search results. Position
/// numbers are absolute across pages (matching `!why <n>`); `query` is
/// only present on first render, not on page-navigation clicks.
pub fn search_results_content(
    query: Option<&str>,
    results: &[(Track, ScoreBreakdown)],
    page: usize,
) -> String {
    let start = page * SEARCH_PAGE_SIZE;
    let lines: Vec<String> = results
        .iter()
        .enumerate()
        .skip(start)
        .take(SEARCH_PAGE_SIZE)
        .map(|(i, (t, _))| {
            format!(
                "`{}.` {} ({}) — {}",
                i + 1,
                t.escaped_title(),
                t.duration_label(),
                t.escaped_uploader()
            )
        })
        .collect();
    let total_pages = search_page_count(results.len());
    let header = match query {
        Some(q) => format!("Top results for `{q}`:"),
        None => format!("Search results — page {} of {total_pages}:", page + 1),
    };
    format!(
        "{header}\n{}\n\nPick one below, use `!play <title>`, or `!why <n>` to see why a result ranked where it did.",
        lines.join("\n")
    )
}

/// Number of pages needed for `total` results, capped at SEARCH_MAX_PAGES.
pub fn search_page_count(total: usize) -> usize {
    total.div_ceil(SEARCH_PAGE_SIZE).clamp(1, SEARCH_MAX_PAGES)
}

/// Discord caps select-menu option labels at 25 characters — well short of
/// most track titles, so this truncates rather than letting the option get
/// rejected outright.
fn truncate_label(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_owned();
    }
    let mut t: String = s.chars().take(max_chars.saturating_sub(1)).collect();
    t.push('…');
    t
}

/// Builds the select menu (current page's results only, values are absolute
/// indices into the full result set) plus page-jump buttons when there's
/// more than one page.
pub fn search_select_menu(
    results: &[(Track, ScoreBreakdown)],
    page: usize,
) -> Vec<CreateActionRow> {
    let start = page * SEARCH_PAGE_SIZE;
    let options: Vec<CreateSelectMenuOption> = results
        .iter()
        .enumerate()
        .skip(start)
        .take(SEARCH_PAGE_SIZE)
        .map(|(i, (t, _))| {
            CreateSelectMenuOption::new(truncate_label(&t.escaped_title(), 25), i.to_string())
        })
        .collect();
    let select_row = CreateActionRow::SelectMenu(CreateSelectMenu::new(
        SEARCH_PICK,
        CreateSelectMenuKind::String { options },
    ));

    let total_pages = search_page_count(results.len());
    if total_pages <= 1 {
        return vec![select_row];
    }

    let buttons = (0..total_pages)
        .map(|p| {
            CreateButton::new(format!("{SEARCH_PAGE_PREFIX}{p}"))
                .label(format!("Page {}", p + 1))
                .style(if p == page {
                    ButtonStyle::Primary
                } else {
                    ButtonStyle::Secondary
                })
                .disabled(p == page)
        })
        .collect();
    vec![select_row, CreateActionRow::Buttons(buttons)]
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

/// Same as `update_response`, but for the embed-based Now Playing message.
/// Clears any leftover plain-text content explicitly, since messages
/// created before this embed existed still have it set.
pub fn update_response_embed(
    embed: CreateEmbed,
    components: Vec<CreateActionRow>,
) -> CreateInteractionResponse {
    CreateInteractionResponse::UpdateMessage(
        CreateInteractionResponseMessage::new()
            .content("")
            .embed(embed)
            .components(components),
    )
}

pub fn selected_index(interaction: &ComponentInteraction) -> Option<usize> {
    match &interaction.data.kind {
        ComponentInteractionDataKind::StringSelect { values } => values.first()?.parse().ok(),
        _ => None,
    }
}
