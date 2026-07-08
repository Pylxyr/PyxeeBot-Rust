mod cache;
mod ytdlp;

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::Semaphore;

use crate::config::Config;
use crate::errors::{BotError, Result};
use crate::models::Track;
use crate::scoring;

pub use cache::{ResolveCache, ResolvedInfo};
pub use ytdlp::{extract_args, search_args};

pub struct Extractor {
    config: Arc<Config>,
    cache: ResolveCache,
    semaphore: Semaphore,
}

impl Extractor {
    pub fn new(config: Arc<Config>) -> Self {
        let cache = ResolveCache::new(&config);
        let semaphore = Semaphore::new(config.ytdlp_concurrent_extracts);
        Self {
            config,
            cache,
            semaphore,
        }
    }

    /// Runs a `ytsearchN:` query, scores results, and returns tracks best
    /// match first.
    pub async fn search(
        &self,
        query: &str,
        requester_id: u64,
        curation_mode: bool,
    ) -> Result<Vec<Track>> {
        let count = self.config.ytdlp_search_results.max(1);
        let args = ytdlp::search_args(&self.config, query, count);
        let entries = self.run(&args).await?;
        let ranked = scoring::rank_entries(query, entries, curation_mode);
        Ok(ranked
            .iter()
            .map(|(item, _)| track_from_json(item, requester_id, query))
            .collect())
    }

    /// Same as `search`, but also returns each track's score breakdown —
    /// used by `!search`/`!why` to explain ranking decisions. Kept separate
    /// from `search` so existing callers aren't affected.
    pub async fn search_with_debug(
        &self,
        query: &str,
        requester_id: u64,
        curation_mode: bool,
    ) -> Result<Vec<(Track, scoring::ScoreBreakdown)>> {
        let count = self.config.ytdlp_search_results.max(1);
        let args = ytdlp::search_args(&self.config, query, count);
        let entries = self.run(&args).await?;
        let ranked = scoring::rank_entries(query, entries, curation_mode);
        Ok(ranked
            .iter()
            .map(|(item, bd)| (track_from_json(item, requester_id, query), bd.clone()))
            .collect())
    }

    /// Extracts metadata for a direct URL (no search/ranking involved).
    /// `flat_playlist` set true also picks up every entry of a playlist URL.
    pub async fn extract_url(
        &self,
        url: &str,
        requester_id: u64,
        flat_playlist: bool,
    ) -> Result<Vec<Track>> {
        let args = ytdlp::extract_args(&self.config, url, flat_playlist);
        let entries = self.run(&args).await?;
        Ok(entries
            .iter()
            .map(|item| track_from_json(item, requester_id, url))
            .collect())
    }

    /// Resolves (or returns cached) the direct audio stream URL for a track.
    pub async fn resolve_stream(&self, track: &Track) -> Result<ResolvedInfo> {
        if let Some(cached) = self.cache.get(&track.webpage_url).await {
            tracing::info!(url = %track.webpage_url, "resolve_stream: cache hit");
            return Ok(cached);
        }
        tracing::info!(url = %track.webpage_url, "resolve_stream: cache miss, extracting");
        let args = ytdlp::extract_args(&self.config, &track.webpage_url, false);
        let entries = self.run(&args).await?;
        let item = entries
            .into_iter()
            .next()
            .ok_or_else(|| BotError::NoResult(track.webpage_url.clone()))?;
        let info = resolved_info_from_json(&item)?;
        self.cache
            .insert(track.webpage_url.clone(), info.clone())
            .await;
        Ok(info)
    }

    pub async fn invalidate_stream(&self, webpage_url: &str) {
        self.cache.invalidate(webpage_url).await;
    }

    async fn run(&self, args: &[String]) -> Result<Vec<Value>> {
        let queue_start = std::time::Instant::now();
        let _permit = self
            .semaphore
            .acquire()
            .await
            .expect("semaphore is never closed");
        let wait = queue_start.elapsed();
        if wait.as_millis() > 50 {
            tracing::info!(waited = ?wait, "extraction: waited for a free yt-dlp slot (YTDLP_CONCURRENT_EXTRACTS may be too low)");
        }
        ytdlp::run_ytdlp(&self.config, args).await
    }
}

fn value_str<'a>(item: &'a Value, key: &str) -> Option<&'a str> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn track_from_json(item: &Value, requester_id: u64, query: &str) -> Track {
    let webpage_url = value_str(item, "webpage_url")
        .or_else(|| value_str(item, "url"))
        .or_else(|| value_str(item, "original_url"))
        .unwrap_or(query)
        .to_owned();

    let uploader = value_str(item, "uploader")
        .or_else(|| value_str(item, "channel"))
        .or_else(|| value_str(item, "artist"))
        .or_else(|| value_str(item, "creator"))
        .unwrap_or("")
        .to_owned();

    let tags: Vec<String> = item
        .get("tags")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();

    Track {
        title: value_str(item, "title").unwrap_or(&webpage_url).to_owned(),
        webpage_url,
        uploader,
        duration: item.get("duration").and_then(Value::as_i64).unwrap_or(0),
        requester_id,
        query: query.to_owned(),
        thumbnail_url: value_str(item, "thumbnail").unwrap_or("").to_owned(),
        tags,
        acodec: value_str(item, "acodec").unwrap_or("").to_owned(),
        abr: item.get("abr").and_then(Value::as_f64).unwrap_or(0.0),
    }
}

fn resolved_info_from_json(item: &Value) -> Result<ResolvedInfo> {
    let stream_url = value_str(item, "url")
        .ok_or_else(|| BotError::YtDlp("yt-dlp response had no playable url".to_owned()))?
        .to_owned();
    Ok(ResolvedInfo {
        stream_url,
        acodec: value_str(item, "acodec").unwrap_or("").to_owned(),
        abr: item.get("abr").and_then(Value::as_f64).unwrap_or(0.0),
    })
}
