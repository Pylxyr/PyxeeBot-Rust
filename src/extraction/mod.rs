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
    /// Full per-video extraction: CPU-heavy, kept tight (default 1).
    extract_semaphore: Semaphore,
    /// `--flat-playlist` search listings: lighter, separate budget.
    search_semaphore: Semaphore,
}

impl Extractor {
    pub fn new(config: Arc<Config>) -> Self {
        let cache = ResolveCache::new(&config);
        let extract_semaphore = Semaphore::new(config.ytdlp_concurrent_extracts);
        let search_semaphore = Semaphore::new(config.ytdlp_curation_concurrency);
        Self {
            config,
            cache,
            extract_semaphore,
            search_semaphore,
        }
    }

    /// Flat-playlist search: metadata listing only, then ranks results.
    pub async fn search(
        &self,
        query: &str,
        requester_id: u64,
        curation_mode: bool,
    ) -> Result<Vec<Track>> {
        let count = self.config.ytdlp_search_results.max(1);
        let args = ytdlp::search_args(&self.config, query, count);
        let entries = self.run_search(&args).await?;
        let ranked = scoring::rank_entries(query, entries, curation_mode);
        let mut tracks = Vec::with_capacity(ranked.len());
        for (item, _) in &ranked {
            let track = track_from_json(item, requester_id, query);
            self.prime_cache(&track.webpage_url, item).await;
            tracks.push(track);
        }
        Ok(tracks)
    }

    /// Same as `search` but returns score breakdowns and takes an explicit count.
    pub async fn search_with_debug(
        &self,
        query: &str,
        requester_id: u64,
        curation_mode: bool,
        count: usize,
    ) -> Result<Vec<(Track, scoring::ScoreBreakdown)>> {
        let count = count.max(1);
        let args = ytdlp::search_args(&self.config, query, count);
        let entries = self.run_search(&args).await?;
        let ranked = scoring::rank_entries(query, entries, curation_mode);
        let mut out = Vec::with_capacity(ranked.len());
        for (item, bd) in &ranked {
            let track = track_from_json(item, requester_id, query);
            self.prime_cache(&track.webpage_url, item).await;
            out.push((track, bd.clone()));
        }
        Ok(out)
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
        let mut tracks = Vec::with_capacity(entries.len());
        for item in &entries {
            let track = track_from_json(item, requester_id, url);
            self.prime_cache(&track.webpage_url, item).await;
            tracks.push(track);
        }
        Ok(tracks)
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

    /// Non-blocking sibling of `resolve_stream` for prefetch: `None` if no
    /// permit is free right now, instead of queuing behind urgent resolves.
    pub async fn try_resolve_stream(&self, track: &Track) -> Option<Result<ResolvedInfo>> {
        if let Some(cached) = self.cache.get(&track.webpage_url).await {
            return Some(Ok(cached));
        }
        let _permit = self.extract_semaphore.try_acquire().ok()?;
        let args = ytdlp::extract_args(&self.config, &track.webpage_url, false);
        let entries = match ytdlp::run_ytdlp(&self.config, &args).await {
            Ok(e) => e,
            Err(e) => return Some(Err(e)),
        };
        let Some(item) = entries.into_iter().next() else {
            return Some(Err(BotError::NoResult(track.webpage_url.clone())));
        };
        let info = match resolved_info_from_json(&item) {
            Ok(i) => i,
            Err(e) => return Some(Err(e)),
        };
        self.cache
            .insert(track.webpage_url.clone(), info.clone())
            .await;
        Some(Ok(info))
    }

    pub async fn invalidate_stream(&self, webpage_url: &str) {
        self.cache.invalidate(webpage_url).await;
    }

    /// Caches resolve info from an already-fully-extracted entry, if it has
    /// one (flat-playlist entries don't and are skipped).
    async fn prime_cache(&self, webpage_url: &str, item: &Value) {
        if item.get("http_headers").is_none() {
            return;
        }
        if let Ok(info) = resolved_info_from_json(item) {
            self.cache.insert(webpage_url.to_owned(), info).await;
        }
    }

    async fn run(&self, args: &[String]) -> Result<Vec<Value>> {
        let queue_start = std::time::Instant::now();
        let _permit = self
            .extract_semaphore
            .acquire()
            .await
            .expect("semaphore is never closed");
        let wait = queue_start.elapsed();
        if wait.as_millis() > 50 {
            tracing::info!(waited = ?wait, "extraction: waited for a free yt-dlp slot (YTDLP_CONCURRENT_EXTRACTS may be too low)");
        }
        ytdlp::run_ytdlp(&self.config, args).await
    }

    async fn run_search(&self, args: &[String]) -> Result<Vec<Value>> {
        let queue_start = std::time::Instant::now();
        let _permit = self
            .search_semaphore
            .acquire()
            .await
            .expect("semaphore is never closed");
        let wait = queue_start.elapsed();
        if wait.as_millis() > 50 {
            tracing::info!(waited = ?wait, "extraction: waited for a free yt-dlp search slot (YTDLP_CURATION_CONCURRENCY may be too low)");
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
        headers: headers_from_json(item),
        content_length: item
            .get("filesize")
            .and_then(Value::as_u64)
            .or_else(|| item.get("filesize_approx").and_then(Value::as_u64)),
    })
}

/// Pulls yt-dlp's `http_headers` object (from `--dump-json`) into a plain
/// list of pairs. Some CDNs (YouTube included) reject the stream without
/// these — e.g. a matching `User-Agent` — so they need to ride along with
/// the resolved URL into songbird's `HttpRequest`.
fn headers_from_json(item: &Value) -> Vec<(String, String)> {
    item.get("http_headers")
        .and_then(Value::as_object)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|(name, value)| {
                    value.as_str().map(|value| (name.clone(), value.to_owned()))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_info_picks_up_headers_and_filesize() {
        let item = serde_json::json!({
            "url": "https://example.com/stream",
            "acodec": "opus",
            "abr": 128.0,
            "filesize": 1234,
            "http_headers": {
                "User-Agent": "yt-dlp",
                "Referer": "https://youtube.com/",
            },
        });

        let resolved = resolved_info_from_json(&item).expect("valid item resolves");

        assert_eq!(resolved.content_length, Some(1234));
        assert_eq!(resolved.headers.len(), 2);
        assert!(resolved
            .headers
            .iter()
            .any(|(k, v)| k == "User-Agent" && v == "yt-dlp"));
    }

    #[test]
    fn resolved_info_falls_back_to_filesize_approx() {
        let item = serde_json::json!({
            "url": "https://example.com/stream",
            "filesize_approx": 5678,
        });

        let resolved = resolved_info_from_json(&item).expect("valid item resolves");

        assert_eq!(resolved.content_length, Some(5678));
        assert!(resolved.headers.is_empty());
    }

    #[test]
    fn resolved_info_missing_url_is_an_error() {
        let item = serde_json::json!({ "acodec": "opus" });
        assert!(resolved_info_from_json(&item).is_err());
    }
}
