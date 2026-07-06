use pyxeebot::components::now_playing_content;
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

#[test]
fn now_playing_content_shows_nothing_playing_when_empty() {
    let snapshot = PlayerSnapshot::default();
    assert_eq!(
        now_playing_content(&snapshot),
        "Nothing is playing right now."
    );
}

#[test]
fn now_playing_content_shows_playing_state() {
    let mut snapshot = PlayerSnapshot::default();
    snapshot.current = Some(track("Song Title", 42));
    snapshot.is_paused = false;
    snapshot.loop_mode = LoopMode::Off;
    let content = now_playing_content(&snapshot);
    assert!(content.contains("Now playing"));
    assert!(content.contains("Song Title"));
    assert!(content.contains("<@42>"));
    assert!(content.contains("2:05")); // 125 seconds
}

#[test]
fn now_playing_content_shows_paused_state() {
    let mut snapshot = PlayerSnapshot::default();
    snapshot.current = Some(track("Song Title", 1));
    snapshot.is_paused = true;
    let content = now_playing_content(&snapshot);
    assert!(content.contains("Paused"));
    assert!(!content.contains("Now playing"));
}

#[test]
fn now_playing_content_shows_loop_mode() {
    let mut snapshot = PlayerSnapshot::default();
    snapshot.current = Some(track("Song Title", 1));
    snapshot.loop_mode = LoopMode::All;
    let content = now_playing_content(&snapshot);
    assert!(content.contains("Entire queue"));
}
