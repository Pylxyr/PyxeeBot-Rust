use std::sync::Arc;
use std::time::Duration;

use moka::future::Cache;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct ResolvedInfo {
    pub stream_url: String,
    pub acodec: String,
    pub abr: f64,

    pub headers: Vec<(String, String)>,

    pub content_length: Option<u64>,
}

// Cached as `Arc<T>` rather than `T`: moka clones the value out on every `get`,
// so caching the bare struct meant every cache *hit* — the hot path — paid for a
// deep clone of the headers/strings. Caching an `Arc` makes that clone O(1).
pub struct ResolveCache {
    inner: Cache<String, Arc<ResolvedInfo>>,
}

impl ResolveCache {
    pub fn new(config: &Config) -> Self {
        let inner = Cache::builder()
            .max_capacity(config.ytdlp_resolve_cache_size)
            .time_to_live(Duration::from_secs(config.ytdlp_resolve_cache_ttl_secs))
            .build();
        Self { inner }
    }

    pub async fn get(&self, webpage_url: &str) -> Option<Arc<ResolvedInfo>> {
        self.inner.get(webpage_url).await
    }

    pub async fn insert(&self, webpage_url: String, info: Arc<ResolvedInfo>) {
        self.inner.insert(webpage_url, info).await;
    }

    pub async fn invalidate(&self, webpage_url: &str) {
        self.inner.invalidate(webpage_url).await;
    }
}

pub struct SearchCache {
    inner: Cache<String, Arc<Vec<serde_json::Value>>>,
}

impl SearchCache {
    pub fn new(config: &Config) -> Self {
        let inner = Cache::builder()
            .max_capacity(config.ytdlp_search_cache_size)
            .time_to_live(Duration::from_secs(config.ytdlp_search_cache_ttl_secs))
            .build();
        Self { inner }
    }

    pub async fn get(&self, key: &str, min_count: usize) -> Option<Arc<Vec<serde_json::Value>>> {
        let cached = self.inner.get(key).await?;
        (cached.len() >= min_count).then_some(cached)
    }

    pub async fn insert(&self, key: String, entries: Arc<Vec<serde_json::Value>>) {
        self.inner.insert(key, entries).await;
    }
}
