use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::config::Config;
use crate::constants::YTDLP_FORMAT;
use crate::errors::{BotError, Result};

/// Builds the yt-dlp argument list for extracting metadata (no download).
/// A pure function so it's testable without ever spawning a process.
pub fn extract_args(config: &Config, query_or_url: &str, flat_playlist: bool) -> Vec<String> {
    let mut args = vec![
        "--dump-json".to_owned(),
        "--no-warnings".to_owned(),
        "--no-playlist".to_owned(),
        "--format".to_owned(),
        YTDLP_FORMAT.to_owned(),
        "--socket-timeout".to_owned(),
        config.ytdlp_socket_timeout.to_string(),
    ];
    if flat_playlist {
        args.push("--flat-playlist".to_owned());
    }
    if let Some(cookies) = &config.ytdlp_cookies_file {
        args.push("--cookies".to_owned());
        args.push(cookies.display().to_string());
    }
    if let Some(js_runtime) = &config.ytdlp_js_runtime_path {
        args.push("--extractor-args".to_owned());
        args.push(format!("youtube:jsi={js_runtime}"));
    }
    args.push(query_or_url.to_owned());
    args
}

/// Builds the yt-dlp argument list for a `ytsearchN:` style text search.
pub fn search_args(config: &Config, query: &str, count: usize) -> Vec<String> {
    let search_target = format!("ytsearch{count}:{query}");
    extract_args(config, &search_target, false)
}

/// Runs yt-dlp with the given arguments and parses each stdout line as a JSON
/// object (yt-dlp emits one JSON object per line for `ytsearchN:` and
/// playlist-style targets, or a single line for a direct URL/query).
pub async fn run_ytdlp(config: &Config, args: &[String]) -> Result<Vec<Value>> {
    let mut child = Command::new("yt-dlp")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| BotError::YtDlp(format!("failed to spawn yt-dlp: {e}")))?;

    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");

    let read_fut = async {
        let mut out_buf = String::new();
        let mut err_buf = String::new();
        let (out_res, err_res) = tokio::join!(
            stdout.read_to_string(&mut out_buf),
            stderr.read_to_string(&mut err_buf),
        );
        out_res.ok();
        err_res.ok();
        (out_buf, err_buf)
    };

    let (stdout_text, stderr_text) = timeout(
        Duration::from_secs(config.ytdlp_extract_timeout_secs),
        read_fut,
    )
    .await
    .map_err(|_| BotError::YtDlp("yt-dlp timed out".to_owned()))?;

    let status = child
        .wait()
        .await
        .map_err(|e| BotError::YtDlp(format!("yt-dlp wait failed: {e}")))?;

    let entries: Vec<Value> = stdout_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();

    if entries.is_empty() && !status.success() {
        let trimmed = stderr_text.trim();
        let msg = if trimmed.is_empty() {
            format!("yt-dlp exited with {status}")
        } else {
            trimmed.to_owned()
        };
        return Err(BotError::YtDlp(msg));
    }

    Ok(entries)
}
