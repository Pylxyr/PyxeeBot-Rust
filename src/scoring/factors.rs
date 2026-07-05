use std::sync::LazyLock;

use chrono::NaiveDate;
use rapidfuzz::fuzz;
use regex::Regex;
use serde_json::Value;

use super::{SearchEntryContext, SearchQueryContext};
use crate::constants::{
    SEARCH_CURATION_EXTRA_PHRASES, SEARCH_CURATION_EXTRA_TOKENS, SEARCH_DISCOURAGED_PHRASES,
    SEARCH_DISCOURAGED_TOKENS, SEARCH_PREFERRED_UPLOADER_TOKENS,
};

// ── Thresholds ───────────────────────────────────────────────────────────────

const THR_UPLOADER_STRONG: f64 = 0.45;
const THR_UPLOADER_WEAK: f64 = 0.20;
const THR_UPLOADER_FULL: f64 = 0.99;
const THR_UPLOADER_PARTIAL: f64 = 0.50;
const THR_TITLE_MIN: f64 = 0.45;
const THR_TITLE_HIGH: f64 = 0.75;
const THR_TITLE_SYNERGY: f64 = 0.55;
pub(super) const THR_DASH_RATIO: f64 = 0.70;
const THR_PENALTY_GATE: f64 = 0.50;
const MAX_DISCOURAGED_PENALTY: f64 = 0.65;
const THR_JP_LATIN_RATIO: f64 = 0.35;
const THR_JP_CJK_HANGUL: f64 = 1.5;

// ── Duration windows (seconds) ───────────────────────────────────────────────

const DUR_OK_MIN: i64 = 60;
const DUR_IDEAL_MIN: i64 = 90;
const DUR_IDEAL_MAX: i64 = 600;
const DUR_OK_MAX: i64 = 660;
const DUR_LONG: i64 = 900;

// ── View-count bonus scaling ──────────────────────────────────────────────────

const VIEW_MIN: i64 = 1_000;
const VIEW_BONUS_MAX: f64 = 0.35;
const VIEW_BONUS_LOG_REF: f64 = 3.0;
const VIEW_BONUS_LOG_RNG: f64 = 6.0;
const VIEW_BONUS_TOPIC: f64 = 0.05;

// ── Recency windows (days) and bonuses ────────────────────────────────────────

const RECENCY_DAYS_NEW: i64 = 180;
const RECENCY_DAYS_RECENT: i64 = 365;
const RECENCY_DAYS_OLDER: i64 = 730;
const RECENCY_BONUS_NEW: f64 = 0.20;
const RECENCY_BONUS_RECENT: f64 = 0.12;
const RECENCY_BONUS_OLDER: f64 = 0.06;

// ── Signal bonuses and penalties ──────────────────────────────────────────────

const ARTIST_BONUS_MULTI: f64 = 0.28;
const ARTIST_BONUS_SINGLE: f64 = 0.12;
const STRONG_UPLOADER_BONUS: f64 = 0.18;
const TOPIC_BONUS_NORMAL: f64 = 0.30;
const TOPIC_BONUS_CURATION: f64 = 0.55;
const COMPLETION_SCALE: f64 = 0.90;
const COMPLETION_BONUS_FULL: f64 = 0.45;
const COMPLETION_BONUS_PARTIAL: f64 = 0.20;
const COMPLETION_BONUS_WEAK: f64 = 0.12;
const SYNERGY_BONUS_FULL: f64 = 0.36;
const SYNERGY_BONUS_PARTIAL: f64 = 0.24;
pub(super) const DASH_FORMAT_BONUS: f64 = 0.18;
pub(super) const VERIFIED_BONUS: f64 = 0.15;
const JP_ORIGINAL_BONUS: f64 = 0.55;
const JP_ROMANIZED_ANCHOR_BONUS: f64 = 1.80;
const WAS_LIVE_PENALTY: f64 = 0.50;
const DURATION_BONUS_IDEAL: f64 = 0.10;
const DURATION_BONUS_OK: f64 = 0.05;
const DURATION_PENALTY_LONG: f64 = -0.12;
const TITLE_ONLY_PENALTY: f64 = 0.40;
const JP_COVER_PENALTY: f64 = 0.75;
const CURATION_PHRASE_PENALTY: f64 = 0.65;
const CURATION_PENALTY_SCALE: f64 = 3.0;
const ANIME_LIVE_PENALTY_SCALE: f64 = 0.3;

// ── Regexes ──────────────────────────────────────────────────────────────────

static ANIME_INTENT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b(op|ed|ost|opening|ending|theme|insert\s*song|anime|season)\b").unwrap()
});
static DASH_SEPARATED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^.+\s*[-–]\s*.+$").unwrap());
static JP_COVER_BRACKET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^[\s\[【\(]*(ギター|ピアノ|バイオリン|チェロ|ベース|ドラム|弾いてみた|歌ってみた|叩いてみた|カバー|アレンジ|フル)[\s\]】\)]*",
    )
    .unwrap()
});
static BRACKET_STRIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\(\[（【][^\)\]）】]*[\)\]）】]").unwrap());
static CJK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\u{3040}-\u{30ff}\u{4e00}-\u{9fff}]").unwrap());
static HANGUL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\u{AC00}-\u{D7AF}\u{3130}-\u{318F}]").unwrap());
static KANA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\u{3040}-\u{30ff}]").unwrap());
static JP_EVENT_FROM_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)[(（]from\s").unwrap());

pub(super) fn anime_intent_re() -> &'static Regex {
    &ANIME_INTENT_RE
}
pub(super) fn dash_separated_re() -> &'static Regex {
    &DASH_SEPARATED_RE
}

// ── Fuzzy: partial_ratio substitute ──────────────────────────────────────────
//
// rapidfuzz-rs (0.5.0) exposes `ratio` but not Python rapidfuzz's
// `partial_ratio` — its own test suite even has the equivalent calls
// commented out. This reproduces the same intent (best alignment of the
// shorter string against a same-length window of the longer one) using
// `ratio` as the underlying primitive.
pub(super) fn partial_ratio(a: &str, b: &str) -> f64 {
    let (shorter, longer): (Vec<char>, Vec<char>) = {
        let ac: Vec<char> = a.chars().collect();
        let bc: Vec<char> = b.chars().collect();
        if ac.len() <= bc.len() {
            (ac, bc)
        } else {
            (bc, ac)
        }
    };
    if shorter.is_empty() {
        return if longer.is_empty() { 1.0 } else { 0.0 };
    }
    if shorter.len() >= longer.len() {
        return fuzz::ratio(shorter.iter().copied(), longer.iter().copied());
    }
    let window = shorter.len();
    let mut best = 0.0_f64;
    for start in 0..=(longer.len() - window) {
        let score = fuzz::ratio(
            shorter.iter().copied(),
            longer[start..start + window].iter().copied(),
        );
        if score > best {
            best = score;
        }
        if best >= 1.0 {
            break;
        }
    }
    best
}

// ── Overlap signals ──────────────────────────────────────────────────────────

pub(super) struct OverlapSignals {
    pub title_overlap: f64,
    pub uploader_overlap: f64,
    pub metadata_overlap: f64,
    pub missing_title_tokens: Vec<String>,
    pub missing_title_uploader_overlap: f64,
}

pub(super) fn compute_overlap_signals(
    query: &SearchQueryContext,
    entry: &SearchEntryContext,
) -> OverlapSignals {
    let title_overlap = super::token_overlap_ratio(&query.query_tokens, &entry.title_token_set);
    let uploader_overlap =
        super::token_overlap_ratio(&query.query_tokens, &entry.uploader_token_set);
    let metadata_overlap =
        super::token_overlap_ratio(&query.query_tokens, &entry.metadata_token_set);
    let missing_title_tokens: Vec<String> = query
        .query_tokens
        .iter()
        .filter(|t| !entry.title_token_set.contains(t.as_str()))
        .cloned()
        .collect();
    let missing_title_uploader_overlap =
        super::token_overlap_ratio(&missing_title_tokens, &entry.uploader_token_set);
    OverlapSignals {
        title_overlap,
        uploader_overlap,
        metadata_overlap,
        missing_title_tokens,
        missing_title_uploader_overlap,
    }
}

// ── Channel signals ──────────────────────────────────────────────────────────

pub(super) struct ChannelSignals {
    pub artist_match_bonus: f64,
    pub strong_uploader_bonus: f64,
    pub topic_bonus: f64,
    pub uploader_preference_bonus: f64,
}

pub(super) fn score_channel_signals(
    query: &SearchQueryContext,
    entry: &SearchEntryContext,
    uploader_overlap: f64,
    title_overlap: f64,
    curation_mode: bool,
) -> ChannelSignals {
    let artist_token_matches = query
        .query_token_set
        .intersection(&entry.uploader_token_set)
        .count();
    let artist_match_bonus = if artist_token_matches >= 2 {
        ARTIST_BONUS_MULTI
    } else if artist_token_matches == 1 {
        ARTIST_BONUS_SINGLE
    } else {
        0.0
    };
    let strong_uploader_bonus = if uploader_overlap >= THR_UPLOADER_STRONG {
        STRONG_UPLOADER_BONUS
    } else {
        0.0
    };
    let has_topic = entry.uploader_token_set.contains("topic");
    let mut topic_bonus = if has_topic {
        if curation_mode {
            TOPIC_BONUS_CURATION
        } else {
            TOPIC_BONUS_NORMAL
        }
    } else {
        0.0
    };

    let mut uploader_preference_bonus = 0.0;
    for &(tok, weight) in SEARCH_PREFERRED_UPLOADER_TOKENS {
        if entry.uploader_token_set.contains(tok) {
            uploader_preference_bonus += weight;
        }
    }

    if title_overlap == 0.0 {
        topic_bonus = 0.0;
        uploader_preference_bonus = 0.0;
    }
    ChannelSignals {
        artist_match_bonus,
        strong_uploader_bonus,
        topic_bonus,
        uploader_preference_bonus,
    }
}

// ── Completion & synergy ──────────────────────────────────────────────────────

pub(super) struct CompletionSignals {
    pub artist_completion_bonus: f64,
    pub title_only_penalty: f64,
    pub title_uploader_synergy: f64,
}

pub(super) fn score_completion_and_synergy(
    title_overlap: f64,
    uploader_overlap: f64,
    has_missing_title_tokens: bool,
    missing_title_uploader_overlap: f64,
) -> CompletionSignals {
    let mut artist_completion_bonus = 0.0;
    let mut title_only_penalty = 0.0;

    if has_missing_title_tokens && title_overlap >= THR_TITLE_MIN {
        artist_completion_bonus += missing_title_uploader_overlap * COMPLETION_SCALE;
        if missing_title_uploader_overlap >= THR_UPLOADER_FULL {
            artist_completion_bonus += COMPLETION_BONUS_FULL;
        } else if missing_title_uploader_overlap >= THR_UPLOADER_PARTIAL {
            artist_completion_bonus += COMPLETION_BONUS_PARTIAL;
        } else if title_overlap >= THR_TITLE_HIGH {
            title_only_penalty = TITLE_ONLY_PENALTY;
        }
    } else if !has_missing_title_tokens && uploader_overlap >= THR_UPLOADER_WEAK {
        artist_completion_bonus += COMPLETION_BONUS_WEAK;
    }

    let title_uploader_synergy =
        if title_overlap >= THR_TITLE_SYNERGY && uploader_overlap >= THR_UPLOADER_WEAK {
            SYNERGY_BONUS_FULL
        } else if title_overlap >= THR_TITLE_HIGH
            && has_missing_title_tokens
            && missing_title_uploader_overlap >= THR_UPLOADER_PARTIAL
        {
            SYNERGY_BONUS_PARTIAL
        } else {
            0.0
        };

    CompletionSignals {
        artist_completion_bonus,
        title_only_penalty,
        title_uploader_synergy,
    }
}

// ── Discouraged penalty ───────────────────────────────────────────────────────

pub(super) fn score_discouraged_penalty(
    query: &SearchQueryContext,
    entry: &SearchEntryContext,
    curation_mode: bool,
    is_anime_query: bool,
) -> f64 {
    let mut penalty = 0.0;
    let raw_has = |tok: &str| query.raw_query_tokens.iter().any(|t| t == tok);

    for &(token, weight) in SEARCH_DISCOURAGED_TOKENS {
        let present =
            entry.metadata_token_set.contains(token) || entry.description_token_set.contains(token);
        if !raw_has(token) && present {
            if is_anime_query && matches!(token, "live" | "stage" | "concert") {
                penalty += weight * ANIME_LIVE_PENALTY_SCALE;
            } else if curation_mode && SEARCH_CURATION_EXTRA_TOKENS.contains(&token) {
                penalty += weight * CURATION_PENALTY_SCALE;
            } else {
                penalty += weight;
            }
        }
    }

    for &(phrase, weight) in SEARCH_DISCOURAGED_PHRASES {
        if !query.normalized_query.contains(phrase) && entry.normalized_metadata.contains(phrase) {
            if is_anime_query && phrase == "tv size" {
                continue;
            }
            penalty += weight;
        }
    }

    if curation_mode {
        for &phrase in SEARCH_CURATION_EXTRA_PHRASES {
            if !query.normalized_query.contains(phrase)
                && entry.normalized_metadata.contains(phrase)
            {
                penalty += CURATION_PHRASE_PENALTY;
            }
        }
    }

    let raw_title = entry
        .item
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("");
    if JP_COVER_BRACKET_RE.is_match(raw_title) {
        let asks_cover = [
            "guitar",
            "piano",
            "violin",
            "bass",
            "acoustic",
            "cover",
            "fingerstyle",
            "ukulele",
        ]
        .iter()
        .any(|tok| raw_has(tok));
        if !asks_cover {
            penalty += JP_COVER_PENALTY;
        }
    }

    if entry.was_live {
        penalty += WAS_LIVE_PENALTY;
    }

    if !curation_mode {
        penalty = penalty.min(MAX_DISCOURAGED_PENALTY);
    }
    penalty
}

// ── Duration / view / recency / JP-original bonuses ──────────────────────────

pub(super) fn score_duration_bonus(duration: i64) -> f64 {
    if (DUR_IDEAL_MIN..=DUR_IDEAL_MAX).contains(&duration) {
        DURATION_BONUS_IDEAL
    } else if (DUR_OK_MIN..=DUR_OK_MAX).contains(&duration) {
        DURATION_BONUS_OK
    } else if duration > DUR_LONG {
        DURATION_PENALTY_LONG
    } else {
        0.0
    }
}

pub(super) fn score_view_bonus(entry: &SearchEntryContext) -> f64 {
    let vc = entry.view_count;
    if vc >= VIEW_MIN {
        let bonus =
            ((vc as f64).log10() - VIEW_BONUS_LOG_REF) / VIEW_BONUS_LOG_RNG * VIEW_BONUS_MAX;
        return bonus.min(VIEW_BONUS_MAX);
    }
    if entry.uploader_token_set.contains("topic") {
        return VIEW_BONUS_TOPIC;
    }
    0.0
}

pub(super) fn score_recency_bonus(
    entry: &SearchEntryContext,
    discouraged_penalty: f64,
    today: NaiveDate,
) -> f64 {
    let ud = &entry.upload_date;
    if !(ud.len() == 8
        && ud.chars().all(|c| c.is_ascii_digit())
        && discouraged_penalty < THR_PENALTY_GATE)
    {
        return 0.0;
    }
    let year: i32 = match ud[0..4].parse() {
        Ok(v) => v,
        Err(_) => return 0.0,
    };
    let month: u32 = match ud[4..6].parse() {
        Ok(v) => v,
        Err(_) => return 0.0,
    };
    let day: u32 = match ud[6..8].parse() {
        Ok(v) => v,
        Err(_) => return 0.0,
    };
    let uploaded = match NaiveDate::from_ymd_opt(year, month, day) {
        Some(d) => d,
        None => return 0.0,
    };
    let days_old = (today - uploaded).num_days();
    if days_old <= RECENCY_DAYS_NEW {
        RECENCY_BONUS_NEW
    } else if days_old <= RECENCY_DAYS_RECENT {
        RECENCY_BONUS_RECENT
    } else if days_old <= RECENCY_DAYS_OLDER {
        RECENCY_BONUS_OLDER
    } else {
        0.0
    }
}

pub(super) fn score_jp_original_bonus(
    query: &SearchQueryContext,
    entry: &SearchEntryContext,
    uploader_overlap: f64,
    discouraged_penalty: f64,
) -> f64 {
    if discouraged_penalty >= THR_PENALTY_GATE {
        return 0.0;
    }
    let raw_title = entry
        .item
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("");
    let title_core = BRACKET_STRIP_RE
        .replace_all(raw_title, "")
        .trim()
        .to_owned();
    if !CJK_RE.is_match(&title_core) {
        return 0.0;
    }
    let latin_chars = title_core.chars().filter(char::is_ascii_alphabetic).count();
    let total_chars = title_core.chars().filter(|&c| c != ' ').count();
    let hangul_count = HANGUL_RE.find_iter(&title_core).count();
    let cjk_count = CJK_RE.find_iter(&title_core).count();
    let latin_ratio = if total_chars > 0 {
        latin_chars as f64 / total_chars as f64
    } else {
        1.0
    };
    let kana_count = KANA_RE.find_iter(&title_core).count();
    let is_jp = kana_count > 0
        && latin_ratio < THR_JP_LATIN_RATIO
        && (hangul_count == 0 || cjk_count as f64 > hangul_count as f64 * THR_JP_CJK_HANGUL);
    if !is_jp || JP_EVENT_FROM_RE.is_match(raw_title) {
        return 0.0;
    }
    let mut bonus = JP_ORIGINAL_BONUS;
    if uploader_overlap > 0.0 && !CJK_RE.is_match(&query.normalized_query) {
        bonus += JP_ROMANIZED_ANCHOR_BONUS;
    }
    bonus
}
