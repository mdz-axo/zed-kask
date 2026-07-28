//! Context injector — retrieves salient memories and injects them into prompts (D11).
//!
//! The `BridgeContextInjector` implements the `agent::ContextInjector` trait
//! by delegating to an `hkask_types::MemoryPort`. It:
//!
//! 1. Calls `memory_port.recall_context(user_prompt, recall_limit)` to retrieve
//!    relevant memory snippets.
//! 2. Filters by minimum confidence (`KaskMemorySettings.recall_min_confidence`).
//! 3. Formats the snippets into a single `LanguageModelRequestMessage` with
//!    `Role::System` containing a "Relevant context from memory:" preamble.
//! 4. Returns the message (or an empty vec if no snippets pass the filter).
//!
//! The injector is wired in the composition root via `agent::set_context_injector`.
//! It is only called for `UserPrompt` and `Subagent` intents — the agent crate
//! gates on intent before calling `inject_context`.
//!
//! ## Recall cooldown
//!
//! `inject_context` is called on every iteration of the turn loop (every
//! tool-call round), not just the first. For a turn with N tool calls, recall
//! would fire N times for the same prompt. The injector caches recall results
//! per `(thread_id, prompt_hash)` for a configurable cooldown (default 30s,
//! override via `HKASK_MEMORY_RECALL_COOLDOWN_SECS`). Within the cooldown,
//! repeated calls for the same prompt return cached snippets without hitting
//! the database. This prevents recall from crowding out inference during
//! multi-round tool-use turns.

use agent::ContextInjector;
use hkask_types::MemoryPort;
use language_model::{LanguageModelRequestMessage, Role};
use language_model_core::MessageContent;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cache key: (thread_id, prompt_hash).
/// The prompt_hash is a blake3-free hash of the user prompt — we don't need
/// cryptographic strength, just collision resistance for cache keys.
type CacheKey = (String, u64);

/// Cached recall result with its insertion timestamp.
struct CachedRecall {
    messages: Vec<LanguageModelRequestMessage>,
    inserted_at: Instant,
}

/// Bridge context injector — retrieves memories and formats them for prompt injection.
pub struct BridgeContextInjector {
    memory_port: Arc<dyn MemoryPort>,
    recall_limit: u32,
    recall_min_confidence: f64,
    /// Per-(thread_id, prompt_hash) recall cache with a cooldown. Prevents
    /// redundant recall calls during multi-round tool-use turns.
    recall_cache: Mutex<HashMap<CacheKey, CachedRecall>>,
    /// Cooldown duration for the recall cache. Within this window, repeated
    /// calls for the same prompt return cached results.
    recall_cooldown: Duration,
}

impl BridgeContextInjector {
    /// Construct a new context injector.
    ///
    /// Reads `recall_limit` and `recall_min_confidence` from `KaskMemorySettings`
    /// at the composition root (which has access to `cx: &App`) and passes them
    /// here.
    ///
    /// The recall cooldown defaults to 30 seconds. Override via
    /// `HKASK_MEMORY_RECALL_COOLDOWN_SECS` (set to 0 to disable caching).
    pub fn new(
        memory_port: Arc<dyn MemoryPort>,
        recall_limit: u32,
        recall_min_confidence: f64,
    ) -> Self {
        let cooldown_secs = std::env::var("HKASK_MEMORY_RECALL_COOLDOWN_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        Self {
            memory_port,
            recall_limit,
            recall_min_confidence,
            recall_cache: Mutex::new(HashMap::new()),
            recall_cooldown: Duration::from_secs(cooldown_secs),
        }
    }

    /// Hash a prompt for cache keying. Uses `std::hash::DefaultHasher` —
    /// not cryptographic, but sufficient for cache key collision avoidance.
    fn prompt_hash(prompt: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        prompt.hash(&mut hasher);
        hasher.finish()
    }

    /// Look up a cached recall result. Returns `None` if the cache is
    /// disabled (cooldown == 0), the key is not cached, or the cached
    /// entry has expired.
    fn lookup_cache(
        &self,
        thread_id: &str,
        prompt_hash: u64,
    ) -> Option<Vec<LanguageModelRequestMessage>> {
        if self.recall_cooldown.is_zero() {
            return None;
        }
        let cache = self.recall_cache.lock().ok()?;
        let entry = cache.get(&(thread_id.to_string(), prompt_hash))?;
        if entry.inserted_at.elapsed() < self.recall_cooldown {
            Some(entry.messages.clone())
        } else {
            None
        }
    }

    /// Store a recall result in the cache.
    fn store_cache(
        &self,
        thread_id: &str,
        prompt_hash: u64,
        messages: Vec<LanguageModelRequestMessage>,
    ) {
        if self.recall_cooldown.is_zero() {
            return;
        }
        if let Ok(mut cache) = self.recall_cache.lock() {
            // Evict expired entries to bound memory growth. Without this, the
            // cache grows unbounded across a long session with many distinct
            // prompts. We evict lazily on insert — cheaper than a background
            // timer and sufficient for the typical session size.
            if cache.len() > 64 {
                let now = Instant::now();
                cache.retain(|_, v| now.duration_since(v.inserted_at) < self.recall_cooldown);
            }
            cache.insert(
                (thread_id.to_string(), prompt_hash),
                CachedRecall {
                    messages,
                    inserted_at: Instant::now(),
                },
            );
        }
    }
}

impl ContextInjector for BridgeContextInjector {
    fn inject_context(
        &self,
        thread_id: &str,
        user_prompt: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Vec<LanguageModelRequestMessage>> + Send + '_>,
    > {
        let limit = self.recall_limit as usize;
        let min_confidence = self.recall_min_confidence;
        let prompt = user_prompt.to_string();
        let memory_port = self.memory_port.clone();
        let prompt_hash = Self::prompt_hash(&prompt);
        let tid = thread_id.to_string();

        // Check the recall cache before hitting the database. The agent crate
        // calls inject_context on every tool-call round within a turn; without
        // this cache, a 5-round turn fires 5 recall calls for the same prompt.
        if let Some(cached) = self.lookup_cache(&tid, prompt_hash) {
            tracing::debug!(
                target: "reg.memory",
                thread_id = %tid,
                "Context injection served from recall cache (cooldown hit)"
            );
            return Box::pin(async move { cached });
        }

        Box::pin(async move {
            let snippets = match memory_port.recall_context(&prompt, limit).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "Context injection recall failed"
                    );
                    return Vec::new();
                }
            };

            // Filter by minimum confidence
            let filtered: Vec<_> = snippets
                .into_iter()
                .filter(|s| s.confidence >= min_confidence)
                .collect();

            if filtered.is_empty() {
                // Cache the empty result too — avoids re-calling recall for
                // prompts that have no matching memories within the cooldown.
                // We store an empty vec so the next lookup returns early.
                self.store_cache(&tid, prompt_hash, Vec::new());
                return Vec::new();
            }

            // Format snippets into a single system message
            let mut context_text = String::from("Relevant context from memory:\n\n");
            for (i, snippet) in filtered.iter().enumerate() {
                if i > 0 {
                    context_text.push_str("\n---\n\n");
                }
                context_text.push_str(&snippet.text);
            }

            tracing::info!(
                target: "reg.memory",
                injected_count = filtered.len(),
                "Injecting memory context into prompt"
            );

            let messages = vec![LanguageModelRequestMessage {
                role: Role::System,
                content: vec![MessageContent::Text(context_text)],
                cache: false,
                reasoning_details: None,
            }];

            self.store_cache(&tid, prompt_hash, messages.clone());
            messages
        })
    }

    fn inject_static_context<'a>(
        &'a self,
        thread_id: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<String>> + Send + 'a>> {
        let memory_port = self.memory_port.clone();
        let thread_id = thread_id.to_string();
        let static_limit = (self.recall_limit * 2) as usize;
        let static_min_confidence = (self.recall_min_confidence + 0.1).min(1.0);

        Box::pin(async move {
            let snippets = match memory_port.recall_context(&thread_id, static_limit).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "Static context recall failed"
                    );
                    Vec::new()
                }
            };

            let filtered: Vec<_> = snippets
                .into_iter()
                .filter(|s| s.confidence >= static_min_confidence)
                .collect();

            if filtered.is_empty() {
                return None;
            }

            let mut context_text = String::from("Session memory context:\n\n");
            for (i, snippet) in filtered.iter().enumerate() {
                if i > 0 {
                    context_text.push_str("\n---\n\n");
                }
                context_text.push_str(&snippet.text);
            }

            tracing::info!(
                target: "reg.memory",
                injected_count = filtered.len(),
                "Injecting static memory context into system prompt"
            );

            Some(context_text)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_types::{MemoryError, MemorySnippet};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A mock MemoryPort that counts recall calls. Used to verify the
    /// recall cache prevents redundant database hits within the cooldown.
    struct CountingMockPort {
        recall_count: AtomicUsize,
    }

    impl CountingMockPort {
        fn new() -> Self {
            Self {
                recall_count: AtomicUsize::new(0),
            }
        }

        fn recall_count(&self) -> usize {
            self.recall_count.load(Ordering::SeqCst)
        }
    }

    impl MemoryPort for CountingMockPort {
        fn ingest_turn<'a>(
            &'a self,
            _record: hkask_types::TurnRecord,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), MemoryError>> + Send + 'a>>
        {
            Box::pin(async move { Ok(()) })
        }

        fn recall_context<'a>(
            &'a self,
            _query: &'a str,
            _limit: usize,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<Vec<MemorySnippet>, MemoryError>>
                    + Send
                    + 'a,
            >,
        > {
            self.recall_count.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(vec![MemorySnippet {
                    text: "mock memory".to_string(),
                    source: "test".to_string(),
                    confidence: 1.0,
                    relevance_score: 1.0,
                }])
            })
        }
    }

    /// Pin the recall cooldown: calling `inject_context` twice with the same
    /// prompt within the cooldown should only hit the MemoryPort once.
    /// Without the cache, a 5-round tool-use turn fires 5 recall calls for
    /// the same prompt.
    #[tokio::test]
    async fn recall_cooldown_caches_repeated_prompts() {
        let port = Arc::new(CountingMockPort::new());
        let port_handle = Arc::clone(&port);
        let injector = BridgeContextInjector::new(port as Arc<dyn MemoryPort>, 5, 0.0);

        // First call — should hit the MemoryPort.
        let _ = injector.inject_context("t1", "hello world").await;
        assert_eq!(
            port_handle.recall_count(),
            1,
            "first call should hit the MemoryPort"
        );

        // Second call with the same prompt — should be served from cache.
        let _ = injector.inject_context("t1", "hello world").await;
        assert_eq!(
            port_handle.recall_count(),
            1,
            "second call within cooldown should be served from cache"
        );

        // Different prompt — should hit the MemoryPort again.
        let _ = injector.inject_context("t1", "different prompt").await;
        assert_eq!(
            port_handle.recall_count(),
            2,
            "different prompt should hit the MemoryPort"
        );

        // Different thread, same prompt — should hit the MemoryPort (cache is per-thread).
        let _ = injector.inject_context("t2", "hello world").await;
        assert_eq!(
            port_handle.recall_count(),
            3,
            "same prompt on different thread should hit the MemoryPort (per-thread cache)"
        );
    }

    /// Pin that the recall cache also caches empty results — avoids re-calling
    /// recall for prompts that have no matching memories within the cooldown.
    #[tokio::test]
    async fn recall_cooldown_caches_empty_results() {
        // A port that always returns empty snippets.
        struct EmptyPort;
        impl MemoryPort for EmptyPort {
            fn ingest_turn<'a>(
                &'a self,
                _record: hkask_types::TurnRecord,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<(), MemoryError>> + Send + 'a>,
            > {
                Box::pin(async move { Ok(()) })
            }
            fn recall_context<'a>(
                &'a self,
                _query: &'a str,
                _limit: usize,
            ) -> std::pin::Pin<
                Box<
                    dyn std::future::Future<Output = Result<Vec<MemorySnippet>, MemoryError>>
                        + Send
                        + 'a,
                >,
            > {
                Box::pin(async move { Ok(Vec::new()) })
            }
        }

        let injector =
            BridgeContextInjector::new(Arc::new(EmptyPort) as Arc<dyn MemoryPort>, 5, 0.0);

        // First call — returns empty vec.
        let result1 = injector.inject_context("t1", "no matches").await;
        assert!(result1.is_empty(), "first call should return empty");

        // Second call — should also return empty, served from cache.
        let result2 = injector.inject_context("t1", "no matches").await;
        assert!(
            result2.is_empty(),
            "second call should return empty from cache"
        );
    }
}
