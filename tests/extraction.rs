use std::path::PathBuf;

use pyxeebot::config::Config;
use pyxeebot::extraction::{extract_args, search_args};

fn test_config() -> Config {
    Config {
        token: "test-token".to_owned(),
        default_prefix: "!".to_owned(),
        bot_owners: vec![123],
        log_level: "INFO".to_owned(),
        db_path: PathBuf::from("test.sqlite3"),
        log_to_file: false,
        log_dir: PathBuf::from("logs"),
        max_queue_size: 100,
        max_queue_size_per_user: 0,
        max_playlist_size: 25,
        idle_timeout_secs: 180,
        empty_channel_timeout_secs: 60,
        ytdlp_cookies_file: None,
        ytdlp_cache_dir: PathBuf::from("ytdlp-cache"),
        ytdlp_js_runtime_path: None,
        ytdlp_pot_provider_base_url: None,
        ytdlp_socket_timeout: 15,
        ytdlp_prefetch_count: 1,
        ytdlp_concurrent_extracts: 1,
        ytdlp_curation_concurrency: 3,
        near_end_prefetch_secs: 30,
        opus_bitrate_kbps: 64,
        ytdlp_search_results: 5,
        ytdlp_resolve_cache_size: 128,
        ytdlp_resolve_cache_ttl_secs: 1800,
        ytdlp_search_cache_size: 200,
        ytdlp_search_cache_ttl_secs: 600,
        ytdlp_extract_timeout_secs: 45,
        np_auto_refresh: false,
        np_auto_refresh_interval: 30,
        error_announce: true,
        lastfm_api_key: None,
        restore_queue_on_restart: true,
        bot_activity_url: "pylxyr.github.io/PyxeeBot-Page/".to_owned(),
    }
}

#[test]
fn extract_args_includes_format_and_no_playlist() {
    let config = test_config();
    let args = extract_args(&config, "https://example.com/video", false);
    assert!(args.contains(&"--dump-json".to_owned()));
    assert!(args.contains(&"--no-playlist".to_owned()));
    assert!(!args.contains(&"--flat-playlist".to_owned()));
    assert_eq!(args.last().unwrap(), "https://example.com/video");
}

#[test]
fn extract_args_flat_playlist_adds_flag() {
    let config = test_config();
    let args = extract_args(&config, "https://example.com/playlist", true);
    assert!(args.contains(&"--flat-playlist".to_owned()));
}

#[test]
fn extract_args_always_sets_cache_dir() {
    // yt-dlp's default cache dir is blocked by ProtectHome=read-only;
    // --cache-dir must always point at config.ytdlp_cache_dir instead.
    let config = test_config();
    let args = extract_args(&config, "https://example.com/video", false);
    let idx = args.iter().position(|a| a == "--cache-dir").unwrap();
    assert_eq!(args[idx + 1], config.ytdlp_cache_dir.display().to_string());
}

#[test]
fn extract_args_omits_pot_provider_by_default() {
    let config = test_config();
    let args = extract_args(&config, "https://example.com/video", false);
    assert!(!args.iter().any(|a| a.starts_with("youtubepot-bgutilhttp:")));
}

#[test]
fn extract_args_adds_pot_provider_when_configured() {
    let mut config = test_config();
    config.ytdlp_pot_provider_base_url = Some("http://127.0.0.1:4416".to_owned());
    let args = extract_args(&config, "https://example.com/video", false);
    assert!(args.contains(&"youtubepot-bgutilhttp:base_url=http://127.0.0.1:4416".to_owned()));
}

#[test]
fn search_args_builds_ytsearch_target() {
    let config = test_config();
    let args = search_args(&config, "some query", 5);
    assert_eq!(args.last().unwrap(), "ytsearch5:some query");
}

#[test]
fn search_args_uses_flat_playlist() {
    // Search only needs listing metadata to rank candidates — the full
    // per-video extraction (format resolution, headers, JS-challenge
    // solving) is deferred to resolve_stream() for whichever single track
    // actually gets played. See extraction::ytdlp::search_args' doc comment.
    let config = test_config();
    let args = search_args(&config, "some query", 5);
    assert!(args.contains(&"--flat-playlist".to_owned()));
}
