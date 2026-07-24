//! hKask MCP Web — In-memory TTL cache with LRU eviction
//!
//! Cache keys include a provider availability fingerprint so that results
//! reflect the current provider topology (not a stale snapshot from when
//! a different set of providers was available).

use std::collections::HashMap;
use std::time::Duration;
use tokio::sync::RwLock;

use super::types::MAX_CACHE_VALUE_BYTES;

#[derive(Clone)]
struct CacheEntry {
    data: serde_json::Value,
    inserted_at: std::time::Instant,
    last_accessed: std::time::Instant,
    ttl: Duration,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > self.ttl
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct CacheKey(pub String);

pub struct ResponseCache {
    entries: RwLock<HashMap<CacheKey, CacheEntry>>,
    max_entries: usize,
    default_ttl: Duration,
}

// N8 (2026-07-20): Eviction is O(n) — `insert` scans all entries to find
// the least-recently-accessed one. This is acceptable because `max_entries`
// is capped at MAX_CACHE_MAX_ENTRIES (200) via the constructor, so the scan
// is bounded at 200 iterations. A true O(1) LRU would require a
// LinkedHashMap or a doubly-linked-list + HashMap composite, which is not
// in std and would add a dependency. The current design is the path of
// least action for the bounded-size use case. If `max_entries` grows beyond
// ~1000, revisit this trade-off.

impl ResponseCache {
    pub fn new(max_entries: usize, default_ttl: Duration) -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            max_entries,
            default_ttl,
        }
    }

    /// Get a cached value, updating its last-accessed time for LRU eviction.
    pub async fn get(&self, key: &CacheKey) -> Option<serde_json::Value> {
        let mut entries = self.entries.write().await;
        let entry = entries.get_mut(key)?;
        if entry.is_expired() {
            return None;
        }
        entry.last_accessed = std::time::Instant::now();
        Some(entry.data.clone())
    }

    pub async fn insert(&self, key: CacheKey, data: serde_json::Value) {
        // Max-value-size guard: don't cache entries larger than MAX_CACHE_VALUE_BYTES
        // to prevent a single large response from dominating cache memory.
        if let Ok(bytes) = serde_json::to_string(&data)
            && bytes.len() > MAX_CACHE_VALUE_BYTES
        {
            tracing::warn!(
                size = bytes.len(),
                max = MAX_CACHE_VALUE_BYTES,
                "Cache value exceeds max size, skipping cache insert"
            );
            return;
        }

        let mut entries = self.entries.write().await;
        // Evict least recently accessed entry when at capacity
        if entries.len() >= self.max_entries
            && let Some(oldest_key) = entries
                .iter()
                .min_by_key(|(_, v)| v.last_accessed)
                .map(|(k, _)| k.clone())
        {
            entries.remove(&oldest_key);
        }
        let now = std::time::Instant::now();
        entries.insert(
            key,
            CacheEntry {
                data,
                inserted_at: now,
                last_accessed: now,
                ttl: self.default_ttl,
            },
        );
    }
}

/// Build a cache key that includes the provider fingerprint.
///
/// This ensures that cached results reflect the current provider topology.
/// If providers are added or removed, the cache key changes and a fresh
/// result is computed rather than serving a stale result from when a
/// different set of providers was available.
pub fn cache_key(
    strategy: &str,
    query: &str,
    params: &serde_json::Value,
    provider_fingerprint: &str,
) -> CacheKey {
    let hash = blake3::hash(
        format!(
            "{strategy}:{query}:{}:{provider_fingerprint}",
            serde_json::to_string(params).unwrap_or_default()
        )
        .as_bytes(),
    );
    CacheKey(hash.to_hex().to_string())
}
