use std::time::Duration;

use moka::future::Cache;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct ResolvedInfo {
    pub stream_url: String,
    pub acodec: String,
    pub abr: f64,
    /// Extra HTTP headers yt-dlp reports are needed to fetch `stream_url`.
    pub headers: Vec<(String, String)>,
    /// Byte length, needed for songbird's `HttpRequest` on CDNs that require bounded byte-ranges.
    pub content_length: Option<u64>,
}

/// Resolved stream URLs keyed by webpage URL; moka handles TTL eviction itself.
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

/// Raw search entries by normalized query — a hit with enough entries counts, fewer is a miss.
pub struct SearchCache {
    inner: Cache<String, Vec<serde_json::Value>>,
}

impl SearchCache {
    pub fn new(config: &Config) -> Self {
        let inner = Cache::builder()
            .max_capacity(config.ytdlp_search_cache_size)
            .time_to_live(Duration::from_secs(config.ytdlp_search_cache_ttl_secs))
            .build();
        Self { inner }
    }

    pub async fn get(&self, key: &str, min_count: usize) -> Option<Vec<serde_json::Value>> {
        let cached = self.inner.get(key).await?;
        (cached.len() >= min_count).then_some(cached)
    }

    pub async fn insert(&self, key: String, entries: Vec<serde_json::Value>) {
        self.inner.insert(key, entries).await;
    }
}
