use pyxeebot::scoring::{normalize_text, rank_entries, token_overlap_ratio, tokenize_text};
use serde_json::json;
use std::collections::HashSet;

#[test]
fn normalize_text_lowercases_and_strips_punctuation() {
    assert_eq!(
        normalize_text("Hello, World! (Official Video)"),
        "hello world official video"
    );
}

#[test]
fn normalize_text_handles_empty_and_symbols_only() {
    assert_eq!(normalize_text(""), "");
    assert_eq!(normalize_text("!!!___***"), "");
}

#[test]
fn tokenize_text_splits_on_non_alnum() {
    assert_eq!(
        tokenize_text("foo-bar_baz 123"),
        vec!["foo", "bar", "baz", "123"]
    );
}

#[test]
fn token_overlap_ratio_full_and_partial() {
    let full: HashSet<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
    let tokens = vec!["a".to_string(), "b".to_string()];
    assert_eq!(token_overlap_ratio(&tokens, &full), 1.0);

    let partial: HashSet<String> = ["a"].iter().map(|s| s.to_string()).collect();
    assert_eq!(token_overlap_ratio(&tokens, &partial), 0.5);
}

#[test]
fn token_overlap_ratio_empty_inputs() {
    let empty_set: HashSet<String> = HashSet::new();
    assert_eq!(token_overlap_ratio(&[], &empty_set), 0.0);
    let some_set: HashSet<String> = ["a".to_string()].into_iter().collect();
    assert_eq!(token_overlap_ratio(&[], &some_set), 0.0);
}

fn candidate(title: &str, uploader: &str, duration: i64, views: i64) -> serde_json::Value {
    json!({
        "title": title,
        "uploader": uploader,
        "channel": uploader,
        "duration": duration,
        "view_count": views,
        "webpage_url": format!("https://example.com/{title}"),
    })
}

#[test]
fn rank_entries_prefers_official_over_live_cover() {
    let entries = vec![
        candidate(
            "Song Title (Official Audio)",
            "Artist Name - Topic",
            200,
            5_000_000,
        ),
        candidate(
            "Song Title (Live at Some Festival)",
            "Random Fan Channel",
            210,
            800,
        ),
    ];
    let ranked = rank_entries("Artist Name Song Title", entries, false);
    assert_eq!(ranked.len(), 2);
    let (best, _) = &ranked[0];
    assert_eq!(best["title"], "Song Title (Official Audio)");
}

#[test]
fn rank_entries_empty_input_returns_empty() {
    let ranked = rank_entries("anything", vec![], false);
    assert!(ranked.is_empty());
}

#[test]
fn rank_entries_ties_keep_original_order() {
    // Two identical candidates: score is identical, so original order (by
    // input index) must be preserved rather than reordered arbitrarily.
    let entries = vec![
        candidate("Same Title", "Same Uploader", 200, 100),
        candidate("Same Title", "Same Uploader", 200, 100),
    ];
    let urls: Vec<String> = entries
        .iter()
        .map(|e| e["webpage_url"].as_str().unwrap().to_owned())
        .collect();
    let ranked = rank_entries("same title", entries, false);
    let ranked_urls: Vec<String> = ranked
        .iter()
        .map(|(v, _)| v["webpage_url"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(urls, ranked_urls);
}

#[test]
fn rank_entries_discouraged_token_lowers_rank() {
    let entries = vec![
        candidate("Song Title Official Audio", "Some Label", 200, 10_000),
        candidate(
            "Song Title Nightcore Remix",
            "Some Other Channel",
            200,
            10_000,
        ),
    ];
    let ranked = rank_entries("Song Title", entries, false);
    let (best, _) = &ranked[0];
    assert_eq!(best["title"], "Song Title Official Audio");
}
