use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pyxeebot::db::{Database, QueueEntryRef};

async fn temp_db() -> Database {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path: PathBuf = std::env::temp_dir().join(format!("pyxeebot-test-{nanos}.sqlite3"));
    Database::new(&path)
        .await
        .expect("database should initialize")
}

#[tokio::test]
async fn prefix_defaults_to_none_then_round_trips() {
    let db = temp_db().await;
    assert_eq!(db.get_prefix(1).await, None);
    db.set_prefix(1, "?").await.unwrap();
    assert_eq!(db.get_prefix(1).await, Some("?".to_owned()));
}

#[tokio::test]
async fn dj_role_round_trips_and_can_be_cleared() {
    let db = temp_db().await;
    assert_eq!(db.get_dj_role_id(1).await, None);
    db.set_dj_role_id(1, Some(555), "!").await.unwrap();
    assert_eq!(db.get_dj_role_id(1).await, Some(555));
    db.set_dj_role_id(1, None, "!").await.unwrap();
    assert_eq!(db.get_dj_role_id(1).await, None);
}

#[tokio::test]
async fn stay_connected_defaults_false_and_round_trips() {
    let db = temp_db().await;
    assert!(!db.get_stay_connected(1).await);
    db.set_stay_connected(1, true, "!").await.unwrap();
    assert!(db.get_stay_connected(1).await);
    db.set_stay_connected(1, false, "!").await.unwrap();
    assert!(!db.get_stay_connected(1).await);
}

#[tokio::test]
async fn autoplay_defaults_false_and_round_trips() {
    let db = temp_db().await;
    assert!(!db.get_autoplay(1).await);
    db.set_autoplay(1, true, "!").await.unwrap();
    assert!(db.get_autoplay(1).await);
}

#[tokio::test]
async fn settings_are_isolated_per_guild() {
    let db = temp_db().await;
    db.set_stay_connected(1, true, "!").await.unwrap();
    assert!(db.get_stay_connected(1).await);
    assert!(!db.get_stay_connected(2).await);
}

#[tokio::test]
async fn save_and_list_and_load_playlist() {
    let db = temp_db().await;
    let entries = vec![
        QueueEntryRef {
            query: "song a",
            title: "Song A",
            webpage_url: "https://example.com/a",
            requester_id: 1,
        },
        QueueEntryRef {
            query: "song b",
            title: "Song B",
            webpage_url: "https://example.com/b",
            requester_id: 2,
        },
    ];
    db.save_playlist(1, "myplaylist", 42, &entries)
        .await
        .unwrap();

    let playlists = db.list_playlists(1).await.unwrap();
    assert_eq!(playlists.len(), 1);
    assert_eq!(playlists[0].name, "myplaylist");
    assert_eq!(playlists[0].track_count, 2);

    let loaded = db.get_playlist_entries(1, "myplaylist").await.unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].title, "Song A");
    assert_eq!(loaded[1].title, "Song B");
}

#[tokio::test]
async fn saving_playlist_again_replaces_old_entries() {
    let db = temp_db().await;
    let first = vec![QueueEntryRef {
        query: "a",
        title: "A",
        webpage_url: "https://example.com/a",
        requester_id: 1,
    }];
    db.save_playlist(1, "p", 1, &first).await.unwrap();

    let second = vec![
        QueueEntryRef {
            query: "b",
            title: "B",
            webpage_url: "https://example.com/b",
            requester_id: 1,
        },
        QueueEntryRef {
            query: "c",
            title: "C",
            webpage_url: "https://example.com/c",
            requester_id: 1,
        },
    ];
    db.save_playlist(1, "p", 1, &second).await.unwrap();

    let loaded = db.get_playlist_entries(1, "p").await.unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].title, "B");
}

#[tokio::test]
async fn delete_playlist_returns_false_when_missing() {
    let db = temp_db().await;
    assert!(!db.delete_playlist(1, "nonexistent").await.unwrap());
    let entries = vec![QueueEntryRef {
        query: "a",
        title: "A",
        webpage_url: "https://example.com/a",
        requester_id: 1,
    }];
    db.save_playlist(1, "p", 1, &entries).await.unwrap();
    assert!(db.delete_playlist(1, "p").await.unwrap());
    assert!(db.list_playlists(1).await.unwrap().is_empty());
}

#[tokio::test]
async fn queue_snapshot_round_trips_and_can_be_cleared() {
    let db = temp_db().await;
    let entries = vec![QueueEntryRef {
        query: "a",
        title: "A",
        webpage_url: "https://example.com/a",
        requester_id: 7,
    }];
    db.save_queue_snapshot(1, &entries).await.unwrap();

    let loaded = db.load_queue_snapshot(1).await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].title, "A");
    assert_eq!(loaded[0].requester_id, 7);

    db.save_queue_snapshot(1, &[]).await.unwrap();
    assert!(db.load_queue_snapshot(1).await.unwrap().is_empty());
}

#[tokio::test]
async fn play_history_and_top_played_and_top_requesters() {
    let db = temp_db().await;
    db.add_play_history(1, "Song A", "https://example.com/a", 10)
        .await
        .unwrap();
    db.add_play_history(1, "Song A", "https://example.com/a", 10)
        .await
        .unwrap();
    db.add_play_history(1, "Song B", "https://example.com/b", 20)
        .await
        .unwrap();

    let top = db.get_top_played(1, 10).await.unwrap();
    assert_eq!(top[0].title, "Song A");
    assert_eq!(top[0].play_count, 2);

    let requesters = db.get_top_requesters(1, 10).await.unwrap();
    assert_eq!(requesters[0].requester_id, 10);
    assert_eq!(requesters[0].request_count, 2);
}

#[tokio::test]
async fn restorable_guilds_need_both_a_channel_and_a_saved_queue() {
    let db = temp_db().await;
    assert!(db.list_restorable_guilds().await.unwrap().is_empty());

    db.set_last_voice_channel(1, Some(999), "!").await.unwrap();
    assert!(
        db.list_restorable_guilds().await.unwrap().is_empty(),
        "a channel with no saved queue shouldn't be restorable"
    );

    let entries = vec![QueueEntryRef {
        query: "a",
        title: "A",
        webpage_url: "https://example.com/a",
        requester_id: 7,
    }];
    db.save_queue_snapshot(1, &entries).await.unwrap();
    assert_eq!(db.list_restorable_guilds().await.unwrap(), vec![(1, 999)]);

    db.set_last_voice_channel(1, None, "!").await.unwrap();
    assert!(
        db.list_restorable_guilds().await.unwrap().is_empty(),
        "clearing the channel should drop it from the restorable list even though the queue is still saved"
    );
}
