use std::time::Duration;

use moka::future::Cache;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct ResolvedInfo {
    pub stream_url: String,
    pub acodec: String,
    pub abr: f64,
    /// Extra HTTP headers yt-dlp reports are needed to fetch `stream_url`
    /// (from `--dump-json`'s `http_headers` object).
    pub headers: Vec<(String, String)>,
    /// Upper bound on the resource's byte length, from yt-dlp's `filesize`
    /// or `filesize_approx`. Some CDNs — notably YouTube's — expect a
    /// bounded `range: bytes=0-N` request instead of an open-ended
    /// `range: bytes=0-`, and songbird's `HttpRequest` needs this value to
    /// build that bound.
    pub content_length: Option<u64>,
}

/// Caches resolved stream URLs keyed by webpage URL. TTL eviction is handled
/// by moka itself — no manual expiry bookkeeping needed, unlike the Python
/// resolver's hand-rolled dict + timestamp check.
pub struct ResolveCache {
    inner: Cache<String, ResolvedInfo>,
}

impl ResolveCache {
    pub fn new(config: &Config) -> Self {
        let inner = Cache::builder()
            .max_capacity(config.ytdlp_resolve_cache_size)
            .time_to_live(Duration::from_secs(config.ytdlp_resolve_cache_ttl_secs))
            .build();
        Self { inner }
    }

    pub async fn get(&self, webpage_url: &str) -> Option<ResolvedInfo> {
        self.inner.get(webpage_url).await
    }

    pub async fn insert(&self, webpage_url: String, info: ResolvedInfo) {
        self.inner.insert(webpage_url, info).await;
    }

    pub async fn invalidate(&self, webpage_url: &str) {
        self.inner.invalidate(webpage_url).await;
    }
}
