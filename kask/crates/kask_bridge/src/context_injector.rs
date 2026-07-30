//! Context injector — retrieves salient memories and injects into prompts (D11).
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
//! ## Prompt-length gate
//!
//! Recall is gated on prompt length: prompts shorter than
//! `MIN_RECALL_PROMPT_LEN` characters or with fewer than `MIN_RECALL_PROMPT_WORDS`
//! words skip recall entirely. Short, code-focused prompts ("fix this", "run
//! the tests") are unlikely to benefit from past conversation history, and
//! skipping them avoids the embedding HTTP call + SQL queries that recall
//! would otherwise fire. This is a zero-cost gate — no SQL, no HTTP, no cache.

use agent::ContextInjector;
use hkask_types::MemoryPort;
use language_model::{LanguageModelRequestMessage, Role};
use language_model_core::MessageContent;
use std::sync::Arc;

use crate::memory::RealMemoryPort;

/// Minimum prompt length (characters) for recall to fire.
/// Prompts shorter than this skip recall entirely.
const MIN_RECALL_PROMPT_LEN: usize = 20;

/// Minimum word count for recall to fire.
/// Prompts with fewer words skip recall entirely.
const MIN_RECALL_PROMPT_WORDS: usize = 3;

/// Bridge context injector — retrieves memories and formats them for prompt injection.
pub struct BridgeContextInjector {
    memory_port: Arc<dyn MemoryPort>,
    recall_limit: u32,
    recall_min_confidence: f64,
}

impl BridgeContextInjector {
    /// Construct a new context injector.
    ///
    /// Reads `recall_limit` and `recall_min_confidence` from `KaskMemorySettings`
    /// at the composition root (which has access to `cx: &App`) and passes them
    /// here.
    pub fn new(
        memory_port: Arc<dyn MemoryPort>,
        recall_limit: u32,
        recall_min_confidence: f64,
    ) -> Self {
        Self {
            memory_port,
            recall_limit,
            recall_min_confidence,
        }
    }

    /// Check whether a prompt is long enough to warrant recall.
    /// Short prompts ("fix this", "run tests") are unlikely to benefit from
    /// memory recall and would waste an embedding HTTP call + SQL queries.
    fn should_recall(prompt: &str) -> bool {
        if prompt.len() < MIN_RECALL_PROMPT_LEN {
            return false;
        }
        let word_count = prompt.split_whitespace().count();
        word_count >= MIN_RECALL_PROMPT_WORDS
    }
}

impl ContextInjector for BridgeContextInjector {
    fn inject_context(
        &self,
        _thread_id: &str,
        user_prompt: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Vec<LanguageModelRequestMessage>> + Send + '_>,
    > {
        let limit = self.recall_limit as usize;
        let min_confidence = self.recall_min_confidence;
        let prompt = user_prompt.to_string();
        let memory_port = self.memory_port.clone();

        // Gate: skip recall for short prompts that won't benefit from it.
        // This avoids the embedding HTTP call + SQL queries for prompts like
        // "fix this", "run the tests", "what does this do".
        if !Self::should_recall(&prompt) {
            return Box::pin(async move { Vec::new() });
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

            vec![LanguageModelRequestMessage {
                role: Role::System,
                content: vec![MessageContent::Text(context_text)],
                cache: false,
                reasoning_details: None,
            }]
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
            let snippets = match memory_port.recall_thread(&thread_id, static_limit).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "Static context thread recall failed"
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

/// Bridge curator context injector — recalls from the curator's sovereign DB.
///
/// Mirrors `BridgeContextInjector` but delegates to
/// `RealMemoryPort::recall_context_curator` (per-turn, content-similarity) and
/// `RealMemoryPort::recall_thread_curator` (static, entity-scoped) instead of
/// the user-scoped `MemoryPort` methods, so the Curator recalls its own
/// episodic + semantic memory (stored in `agents/curator/pod.db`) rather than
/// the user's. This closes the curator memory loop: the Curator ingests its
/// own turns (D6, curator-perspective episodic) and recalls them here (D11,
/// curator-scoped recall), exactly parallel to the user agent's loop.
///
/// Wired in the composition root via `agent::set_curator_context_injector`.
pub struct BridgeCuratorContextInjector {
    memory_port: Arc<RealMemoryPort>,
    recall_limit: u32,
    recall_min_confidence: f64,
}

impl BridgeCuratorContextInjector {
    /// Construct a new curator context injector.
    ///
    /// Takes the same `RealMemoryPort` the user injector uses — the curator
    /// recall method (`recall_context_curator`) is on `RealMemoryPort` and
    /// reads from the `curator_episodic` / `curator_semantic` fields on that
    /// struct, which are opened alongside the user's stores in `RealMemoryPort::new`.
    pub fn new(
        memory_port: Arc<RealMemoryPort>,
        recall_limit: u32,
        recall_min_confidence: f64,
    ) -> Self {
        Self {
            memory_port,
            recall_limit,
            recall_min_confidence,
        }
    }

    /// Reuse the same prompt-length gate as the user injector — short prompts
    /// skip recall to avoid the embedding HTTP call + SQL queries.
    fn should_recall(prompt: &str) -> bool {
        if prompt.len() < MIN_RECALL_PROMPT_LEN {
            return false;
        }
        let word_count = prompt.split_whitespace().count();
        word_count >= MIN_RECALL_PROMPT_WORDS
    }
}

impl ContextInjector for BridgeCuratorContextInjector {
    fn inject_context(
        &self,
        _thread_id: &str,
        user_prompt: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Vec<LanguageModelRequestMessage>> + Send + '_>,
    > {
        let limit = self.recall_limit as usize;
        let min_confidence = self.recall_min_confidence;
        let prompt = user_prompt.to_string();
        let memory_port = self.memory_port.clone();

        if !Self::should_recall(&prompt) {
            return Box::pin(async move { Vec::new() });
        }

        Box::pin(async move {
            let snippets = match memory_port.recall_context_curator(&prompt, limit).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "Curator context injection recall failed"
                    );
                    return Vec::new();
                }
            };

            let filtered: Vec<_> = snippets
                .into_iter()
                .filter(|s| s.confidence >= min_confidence)
                .collect();

            if filtered.is_empty() {
                return Vec::new();
            }

            let mut context_text = String::from("Relevant context from curator memory:\n\n");
            for (i, snippet) in filtered.iter().enumerate() {
                if i > 0 {
                    context_text.push_str("\n---\n\n");
                }
                context_text.push_str(&snippet.text);
            }

            tracing::info!(
                target: "reg.memory",
                injected_count = filtered.len(),
                "Injecting curator memory context into prompt"
            );

            vec![LanguageModelRequestMessage {
                role: Role::System,
                content: vec![MessageContent::Text(context_text)],
                cache: false,
                reasoning_details: None,
            }]
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
            let snippets = match memory_port
                .recall_thread_curator(&thread_id, static_limit)
                .await
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(
                        target: "reg.memory",
                        error = %e,
                        "Curator static context thread recall failed"
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

            let mut context_text = String::from("Session curator memory context:\n\n");
            for (i, snippet) in filtered.iter().enumerate() {
                if i > 0 {
                    context_text.push_str("\n---\n\n");
                }
                context_text.push_str(&snippet.text);
            }

            tracing::info!(
                target: "reg.memory",
                injected_count = filtered.len(),
                "Injecting static curator memory context into system prompt"
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

    /// A mock MemoryPort that counts recall calls.
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

    /// Pin the prompt-length gate: short prompts skip recall entirely,
    /// avoiding the embedding HTTP call + SQL queries.
    #[tokio::test]
    async fn short_prompts_skip_recall() {
        let port = Arc::new(CountingMockPort::new());
        let port_handle = Arc::clone(&port);
        let injector = BridgeContextInjector::new(port as Arc<dyn MemoryPort>, 5, 0.0);

        // Short prompt — should NOT hit the MemoryPort.
        let result = injector.inject_context("t1", "fix this").await;
        assert!(result.is_empty(), "short prompt should return no messages");
        assert_eq!(
            port_handle.recall_count(),
            0,
            "short prompt should not hit the MemoryPort"
        );

        // Two-word prompt under the word threshold — should NOT hit.
        let result = injector.inject_context("t1", "run tests now").await;
        assert!(
            result.is_empty(),
            "two-word prompt should return no messages"
        );
        assert_eq!(
            port_handle.recall_count(),
            0,
            "two-word prompt should not hit the MemoryPort"
        );
    }

    /// Pin that long-enough prompts DO fire recall.
    #[tokio::test]
    async fn long_prompts_fire_recall() {
        let port = Arc::new(CountingMockPort::new());
        let port_handle = Arc::clone(&port);
        let injector = BridgeContextInjector::new(port as Arc<dyn MemoryPort>, 5, 0.0);

        // Long prompt — should hit the MemoryPort.
        let result = injector
            .inject_context("t1", "how do I set up the deployment pipeline?")
            .await;
        assert!(
            !result.is_empty(),
            "long prompt should return memory messages"
        );
        assert_eq!(
            port_handle.recall_count(),
            1,
            "long prompt should hit the MemoryPort"
        );
    }

    /// Pin the exact boundary: MIN_RECALL_PROMPT_LEN chars and MIN_RECALL_PROMPT_WORDS words.
    #[tokio::test]
    async fn boundary_prompt_fires_recall() {
        let port = Arc::new(CountingMockPort::new());
        let port_handle = Arc::clone(&port);
        let injector = BridgeContextInjector::new(port as Arc<dyn MemoryPort>, 5, 0.0);

        // Exactly at the boundary: >= 3 words, >= 20 chars.
        // "three words here exactly" = 24 chars, 4 words — clears both gates.
        let boundary_prompt = "three words here exactly";
        assert!(boundary_prompt.len() >= MIN_RECALL_PROMPT_LEN);
        assert!(boundary_prompt.split_whitespace().count() >= MIN_RECALL_PROMPT_WORDS);

        let result = injector.inject_context("t1", boundary_prompt).await;
        assert!(
            !result.is_empty(),
            "boundary prompt should return memory messages"
        );
        assert_eq!(
            port_handle.recall_count(),
            1,
            "boundary prompt should hit the MemoryPort"
        );
    }
}
