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

use agent::ContextInjector;
use hkask_types::MemoryPort;
use language_model::{LanguageModelRequestMessage, Role};
use language_model_core::MessageContent;
use std::sync::Arc;

/// Bridge context injector — retrieves memories and formats them for prompt injection.
pub struct BridgeContextInjector {
    memory_port: Arc<dyn MemoryPort>,
    recall_limit: u32,
    recall_min_confidence: f64,
}

impl BridgeContextInjector {
    /// Construct a new context injector.
    ///
    /// Reads `recall_limit` and `recall_min_confidence` from `KaskMemorySettings`.
    /// Construct a new context injector with explicit settings.
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
                content: vec![MessageContent::Text(context_text.into())],
                cache: false,
                reasoning_details: None,
            }]
        })
    }
}
