//! Auto-condensation of conversation history when approaching context limits.
//!
//! Two-phase condensation pipeline:
//! 1. **Phase 1 (CPU):** Pre-compress the old half of conversation history
//!    using `CondenserEngine` (Profile::Heavy, ConversationHistory category).
//!    This reduces token count before the more expensive LLM summarization.
//! 2. **Phase 2 (LLM):** Summarize the pre-compressed old half via the
//!    centralized inference router, producing a structured summary.
//!
//! When `pre_compress` is false, Phase 1 is skipped — the raw old half is
//! fed directly to the LLM summarizer (the original behavior).
//!
//! The condenser fetches raw episodes, splits at the saliency midpoint,
//! summarizes the oldest half, and returns a rebuilt input with condensed +
//! recent conversation blocks.

use hkask_capability::DelegationToken;
use hkask_types::DataCategory;
use hkask_types::template::LLMParameters;

use super::types::TurnRequest;
use crate::memory::MemoryService;
use hkask_services_context::AgentService;

use super::service::ChatService;
use hkask_condenser::inference::SUMMARY_SYSTEM_PROMPT;

impl ChatService {
    /// Condense the oldest half of conversation history when approaching context limits.
    ///
    /// Two-phase condensation: CPU pre-compress (optional) → LLM summarize.
    /// Returns `None` on any failure (graceful degradation — caller falls back
    /// to uncondensed context).
    pub(super) async fn condense_history(
        ctx: &AgentService,
        req: &TurnRequest,
        token: &DelegationToken,
        base_input: &str,
    ) -> Option<String> {
        // Constants for condensation behavior (previously per-request fields).
        const SALIENCY_WINDOW: usize = 5;
        const PRE_COMPRESS: bool = true;

        // [NORMATIVE] Sovereignty gate (H3/P2): condensing reads episodic
        // (sovereign) history — only proceed when the owner has granted consent.
        if !MemoryService::has_memory_consent(ctx, &req.agent_webid, &DataCategory::EpisodicMemory)
        {
            return None;
        }
        let episodes = MemoryService::recall_raw_episodes(
            &req.episodic_storage,
            &req.agent_webid,
            token,
            (SALIENCY_WINDOW * 4).max(8),
        );
        if episodes.len() < 4 {
            return None;
        }

        let keep_count = (SALIENCY_WINDOW * 2).min(episodes.len().saturating_sub(2));
        let old_half = &episodes[..episodes.len() - keep_count];
        let recent_half = &episodes[episodes.len() - keep_count..];

        let recent_text = hkask_condenser::inference::format_conversation_text(recent_half);
        let old_text = hkask_condenser::inference::format_conversation_text(old_half);

        // Phase 1: CPU pre-compression (optional). Compress the old half with
        // CondenserEngine before feeding to the LLM summarizer. This reduces
        // token count and inference cost. If compression produces empty output,
        // fall back to the raw old_text (graceful degradation).
        let old_text_for_llm = if PRE_COMPRESS {
            let mut engine = hkask_condenser::engine::CondenserEngine::new();
            engine.set_profile(hkask_condenser::types::Profile::Heavy);
            let compressed = engine.compress(
                "condense_history",
                &old_text,
                Some(hkask_condenser::types::ContextCategory::ConversationHistory),
            );
            if compressed.content.is_empty() {
                old_text
            } else {
                tracing::debug!(
                    target: "reg.chat.condense",
                    agent = %req.userpod_name,
                    phase = "cpu_pre_compress",
                    original_bytes = compressed.original_bytes,
                    compressed_bytes = compressed.compressed_bytes,
                    algorithm = %compressed.algorithm,
                    "CPU pre-compression applied"
                );
                compressed.content
            }
        } else {
            old_text
        };

        // Phase 2: LLM summarization of the (pre-compressed) old half.
        let summary_prompt =
            hkask_condenser::inference::build_summarization_prompt(&old_text_for_llm, &req.input);

        let full_prompt = format!("{SUMMARY_SYSTEM_PROMPT}\n\nUser: {summary_prompt}");

        let condenser_model = &req.model;
        let params = LLMParameters {
            temperature: 0.3,
            top_p: 0.9,
            top_k: 40,
            min_p: 0.0,
            typical_p: 0.0,
            frequency_penalty: 0.0,
            presence_penalty: 0.0,
            max_tokens: 500,
            seed: None,
            disable_thinking: true,
            adapter: None,
            // Bypass fusion: thread summarization is a single-model call, not a
            // multi-model deliberation. Fusion orchestration adds latency and
            // cost without benefit for this straightforward extraction task.
            bypass_fusion: true,
            fusion_config: None,
            system_prompt: None,
        };

        let port = ctx.infra().inference.clone()?;
        let result = port
            .generate_with_model(&full_prompt, &params, Some(condenser_model), None)
            .await
            .ok()?;

        let summary = result.text;
        if summary.trim().is_empty() {
            return None;
        }

        tracing::debug!(
            target: "reg.chat.condense",
            agent = %req.userpod_name,
            old_msgs = old_half.len(),
            recent_msgs = recent_half.len(),
            summary_len = summary.len(),
            pre_compressed = PRE_COMPRESS,
            "History condensed"
        );

        Some(format!(
            "{base_input}\n\n[Condensed history]\n{summary}\n\n[Recent conversation]\n{recent_text}"
        ))
    }
}
