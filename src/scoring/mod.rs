mod factors;

use std::collections::HashSet;
use std::sync::LazyLock;

use chrono::NaiveDate;
use rapidfuzz::fuzz;
use regex::Regex;
use serde_json::Value;

use crate::constants::{
    SEARCH_ANIME_SIGNAL_TOKENS, SEARCH_GENERIC_TOKENS, SEARCH_PREFERRED_PHRASES,
};

// ── Weights (multiplied against [0, 1] signal values) ───────────────────────

const W_FUZZY_RATIO: f64 = 0.32;
const W_METADATA_RATIO: f64 = 0.20;
const W_TITLE_OVERLAP: f64 = 0.44;
const W_UPLOADER_OVERLAP: f64 = 0.50;
const W_METADATA_OVERLAP: f64 = 0.36;
const W_EXACT_METADATA: f64 = 0.18;
const W_PREFIX_MATCH: f64 = 0.10;
const W_ALL_TITLE_TOKENS: f64 = 0.16;
const W_ALL_METADATA_TOKENS: f64 = 0.24;

// ── Anchor-match scores ──────────────────────────────────────────────────────

const ANCHOR_UPLOADER_BASE: f64 = 1.05;
const ANCHOR_UPLOADER_PER_WORD: f64 = 0.20;
const ANCHOR_TITLE_BASE: f64 = 0.20;
const ANCHOR_TITLE_PER_WORD: f64 = 0.10;
const ANCHOR_NO_MATCH: f64 = -0.30;

static WORD_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z0-9]+").unwrap());

// ── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Intent {
    pub anime: bool,
    pub dash_format: bool,
    pub has_artist: bool,
}

#[derive(Debug, Clone)]
pub struct SearchQueryContext {
    pub normalized_query: String,
    pub raw_query_tokens: Vec<String>,
    pub query_tokens: Vec<String>,
    pub query_token_set: HashSet<String>,
    pub anchor_phrases: Vec<String>,
    pub intent: Intent,
}

#[derive(Debug, Clone)]
pub struct SearchEntryContext {
    pub item: Value,
    pub normalized_title: String,
    pub normalized_uploader: String,
    pub normalized_metadata: String,
    pub title_token_set: HashSet<String>,
    pub uploader_token_set: HashSet<String>,
    pub metadata_token_set: HashSet<String>,
    pub duration: i64,
    pub view_count: i64,
    pub channel_is_verified: bool,
    pub upload_date: String,
    pub was_live: bool,
    pub description_token_set: HashSet<String>,
}

/// Per-candidate score breakdown, kept for the `!why` debug command (Phase 3).
/// `rank_entries` always returns these; the caller decides whether to keep or
/// discard them — unlike the Python version's opt-in `breakdown` dict, there's
/// no perf reason in Rust to make this conditional.
#[derive(Debug, Clone, Default)]
pub struct ScoreBreakdown {
    pub final_score: f64,
    pub title_overlap: f64,
    pub uploader_overlap: f64,
    pub ratio: f64,
    pub metadata_ratio: f64,
    pub topic_bonus: f64,
    pub uploader_pref_bonus: f64,
    pub anchor_score: f64,
    pub artist_match_bonus: f64,
    pub strong_uploader_bonus: f64,
    pub artist_completion_bonus: f64,
    pub title_uploader_synergy: f64,
    pub preferred_bonus: f64,
    pub discouraged_penalty: f64,
    pub duration_bonus: f64,
    pub jp_original_bonus: f64,
    pub view_bonus: f64,
    pub verified_bonus: f64,
    pub recency_bonus: f64,
}

// ── Text normalization ───────────────────────────────────────────────────────

pub fn normalize_text(value: &str) -> String {
    WORD_RE
        .find_iter(&value.to_lowercase())
        .map(|m| m.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn tokenize_text(value: &str) -> Vec<String> {
    WORD_RE
        .find_iter(&value.to_lowercase())
        .map(|m| m.as_str().to_owned())
        .collect()
}

fn word_boundary_match(p: &str, t: &str) -> bool {
    if p == t || t.starts_with(&format!("{p} ")) || t.ends_with(&format!(" {p}")) {
        return true;
    }
    t.contains(&format!(" {p} "))
}

pub fn signal_tokens(query: &str) -> Vec<String> {
    let tokens = tokenize_text(query);
    let filtered: Vec<String> = tokens
        .iter()
        .filter(|t| {
            !SEARCH_GENERIC_TOKENS.contains(&t.as_str())
                || SEARCH_ANIME_SIGNAL_TOKENS.contains(&t.as_str())
        })
        .cloned()
        .collect();
    if filtered.is_empty() {
        tokens
    } else {
        filtered
    }
}

pub fn detect_intent(query: &str) -> Intent {
    let q = query.trim();
    Intent {
        anime: factors::anime_intent_re().is_match(q),
        dash_format: factors::dash_separated_re().is_match(q),
        has_artist: q.contains(' '),
    }
}

pub fn token_overlap_ratio(query_tokens: &[String], candidate: &HashSet<String>) -> f64 {
    if query_tokens.is_empty() || candidate.is_empty() {
        return 0.0;
    }
    let hits = query_tokens
        .iter()
        .filter(|t| candidate.contains(t.as_str()))
        .count();
    hits as f64 / query_tokens.len() as f64
}

// ── Entry preparation ────────────────────────────────────────────────────────

fn value_str<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn candidate_title_text(item: &Value) -> String {
    normalize_text(value_str(item, "title").unwrap_or(""))
}

fn candidate_uploader_text(item: &Value) -> String {
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    for key in ["channel", "uploader", "artist", "creator"] {
        if let Some(v) = value_str(item, key) {
            if seen.insert(v.to_owned()) {
                parts.push(v.to_owned());
            }
        }
    }
    normalize_text(&parts.join(" "))
}

pub fn prepare_entry(item: Value) -> SearchEntryContext {
    let normalized_title = candidate_title_text(&item);
    let normalized_uploader = candidate_uploader_text(&item);
    let title_token_set: HashSet<String> = tokenize_text(&normalized_title).into_iter().collect();
    let uploader_token_set: HashSet<String> =
        tokenize_text(&normalized_uploader).into_iter().collect();
    let normalized_metadata = [normalized_title.as_str(), normalized_uploader.as_str()]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let metadata_token_set: HashSet<String> = title_token_set
        .union(&uploader_token_set)
        .cloned()
        .collect();
    let description = value_str(&item, "description").unwrap_or("");
    let description_token_set: HashSet<String> =
        tokenize_text(&description.chars().take(500).collect::<String>())
            .into_iter()
            .collect();

    SearchEntryContext {
        duration: item.get("duration").and_then(Value::as_i64).unwrap_or(0),
        view_count: item.get("view_count").and_then(Value::as_i64).unwrap_or(0),
        channel_is_verified: item
            .get("channel_is_verified")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        upload_date: value_str(&item, "upload_date").unwrap_or("").to_owned(),
        was_live: item
            .get("was_live")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        item,
        normalized_title,
        normalized_uploader,
        normalized_metadata,
        title_token_set,
        uploader_token_set,
        metadata_token_set,
        description_token_set,
    }
}

// ── Anchor phrase derivation ─────────────────────────────────────────────────

fn derive_anchor_phrases_inner(query_tokens: &[String], uploader_texts: &[String]) -> Vec<String> {
    if query_tokens.is_empty() || uploader_texts.is_empty() {
        return Vec::new();
    }
    let max_phrase_size = 3.min(query_tokens.len());

    if uploader_texts.len() == 1 {
        let single = &uploader_texts[0];
        for size in (1..=max_phrase_size).rev() {
            let mut phrases = Vec::new();
            let mut seen = HashSet::new();
            for start in 0..=(query_tokens.len() - size) {
                let phrase = query_tokens[start..start + size].join(" ");
                if !seen.insert(phrase.clone()) {
                    continue;
                }
                if single.contains(phrase.as_str()) {
                    phrases.push(phrase);
                }
            }
            if !phrases.is_empty() {
                phrases.truncate(4);
                return phrases;
            }
        }
        return Vec::new();
    }

    for size in (1..=max_phrase_size).rev() {
        let mut matches: Vec<(usize, String)> = Vec::new();
        let mut seen = HashSet::new();
        for start in 0..=(query_tokens.len() - size) {
            let phrase = query_tokens[start..start + size].join(" ");
            if !seen.insert(phrase.clone()) {
                continue;
            }
            if size == 1 && phrase.len() <= 2 {
                continue;
            }
            let count = uploader_texts
                .iter()
                .filter(|t| word_boundary_match(&phrase, t.as_str()))
                .count();
            if count > 0 && count < uploader_texts.len() {
                matches.push((count, phrase));
            }
        }
        if !matches.is_empty() {
            matches.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let best = matches[0].0;
            let mut out: Vec<String> = matches
                .into_iter()
                .filter(|(cnt, _)| *cnt == best)
                .map(|(_, p)| p)
                .collect();
            out.truncate(4);
            return out;
        }
    }
    Vec::new()
}

pub fn derive_anchor_phrases(
    query_tokens: &[String],
    entries: &[SearchEntryContext],
) -> Vec<String> {
    let uploader_texts: Vec<String> = entries
        .iter()
        .filter(|e| !e.normalized_uploader.is_empty())
        .map(|e| e.normalized_uploader.clone())
        .collect();
    derive_anchor_phrases_inner(query_tokens, &uploader_texts)
}

pub fn build_query_context(
    search_text: &str,
    entries: &[SearchEntryContext],
) -> Option<SearchQueryContext> {
    if search_text.is_empty() {
        return None;
    }
    let query_tokens = signal_tokens(search_text);
    let normalized_query = normalize_text(search_text);
    if normalized_query.is_empty() {
        return None;
    }
    let anchor_phrases = derive_anchor_phrases(&query_tokens, entries);
    Some(SearchQueryContext {
        raw_query_tokens: tokenize_text(search_text),
        query_token_set: query_tokens.iter().cloned().collect(),
        query_tokens,
        normalized_query,
        anchor_phrases,
        intent: detect_intent(search_text),
    })
}

pub fn score_anchor_match(entry: &SearchEntryContext, anchor_phrases: &[String]) -> f64 {
    if anchor_phrases.is_empty() || entry.normalized_metadata.is_empty() {
        return 0.0;
    }
    let uploader_matches: Vec<&String> = anchor_phrases
        .iter()
        .filter(|p| word_boundary_match(p.as_str(), &entry.normalized_uploader))
        .collect();
    if !uploader_matches.is_empty() {
        let longest = uploader_matches
            .iter()
            .map(|p| p.split(' ').count())
            .max()
            .unwrap_or(1);
        return ANCHOR_UPLOADER_BASE + ((longest - 1) as f64 * ANCHOR_UPLOADER_PER_WORD);
    }
    let title_only: Vec<&String> = anchor_phrases
        .iter()
        .filter(|p| {
            word_boundary_match(p.as_str(), &entry.normalized_metadata)
                && !word_boundary_match(p.as_str(), &entry.normalized_uploader)
        })
        .collect();
    if !title_only.is_empty() {
        let longest = title_only
            .iter()
            .map(|p| p.split(' ').count())
            .max()
            .unwrap_or(1);
        return ANCHOR_TITLE_BASE + ((longest - 1) as f64 * ANCHOR_TITLE_PER_WORD);
    }
    ANCHOR_NO_MATCH
}

// ── Fuzzy ratio (rapidfuzz-rs has no partial_ratio; see factors.rs) ─────────

fn fuzzy_ratio(a: &str, b: &str) -> f64 {
    fuzz::ratio(a.chars(), b.chars())
}

// ── Top-level scoring ────────────────────────────────────────────────────────

pub fn score_entry(
    query: &SearchQueryContext,
    entry: &SearchEntryContext,
    curation_mode: bool,
    today: NaiveDate,
) -> ScoreBreakdown {
    if query.normalized_query.is_empty() || entry.normalized_metadata.is_empty() {
        return ScoreBreakdown::default();
    }

    let overlap = factors::compute_overlap_signals(query, entry);
    let ratio = fuzzy_ratio(&query.normalized_query, &entry.normalized_title);
    let metadata_ratio =
        factors::partial_ratio(&query.normalized_query, &entry.normalized_metadata);
    let exact_metadata_match = entry
        .normalized_metadata
        .contains(query.normalized_query.as_str()) as u8 as f64;
    let metadata_prefix_match = entry
        .normalized_metadata
        .starts_with(query.normalized_query.as_str()) as u8 as f64;
    let all_title_tokens_match = (!query.query_token_set.is_empty()
        && query.query_token_set.is_subset(&entry.title_token_set))
        as u8 as f64;
    let all_metadata_tokens_match = (!query.query_token_set.is_empty()
        && query.query_token_set.is_subset(&entry.metadata_token_set))
        as u8 as f64;

    let channel = factors::score_channel_signals(
        query,
        entry,
        overlap.uploader_overlap,
        overlap.title_overlap,
        curation_mode,
    );
    let completion = factors::score_completion_and_synergy(
        overlap.title_overlap,
        overlap.uploader_overlap,
        !overlap.missing_title_tokens.is_empty(),
        overlap.missing_title_uploader_overlap,
    );

    let dash_format_bonus = if query.intent.dash_format && ratio >= factors::THR_DASH_RATIO {
        factors::DASH_FORMAT_BONUS
    } else {
        0.0
    };
    let mut preferred_bonus = 0.0;
    for &(phrase, weight) in SEARCH_PREFERRED_PHRASES {
        if entry.normalized_metadata.contains(phrase) {
            preferred_bonus += weight;
        }
    }

    let discouraged_penalty =
        factors::score_discouraged_penalty(query, entry, curation_mode, query.intent.anime);
    let duration_bonus = factors::score_duration_bonus(entry.duration);
    let anchor_score = score_anchor_match(entry, &query.anchor_phrases);
    let view_bonus = factors::score_view_bonus(entry);
    let verified_bonus = if entry.channel_is_verified {
        factors::VERIFIED_BONUS
    } else {
        0.0
    };
    let recency_bonus = factors::score_recency_bonus(entry, discouraged_penalty, today);
    let jp_original_bonus = factors::score_jp_original_bonus(
        query,
        entry,
        overlap.uploader_overlap,
        discouraged_penalty,
    );

    let final_score = (ratio * W_FUZZY_RATIO)
        + (metadata_ratio * W_METADATA_RATIO)
        + (overlap.title_overlap * W_TITLE_OVERLAP)
        + (overlap.uploader_overlap * W_UPLOADER_OVERLAP)
        + (overlap.metadata_overlap * W_METADATA_OVERLAP)
        + (exact_metadata_match * W_EXACT_METADATA)
        + (metadata_prefix_match * W_PREFIX_MATCH)
        + (all_title_tokens_match * W_ALL_TITLE_TOKENS)
        + (all_metadata_tokens_match * W_ALL_METADATA_TOKENS)
        + channel.artist_match_bonus
        + channel.strong_uploader_bonus
        + channel.topic_bonus
        + channel.uploader_preference_bonus
        + completion.artist_completion_bonus
        + completion.title_uploader_synergy
        + dash_format_bonus
        + preferred_bonus
        + duration_bonus
        + anchor_score
        + jp_original_bonus
        + view_bonus
        + verified_bonus
        + recency_bonus
        - completion.title_only_penalty
        - discouraged_penalty;

    ScoreBreakdown {
        final_score,
        title_overlap: overlap.title_overlap,
        uploader_overlap: overlap.uploader_overlap,
        ratio,
        metadata_ratio,
        topic_bonus: channel.topic_bonus,
        uploader_pref_bonus: channel.uploader_preference_bonus,
        anchor_score,
        artist_match_bonus: channel.artist_match_bonus,
        strong_uploader_bonus: channel.strong_uploader_bonus,
        artist_completion_bonus: completion.artist_completion_bonus,
        title_uploader_synergy: completion.title_uploader_synergy,
        preferred_bonus,
        discouraged_penalty,
        duration_bonus,
        jp_original_bonus,
        view_bonus,
        verified_bonus,
        recency_bonus,
    }
}

/// Scores, sorts (descending; ties keep original order), and returns entries
/// alongside their breakdowns. Unlike the Python version, this does not own
/// any per-guild debug history — the `!why` command (Phase 3) is responsible
/// for keeping whatever slice of this it wants to remember.
pub fn rank_entries(
    search_text: &str,
    entries: Vec<Value>,
    curation_mode: bool,
) -> Vec<(Value, ScoreBreakdown)> {
    let prepared: Vec<SearchEntryContext> = entries
        .into_iter()
        .filter(|v| !v.is_null())
        .map(prepare_entry)
        .collect();
    if prepared.is_empty() {
        return Vec::new();
    }

    let ctx = match build_query_context(search_text, &prepared) {
        Some(c) => c,
        None => {
            return prepared
                .into_iter()
                .map(|e| (e.item, ScoreBreakdown::default()))
                .collect()
        }
    };

    let today = chrono::Local::now().date_naive();
    let mut scored: Vec<(usize, SearchEntryContext, ScoreBreakdown)> = prepared
        .into_iter()
        .enumerate()
        .map(|(i, entry)| {
            let bd = score_entry(&ctx, &entry, curation_mode, today);
            (i, entry, bd)
        })
        .collect();

    scored.sort_by(|a, b| {
        b.2.final_score
            .partial_cmp(&a.2.final_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.0.cmp(&b.0))
    });

    scored
        .into_iter()
        .map(|(_, entry, bd)| (entry.item, bd))
        .collect()
}
