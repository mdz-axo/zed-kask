//! Pure formatting functions for thread summary — prompt building, text formatting,
//! token estimation, and output construction.
//!
//! Inference is handled by the centralized `InferencePort` (hkask-inference router).
//! This module contains only the testable pure logic with no HTTP or async.

use crate::types::ThreadSummaryOutput;

/// System prompt for LLM-assisted thread summarization.
///
/// Shared by the MCP server's `condenser_thread_summary` tool and the
/// ChatService's `condense_history` auto-condense path. Both callers
/// feed this as the system prompt for the inference request.
pub const SUMMARY_SYSTEM_PROMPT: &str = "You are a context condensation assistant. Produce structured summaries that \
     preserve technical details (file paths, error messages, decisions) while \
     eliminating verbosity. Use bullet points. Be concise.";

/// Format a parsed messages array into a conversation text string.
///
/// Each message becomes `[role]: content\n\n`. Messages missing `role` or
/// `content` fields use `"unknown"` and `""` respectively.
pub fn format_conversation_text(messages: &[serde_json::Value]) -> String {
    let mut text = String::new();
    for msg in messages {
        let role = msg
            .get("role")
            .and_then(|r| r.as_str())
            .unwrap_or("unknown");
        let content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
        text.push_str(&format!("[{role}]: {content}\n\n"));
    }
    text
}

/// Build the summarization prompt from conversation text and current query.
pub fn build_summarization_prompt(conversation_text: &str, current_query: &str) -> String {
    format!(
        "Summarize this conversation history for context compaction. \
         Preserve: key decisions, file paths mentioned, error states encountered, \
         code changes made, and the current task goal. \
         Discard: verbose tool output, intermediate file reads, repeated information, \
         and anything not directly relevant to the current task.\n\n\
         Current task: {current_query}\n\n\
         Conversation history:\n{conversation_text}"
    )
}

/// Build a `ThreadSummaryOutput` from extracted data.
pub fn build_summary_output(
    summary: String,
    original_text: &str,
    msg_count: usize,
    inference_model: String,
) -> ThreadSummaryOutput {
    ThreadSummaryOutput {
        original_tokens_approx: approx_token_count(original_text),
        summary_tokens_approx: approx_token_count(&summary),
        summary,
        original_message_count: msg_count,
        inference_model,
    }
}

/// Approximate token count using character-length heuristic.
///
/// Uses the standard ~4 characters per token rule of thumb for English text.
/// This is the same heuristic used by OpenAI's tiktoken and Anthropic's
/// Claude token estimator for rough context-window planning.
///
/// For accurate token counts, use a model-specific tokenizer. This function
/// provides a fast, dependency-free estimate suitable for condensation
/// threshold checks and context-window budgeting.
pub fn approx_token_count(text: &str) -> usize {
    // ~4 characters per token for English text. Floor at 1 to avoid zero
    // for very short inputs (empty string returns 1, which is harmless
    // for threshold comparisons).
    (text.len() / 4).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_count_uses_char_heuristic() {
        // 40 chars → ~10 tokens
        assert_eq!(
            approx_token_count("1234567890123456789012345678901234567890"),
            10
        );
        // 4 chars → 1 token
        assert_eq!(approx_token_count("test"), 1);
        // 7 chars → 1 token (floor)
        assert_eq!(approx_token_count("testing"), 1);
        // 8 chars → 2 tokens
        assert_eq!(approx_token_count("test test"), 2);
    }

    #[test]
    fn token_count_empty_floors_at_one() {
        assert_eq!(approx_token_count(""), 1);
    }

    #[test]
    fn token_count_scales_with_length() {
        let short = approx_token_count("hello world");
        let long = approx_token_count(&"x".repeat(400));
        assert!(long > short, "longer text should have higher token count");
        assert_eq!(long, 100); // 400 chars / 4 = 100
    }
}
