//! TTL cache for market-data responses (T6).
//!
//! Plain in-process memoization with explicit expiry. A miss or expiry is a
//! refetch; the cache never synthesizes data. Testable via injected clock —
//! `new_with_clock` lets tests control time without sleeping.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct Entry {
    value: serde_json::Value,
    inserted: Instant,
}

pub struct TtlCache {
    ttl: Duration,
    entries: Mutex<HashMap<String, Entry>>,
    clock: fn() -> Instant,
}

impl TtlCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_secs),
            entries: Mutex::new(HashMap::new()),
            clock: Instant::now,
        }
    }

    #[cfg(test)]
    fn new_with_clock(ttl_secs: u64, clock: fn() -> Instant) -> Self {
        Self {
            ttl: Duration::from_secs(ttl_secs),
            entries: Mutex::new(HashMap::new()),
            clock,
        }
    }

    /// Return the cached value if present and fresh.
    pub fn get(&self, key: &str) -> Option<serde_json::Value> {
        let now = (self.clock)();
        let entries = self.entries.lock().unwrap_or_else(|e| e.into_inner());
        let entry = entries.get(key)?;
        if now.duration_since(entry.inserted) < self.ttl {
            Some(entry.value.clone())
        } else {
            None
        }
    }

    pub fn put(&self, key: &str, value: serde_json::Value) {
        let now = (self.clock)();
        self.entries
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                key.to_string(),
                Entry {
                    value,
                    inserted: now,
                },
            );
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static FAKE_NOW_SECS: AtomicU64 = AtomicU64::new(0);

    fn fake_now() -> Instant {
        // Anchor a real Instant and offset by the fake clock's seconds.
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_secs(FAKE_NOW_SECS.load(Ordering::SeqCst))
    }

    #[test]
    fn hit_within_ttl_and_miss_after_expiry() {
        FAKE_NOW_SECS.store(0, Ordering::SeqCst);
        let cache = TtlCache::new_with_clock(60, fake_now);
        cache.put("k", serde_json::json!({"v": 1}));
        assert!(cache.get("k").is_some(), "fresh entry hits");

        FAKE_NOW_SECS.store(30, Ordering::SeqCst);
        assert!(cache.get("k").is_some(), "within TTL still hits");

        FAKE_NOW_SECS.store(61, Ordering::SeqCst);
        assert!(cache.get("k").is_none(), "past TTL misses");
    }

    #[test]
    fn miss_on_unknown_key() {
        let cache = TtlCache::new_with_clock(60, fake_now);
        assert!(cache.get("absent").is_none());
        assert_eq!(cache.len(), 0);
    }
}
