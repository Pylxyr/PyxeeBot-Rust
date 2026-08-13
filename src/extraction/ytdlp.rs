use std::process::Stdio;
use std::time::Duration;

use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use crate::config::Config;
use crate::constants::{YTDLP_FORMAT, YTDLP_RETRY_FORMAT};
use crate::errors::{BotError, Result};

/// Caps a single yt-dlp invocation's stdout/stderr so a runaway process can't exhaust RAM.
const MAX_YTDLP_OUTPUT_BYTES: usize = 8 * 1024 * 1024;

/// Reads up to the cap, then keeps draining (discarding) so the child never blocks on a full pipe.
async fn read_capped<R: tokio::io::AsyncRead + Unpin>(mut reader: R) -> String {
    let mut buf = Vec::with_capacity(64 * 1024);
    let mut chunk = [0u8; 8192];
    loop {
        let n = match reader.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        if buf.len() < MAX_YTDLP_OUTPUT_BYTES {
            let remaining = MAX_YTDLP_OUTPUT_BYTES - buf.len();
            buf.extend_from_slice(&chunk[..n.min(remaining)]);
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Pure function (testable without spawning); `client_override` retries with a different player client.
pub fn extract_args(
    config: &Config,
    query_or_url: &str,
    flat_playlist: bool,
    client_override: Option<&str>,
) -> Vec<String> {
    let format = if client_override.is_some() {
        YTDLP_RETRY_FORMAT
    } else {
        YTDLP_FORMAT
    };
    let mut args = vec![
        "--dump-json".to_owned(),
        "--no-warnings".to_owned(),
        "--no-playlist".to_owned(),
        "--format".to_owned(),
        format.to_owned(),
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
    args.push("--cache-dir".to_owned());
    args.push(config.ytdlp_cache_dir.display().to_string());

    let mut youtube_args = Vec::new();
    if let Some(js_runtime) = &config.ytdlp_js_runtime_path {
        youtube_args.push(format!("jsi={js_runtime}"));
    }
    if let Some(client) = client_override {
        youtube_args.push(format!("player_client={client}"));
    }
    if !youtube_args.is_empty() {
        args.push("--extractor-args".to_owned());
        args.push(format!("youtube:{}", youtube_args.join(";")));
    }

    args.push(query_or_url.to_owned());
    args
}

/// `--flat-playlist`: ranking only needs the inline metadata, not full per-video extraction.
pub fn search_args(config: &Config, query: &str, count: usize) -> Vec<String> {
    let search_target = format!("ytsearch{count}:{query}");
    extract_args(config, &search_target, true, None)
}

/// Lists up to `limit` entries of a playlist URL; `--playlist-end` caps yt-dlp's own work.
pub fn extract_playlist_args(config: &Config, playlist_url: &str, limit: usize) -> Vec<String> {
    let mut args = vec![
        "--dump-json".to_owned(),
        "--no-warnings".to_owned(),
        "--flat-playlist".to_owned(),
        "--playlist-end".to_owned(),
        limit.max(1).to_string(),
        "--socket-timeout".to_owned(),
        config.ytdlp_socket_timeout.to_string(),
    ];
    if let Some(cookies) = &config.ytdlp_cookies_file {
        args.push("--cookies".to_owned());
        args.push(cookies.display().to_string());
    }
    args.push("--cache-dir".to_owned());
    args.push(config.ytdlp_cache_dir.display().to_string());
    if let Some(js_runtime) = &config.ytdlp_js_runtime_path {
        args.push("--extractor-args".to_owned());
        args.push(format!("youtube:jsi={js_runtime}"));
    }
    args.push(playlist_url.to_owned());
    args
}

/// Parses each stdout line as JSON — yt-dlp emits one line per result for search/playlist targets.
pub async fn run_ytdlp(config: &Config, args: &[String]) -> Result<Vec<Value>> {
    tracing::info!(cmd = %format!("yt-dlp {}", args.join(" ")), "run_ytdlp: spawning");
    let start = std::time::Instant::now();

    let mut child = Command::new("yt-dlp")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            tracing::error!(error = %e, "run_ytdlp: failed to spawn — is yt-dlp on PATH?");
            BotError::YtDlp(format!("failed to spawn yt-dlp: {e}"))
        })?;

    let mut stdout = child.stdout.take().expect("stdout was piped");
    let mut stderr = child.stderr.take().expect("stderr was piped");

    let read_fut = async {
        tokio::join!(read_capped(&mut stdout), read_capped(&mut stderr))
    };

    let timeout_secs = config.ytdlp_extract_timeout_secs;
    let (stdout_text, stderr_text) = match timeout(Duration::from_secs(timeout_secs), read_fut)
        .await
    {
        Ok(result) => result,
        Err(_) => {
            tracing::error!(elapsed = ?start.elapsed(), timeout_secs, "run_ytdlp: TIMED OUT — process will be killed (kill_on_drop)");
            return Err(BotError::YtDlp(format!(
                "yt-dlp timed out after {timeout_secs}s"
            )));
        }
    };

    let status = child
        .wait()
        .await
        .map_err(|e| BotError::YtDlp(format!("yt-dlp wait failed: {e}")))?;

    let elapsed = start.elapsed();
    if !stderr_text.trim().is_empty() {
        tracing::info!(elapsed = ?elapsed, status = %status, stderr = %stderr_text.trim(), "run_ytdlp: stderr output");
    }

    let entries: Vec<Value> = stdout_text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect();

    tracing::info!(elapsed = ?elapsed, status = %status, stdout_lines = stdout_text.lines().count(), parsed_entries = entries.len(), "run_ytdlp: finished");

    if entries.is_empty() && !status.success() {
        let trimmed = stderr_text.trim();
        let msg = if trimmed.is_empty() {
            format!("yt-dlp exited with {status}")
        } else {
            trimmed.to_owned()
        };
        tracing::error!(elapsed = ?elapsed, status = %status, "run_ytdlp: failed, no entries parsed");
        return Err(BotError::YtDlp(msg));
    }

    Ok(entries)
}
