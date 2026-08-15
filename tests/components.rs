use std::sync::Arc;

use pyxeebot::components::now_playing_embed;
use pyxeebot::models::{LoopMode, Track};
use pyxeebot::player::PlayerSnapshot;

fn track(title: &str, requester_id: u64) -> Track {
    Track {
        title: title.to_owned(),
        webpage_url: format!("https://example.com/{title}"),
        uploader: "Uploader".to_owned(),
        duration: 125,
        requester_id,
        query: title.to_owned(),
        thumbnail_url: String::new(),
        tags: Vec::new(),
        acodec: String::new(),
        abr: 0.0,
    }
}

fn embed_json(snapshot: &PlayerSnapshot) -> String {
    serde_json::to_string(&now_playing_embed(snapshot)).unwrap()
}

#[test]
fn now_playing_embed_shows_nothing_playing_when_empty() {
    let snapshot = PlayerSnapshot::default();
    assert!(embed_json(&snapshot).contains("Nothing is playing right now."));
}

#[test]
fn now_playing_embed_shows_playing_state() {
    let snapshot = PlayerSnapshot {
        current: Some(Arc::new(track("Song Title", 42))),
        is_paused: false,
        loop_mode: LoopMode::Off,
        ..Default::default()
    };
    let json = embed_json(&snapshot);
    assert!(json.contains("Now Playing"));
    assert!(json.contains("Song Title"));
    assert!(json.contains("<@42>"));
    assert!(json.contains("2:05"));
}

#[test]
fn now_playing_embed_shows_paused_state() {
    let snapshot = PlayerSnapshot {
        current: Some(Arc::new(track("Song Title", 1))),
        is_paused: true,
        ..Default::default()
    };
    let json = embed_json(&snapshot);
    assert!(json.contains("Paused"));
    assert!(!json.contains("Now Playing"));
}

#[test]
fn now_playing_embed_shows_loop_mode() {
    let snapshot = PlayerSnapshot {
        current: Some(Arc::new(track("Song Title", 1))),
        loop_mode: LoopMode::All,
        ..Default::default()
    };
    assert!(embed_json(&snapshot).contains("Entire queue"));
}

#[test]
fn now_playing_embed_shows_up_next() {
    let snapshot = PlayerSnapshot {
        current: Some(Arc::new(track("Song Title", 1))),
        queue: vec![Arc::new(track("Next Song", 2)), Arc::new(track("Third Song", 3))],
        ..Default::default()
    };
    let json = embed_json(&snapshot);
    assert!(json.contains("Up next"));
    assert!(json.contains("Next Song"));
    assert!(json.contains("Third Song"));
}
