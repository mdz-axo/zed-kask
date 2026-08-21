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
}
