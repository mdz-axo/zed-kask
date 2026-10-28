//! Token-bucket per-tool rate limiting.
//!
//! Enforces a configurable number of requests per time window per tool name.
//! This is an external API boundary rate limiter — it protects the server
//! from external client DoS, distinct from internal energy budget tracking.
//! On rate limit, returns `WebError::RateLimited`.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::research::types::WebError;

pub struct RateLimiter {
    windows: Mutex<HashMap<String, RateWindow>>,
    max_requests: u32,
    window_secs: u64,
}

struct RateWindow {
    count: u32,
    expires_at: std::time::Instant,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            windows: Mutex::new(HashMap::new()),
            max_requests,
            window_secs,
        }
    }

    /// Check whether a request for the given tool is allowed.
    /// Returns Ok(()) if allowed, or `WebError::RateLimited` if exceeded.
    pub fn check(&self, tool_name: &str) -> Result<(), WebError> {
        let mut windows = self
            .windows
            .lock()
            .map_err(|_| WebError::ProviderError("rate limiter lock poisoned".to_string()))?;
        let now = std::time::Instant::now();
        let entry = windows.entry(tool_name.to_string()).or_insert(RateWindow {
            count: 0,
            expires_at: now + std::time::Duration::from_secs(self.window_secs),
        });
        if now >= entry.expires_at {
            entry.count = 0;
            entry.expires_at = now + std::time::Duration::from_secs(self.window_secs);
        }
        entry.count += 1;
        if entry.count > self.max_requests {
            Err(WebError::RateLimited(format!(
                "Rate limit exceeded for {tool_name}: {} requests per {}s",
                self.max_requests, self.window_secs
            )))
        } else {
            Ok(())
        }
    }
}
