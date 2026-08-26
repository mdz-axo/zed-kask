//! QA generation pipeline — the shared prompt-formatting and result-writing
//! logic used by all three QA generation paths:
//!
//! 1. `corpus_generate_qa` (single chunk, synchronous)
//! 2. `corpus_generate_qa_batch` (multiple chunks, concurrent synchronous)
//! 3. `generate_qa_via_batch_api` (multiple chunks, provider Batch API)
//!
//! Extracted from `tools/semantic.rs` where the prompt formatting and result
//! envelope construction were duplicated 3×. A template change now touches
//! one file instead of three.

use serde_json::json;

use crate::tools::semantic::qa::{BatchQaPrompt, QaPair};
use crate::{render_docproc_template, CONTENT_GUARD_INSTRUCTION};

/// The LLM parameters used by all QA generation paths.
///
/// Single source of truth — previously duplicated in `corpus_generate_qa`
/// and the synchronous batch path with identical values.
pub(crate) fn qa_llm_parameters() -> hkask_types::template::LLMParameters {
    hkask_types::template::LLMParameters {
        temperature: 0.3,
        top_p: 0.95,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        top_k: 0,
        min_p: 0.0,
        typical_p: 0.0,
        thinking_allowed: false,
        ..Default::default()
    }
}

/// Default Bloom's taxonomy levels when none are specified.
pub(crate) fn default_bloom_levels() -> Vec<String> {
    vec!["factual".to_string(), "conceptual".to_string()]
}

/// The system prompt for batch API QA generation.
///
/// The synchronous paths embed this in the single prompt string; the batch
/// API path sends it as a separate system message.
pub(crate) const BATCH_SYSTEM_PROMPT: &str =
    "You are a training data generator. Generate ONE question-answer pair grounded in the passage's actual content.";

/// A formatted QA generation prompt.
pub(crate) struct FormattedQaPrompt {
    /// The full prompt text (template-rendered or inline fallback).
    pub text: String,
    /// The provenance identifier for the template used.
    pub template_source: &'static str,
}

/// Format a single-chunk QA generation prompt.
///
/// Renders the `generate-qa` docproc template with the chunk's levels, id,
/// and text. Falls back to an inline prompt when the template is unavailable.
/// The text is NOT content-guarded here — callers guard their own input
/// (the single-chunk path guards, the batch paths receive pre-composed text).
pub(crate) fn format_single_chunk_prompt(
    levels_str: &str,
    chunk_id: &str,
    text: &str,
) -> FormattedQaPrompt {
    let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    vars.insert("levels", levels_str.to_string());
    vars.insert("chunk_id", chunk_id.to_string());
    vars.insert("text", text.to_string());
    let tpl = render_docproc_template("generate-qa", &vars);
    if tpl.is_empty() {
        FormattedQaPrompt {
            text: format!(
                "{CONTENT_GUARD_INSTRUCTION}Based on the following text, generate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nText (chunk {chunk_id}):\n{text}\n\nFor each level, provide question, answer, and bloom_level.\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\"}}]}}",
            ),
            template_source: "inline-fallback",
        }
    } else {
        FormattedQaPrompt {
            text: tpl,
            template_source: "registry/templates/docproc/generate-qa.j2",
        }
    }
}

/// Format a cross-reference QA generation prompt (multi-passage synthesis).
///
/// Used only by `corpus_generate_qa` when `texts` is provided. Always inline
/// — there is no template for the cross-reference format.
pub(crate) fn format_cross_reference_prompt(
    levels_str: &str,
    chunk_id: &str,
    passages: &[String],
) -> FormattedQaPrompt {
    let mut text = String::new();
    for (i, p) in passages.iter().enumerate() {
        text.push_str(&format!("[Passage {}]\n{}\n\n", i + 1, crate::guard_content(p)));
    }
    FormattedQaPrompt {
        text: format!(
            "{CONTENT_GUARD_INSTRUCTION}You are synthesizing knowledge across {n} passages.\n\nGenerate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nThe questions should require synthesizing information from MULTIPLE passages — compare, contrast, diagnose patterns, or trace causal connections across sources.\n\nFor each QA, cite which passages support the answer (e.g., 'Per Passage 1, ... while Passage 2 notes ...').\n\nPassages (chunk group {chunk_id}):\n{text}\n\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\", \"sources\": [1, 3]}}]}}",
            n = passages.len(),
        ),
        template_source: "inline-cross-reference",
    }
}

/// Format a batch prompt's user text (for the Batch API path).
///
/// Same as [`format_single_chunk_prompt`] but without the content-guard
/// prefix — the batch API path composes system + user messages separately,
/// and the guard instruction lives in the system message.
pub(crate) fn format_batch_user_text(
    levels_str: &str,
    chunk_id: &str,
    text: &str,
) -> String {
    let mut vars: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    vars.insert("levels", levels_str.to_string());
    vars.insert("chunk_id", chunk_id.to_string());
    vars.insert("text", text.to_string());
    let tpl = render_docproc_template("generate-qa", &vars);
    if tpl.is_empty() {
        format!(
            "Based on the following text, generate question-answer pairs at these Bloom's taxonomy levels: {levels_str}.\n\nText (chunk {chunk_id}):\n{text}\n\nFor each level, provide question, answer, and bloom_level.\nRespond in JSON: {{\"qa_pairs\": [{{\"question\": \"...\", \"answer\": \"...\", \"bloom_level\": \"...\"}}]}}",
        )
    } else {
        tpl
    }
}

/// Build the QA result envelope for one QA pair.
///
/// The envelope format matches what `corpus_ingest_qa`'s `parse_qa_record`
/// expects: `chunk_ref`, `source`, `qa_type`, `response`, `provenance`,
/// `tokens_used`.
pub(crate) fn qa_result_envelope(
    prompt: &BatchQaPrompt,
    pair: QaPair,
    model: &str,
    template_source: &str,
    tokens_used: impl Into<u64>,
) -> serde_json::Value {
    json!({
        "chunk_ref": prompt.chunk_id,
        "source": prompt.source,
        "qa_type": pair.bloom_level,
        "response": {
            "instruction": pair.question,
            "output": pair.answer,
            "type": pair.bloom_level,
            "concepts": prompt.concepts,
        },
        "provenance": {
            "generator_model": model,
            "prompt_template": template_source,
            "source_chunk_ref": prompt.chunk_id,
        },
        "tokens_used": tokens_used.into(),
    })
}

/// Build the error envelope for a failed prompt.
pub(crate) fn qa_error_envelope(chunk_id: &str, error: &str) -> serde_json::Value {
    json!({
        "chunk_id": chunk_id,
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_llm_parameters_match_historical_values() {
        let params = qa_llm_parameters();
        assert_eq!(params.temperature, 0.3);
        assert_eq!(params.top_p, 0.95);
        assert!(!params.thinking_allowed);
    }

    #[test]
    fn default_bloom_levels_are_factual_and_conceptual() {
        let levels = default_bloom_levels();
        assert_eq!(levels, vec!["factual".to_string(), "conceptual".to_string()]);
    }

    #[test]
    fn format_single_chunk_prompt_falls_back_inline() {
        // The template registry is not available in tests (HKASK_TEMPLATE_ROOT
        // unset), so the inline fallback fires.
        let result = format_single_chunk_prompt("factual", "chunk-1", "some text");
        assert!(result.text.contains("some text"));
        assert!(result.text.contains("chunk-1"));
        assert!(result.text.contains("factual"));
    }

    #[test]
    fn format_batch_user_text_falls_back_inline() {
        let result = format_batch_user_text("factual", "chunk-1", "some text");
        assert!(result.contains("some text"));
        assert!(result.contains("chunk-1"));
    }

    #[test]
    fn format_cross_reference_prompt_includes_all_passages() {
        let passages = vec!["first".to_string(), "second".to_string()];
        let result = format_cross_reference_prompt("factual", "group-1", &passages);
        assert!(result.text.contains("[Passage 1]"));
        assert!(result.text.contains("[Passage 2]"));
        assert!(result.text.contains("first"));
        assert!(result.text.contains("second"));
        assert_eq!(result.template_source, "inline-cross-reference");
    }
}
