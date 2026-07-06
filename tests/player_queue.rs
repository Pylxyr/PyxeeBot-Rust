use pyxeebot::models::{LoopMode, Track};
use pyxeebot::player::PlayerState;

fn track(title: &str, duration: i64) -> Track {
    Track {
        title: title.to_owned(),
        webpage_url: format!("https://example.com/{title}"),
        uploader: "Uploader".to_owned(),
        duration,
        requester_id: 1,
        query: title.to_owned(),
        thumbnail_url: String::new(),
        tags: Vec::new(),
        acodec: String::new(),
        abr: 0.0,
    }
}

fn sum_durations(state: &PlayerState) -> i64 {
    state.queue.iter().map(|t| t.duration).sum()
}

#[test]
fn push_back_accumulates_duration() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 100));
    state.push_back(track("b", 50));
    assert_eq!(state.total_duration, 150);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn push_back_evicts_front_at_capacity_and_fixes_total_duration() {
    let mut state = PlayerState::new(2, false, false);
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    // Queue is full; pushing a third evicts "a" from the front.
    state.push_back(track("c", 50));
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].title, "b");
    assert_eq!(state.queue[1].title, "c");
    assert_eq!(state.total_duration, 70);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn push_front_evicts_back_at_capacity_and_fixes_total_duration() {
    // This is the exact bug class fixed in the Python version: appendleft
    // on a full deque silently evicts the back, and total_duration must
    // account for that evicted track, not just add the new one.
    let mut state = PlayerState::new(2, false, false);
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    state.push_front(track("prev", 50));
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].title, "prev");
    assert_eq!(state.queue[1].title, "a");
    // "b" (duration 20) was evicted from the back.
    assert_eq!(state.total_duration, 60);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn pop_front_decrements_duration() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    let popped = state.pop_front().unwrap();
    assert_eq!(popped.title, "a");
    assert_eq!(state.total_duration, 20);
}

#[test]
fn play_previous_with_no_history_returns_false() {
    let mut state = PlayerState::new(10, false, false);
    assert!(!state.play_previous());
}

#[test]
fn play_previous_moves_history_to_current_and_current_back_to_queue() {
    let mut state = PlayerState::new(10, false, false);
    state.history.push(track("prev", 50));
    state.current = Some(track("current", 30));
    state.push_back(track("next", 10));

    assert!(state.play_previous());
    assert_eq!(state.current.as_ref().unwrap().title, "prev");
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].title, "current");
    assert_eq!(state.queue[1].title, "next");
    assert_eq!(state.total_duration, 40);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn play_previous_respects_capacity_eviction() {
    // Full queue: pushing the old "current" back onto the front must evict
    // the back and adjust total_duration accordingly, exactly like any
    // other push_front call.
    let mut state = PlayerState::new(2, false, false);
    state.history.push(track("prev", 50));
    state.current = Some(track("current", 30));
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    // queue is now [a, b], full at capacity 2.

    assert!(state.play_previous());
    assert_eq!(state.current.as_ref().unwrap().title, "prev");
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].title, "current");
    assert_eq!(state.queue[1].title, "a");
    assert_eq!(state.total_duration, 40);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn requeue_finished_loop_one_pushes_front_with_eviction() {
    let mut state = PlayerState::new(2, false, false);
    state.loop_mode = LoopMode::One;
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    state.requeue_finished(track("finished", 100));
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].title, "finished");
    assert_eq!(state.queue[1].title, "a");
    assert_eq!(state.total_duration, 110);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn requeue_finished_loop_all_pushes_back_with_eviction() {
    let mut state = PlayerState::new(2, false, false);
    state.loop_mode = LoopMode::All;
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    state.requeue_finished(track("finished", 100));
    assert_eq!(state.queue.len(), 2);
    assert_eq!(state.queue[0].title, "b");
    assert_eq!(state.queue[1].title, "finished");
    assert_eq!(state.total_duration, 120);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn requeue_finished_loop_off_drops_the_track() {
    let mut state = PlayerState::new(10, false, false);
    state.loop_mode = LoopMode::Off;
    state.push_back(track("a", 10));
    state.requeue_finished(track("finished", 100));
    assert_eq!(state.queue.len(), 1);
    assert_eq!(state.total_duration, 10);
}

#[test]
fn should_disconnect_when_empty_honours_stay_connected() {
    let mut state = PlayerState::new(10, false, false);
    state.stay_connected = true;
    assert!(!state.should_disconnect_when_empty(false));
    state.stay_connected = false;
    assert!(state.should_disconnect_when_empty(false));
    assert!(!state.should_disconnect_when_empty(true));
}

#[test]
fn should_disconnect_when_idle_requires_empty_queue_and_no_current() {
    let mut state = PlayerState::new(10, false, false);
    assert!(state.should_disconnect_when_idle());

    state.current = Some(track("a", 10));
    assert!(!state.should_disconnect_when_idle());
    state.current = None;

    state.push_back(track("b", 10));
    assert!(!state.should_disconnect_when_idle());
    state.pop_front();
    assert!(state.should_disconnect_when_idle());

    state.stay_connected = true;
    assert!(!state.should_disconnect_when_idle());
}

#[test]
fn clear_resets_total_duration() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    let removed = state.clear();
    assert_eq!(removed, 2);
    assert_eq!(state.total_duration, 0);
    assert!(state.queue.is_empty());
}

#[test]
fn remove_decrements_total_duration() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    state.push_back(track("c", 30));
    let removed = state.remove(1).unwrap();
    assert_eq!(removed.title, "b");
    assert_eq!(state.total_duration, 40);
    assert_eq!(state.total_duration, sum_durations(&state));
}

#[test]
fn remove_out_of_bounds_returns_none_and_leaves_state_untouched() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 10));
    assert!(state.remove(5).is_none());
    assert_eq!(state.total_duration, 10);
}

#[test]
fn move_track_reorders_without_changing_duration() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 10));
    state.push_back(track("b", 20));
    state.push_back(track("c", 30));
    assert!(state.move_track(0, 2));
    assert_eq!(state.queue[0].title, "b");
    assert_eq!(state.queue[1].title, "c");
    assert_eq!(state.queue[2].title, "a");
    assert_eq!(state.total_duration, 60);
}

#[test]
fn move_track_out_of_bounds_returns_false() {
    let mut state = PlayerState::new(10, false, false);
    state.push_back(track("a", 10));
    assert!(!state.move_track(0, 5));
    assert!(!state.move_track(5, 0));
}

#[test]
fn user_queue_count_filters_by_requester() {
    let mut state = PlayerState::new(10, false, false);
    let mut t1 = track("a", 10);
    t1.requester_id = 111;
    let mut t2 = track("b", 10);
    t2.requester_id = 222;
    let mut t3 = track("c", 10);
    t3.requester_id = 111;
    state.push_back(t1);
    state.push_back(t2);
    state.push_back(t3);
    assert_eq!(state.user_queue_count(111), 2);
    assert_eq!(state.user_queue_count(222), 1);
    assert_eq!(state.user_queue_count(999), 0);
}

#[test]
fn advance_moves_current_to_history_and_pulls_next() {
    let mut state = PlayerState::new(10, false, false);
    state.current = Some(track("now-playing", 10));
    state.push_back(track("next", 20));
    let new_current = state.advance();
    assert_eq!(new_current.unwrap().title, "next");
    assert_eq!(state.history.len(), 1);
    assert_eq!(state.history[0].title, "now-playing");
    assert_eq!(state.total_duration, 0);
}
