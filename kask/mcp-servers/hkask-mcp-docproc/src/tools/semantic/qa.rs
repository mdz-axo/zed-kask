//! QA generation helpers — response parsing, error types, batch writer.
//!
//! Used by `docproc_generate_qa` and `docproc_generate_qa_batch` in `mod.rs`.

use crate::*;
use serde::{Deserialize, Serialize};
use std::io::Write;

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct QaGenerationResponse {
    pub qa_pairs: Vec<QaPair>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct QaPair {
    pub question: String,
    pub answer: String,
    pub bloom_level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<usize>>,
}

/// Typed errors for QA response parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub enum QaParseError {
    #[error("QA response must be JSON with a qa_pairs array: {0}")]
    InvalidJson(String),
    #[error("QA response must contain at least one QA pair")]
    Empty,
    #[error("QA pair {index} must have non-empty question and answer")]
    EmptyField { index: usize },
    #[error("QA pair {index} has unsupported Bloom level '{level}'")]
    InvalidBloomLevel { index: usize, level: String },
    #[error("cross-reference QA pair {index} must cite at least one passage")]
    MissingCitation { index: usize },
    #[error("cross-reference QA pair {index} cites a passage outside 1..={passage_count}")]
    InvalidCitation { index: usize, passage_count: usize },
}

/// Parse model output into source-grounded QA pairs.
///
/// expect: "Generated QA data is safe to admit to the corpus only when it is complete and grounded."
/// [P4] Motivating: Clear Boundaries — the inference boundary rejects malformed or unsupported training data.
/// pre: response is JSON produced for the requested Bloom levels.
/// post: returns only non-empty pairs whose Bloom levels and cross-reference citations are valid.
/// inv: does not repair or silently reinterpret model output.
/// [P1] Constraining: User Sovereignty — provenance remains attached to generated training data.
pub(crate) fn parse_qa_response(
    response: &str,
    requested_levels: &[String],
    cross_reference_passage_count: Option<usize>,
) -> Result<QaGenerationResponse, QaParseError> {
    let parsed: QaGenerationResponse =
        serde_json::from_str(response).map_err(|e| QaParseError::InvalidJson(e.to_string()))?;

    if parsed.qa_pairs.is_empty() {
        return Err(QaParseError::Empty);
    }

    for (index, pair) in parsed.qa_pairs.iter().enumerate() {
        if pair.question.trim().is_empty() || pair.answer.trim().is_empty() {
            return Err(QaParseError::EmptyField { index });
        }
        if !requested_levels
            .iter()
            .any(|level| level == &pair.bloom_level)
        {
            return Err(QaParseError::InvalidBloomLevel {
                index,
                level: pair.bloom_level.clone(),
            });
        }
        if let Some(passage_count) = cross_reference_passage_count {
            if pair.sources.is_none() {
                return Err(QaParseError::MissingCitation { index });
            }
            if let Some(ref sources) = pair.sources {
                for &src in sources {
                    if src == 0 || src > passage_count {
                        return Err(QaParseError::InvalidCitation {
                            index,
                            passage_count,
                        });
                    }
                }
            }
        }
    }

    Ok(parsed)
}

/// Write a QA batch result as one JSONL line to the output file with
/// incremental flush every 10 completions for crash safety.
pub(crate) fn write_qa_result(
    result: &serde_json::Value,
    output_writer: &Arc<Mutex<std::io::BufWriter<std::fs::File>>>,
    write_count: &std::sync::atomic::AtomicUsize,
) {
    let mut w = output_writer.lock().unwrap();
    let _ = serde_json::to_writer(&mut *w, result);
    let _ = writeln!(&mut *w);
    let count = write_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if count.is_multiple_of(10) {
        let _ = w.flush();
    }
}

/// Resolve the QA model from request override, env, or settings default.
pub(crate) fn configured_qa_model(requested_model: Option<String>) -> Option<String> {
    if let Some(m) = requested_model {
        return Some(m);
    }
    std::env::var("HKASK_QA_MODEL")
        .ok()
        .or_else(|| std::env::var("HKASK_DEFAULT_MODEL").ok())
}

/// A single prompt spec parsed from prompts_jsonl for batch QA generation.
/// Internal to the batch tool — not part of the public request schema.
#[derive(Debug, Deserialize)]
pub(crate) struct BatchQaPrompt {
    pub text: String,
    pub chunk_id: String,
    pub bloom_levels: Option<Vec<String>>,
    pub source: String,
    pub concepts: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qa_response_rejects_missing_qa_pairs_array() {
        let result = parse_qa_response(
            r#"{"question":"What changed?"}"#,
            &["factual".to_string()],
            None,
        );

        assert!(
            result.is_err(),
            "responses without a qa_pairs array must be rejected"
        );
    }

    #[test]
    fn qa_response_rejects_unrequested_bloom_level() {
        let result = parse_qa_response(
            r#"{"qa_pairs":[{"question":"What changed?","answer":"A result changed.","bloom_level":"evaluate"}]}"#,
            &["factual".to_string()],
            None,
        );

        assert!(result.is_err(), "unrequested Bloom levels must be rejected");
    }

    #[test]
    fn cross_reference_qa_requires_valid_citations() {
        let result = parse_qa_response(
            r#"{"qa_pairs":[{"question":"How do they differ?","answer":"They differ.","bloom_level":"analyze","sources":[3]}]}"#,
            &["analyze".to_string()],
            Some(2),
        );

        assert!(
            result.is_err(),
            "citations outside the supplied passages must be rejected"
        );
    }

    #[test]
    fn qa_response_preserves_valid_pairs() {
        let parsed = parse_qa_response(
            r#"{"qa_pairs":[{"question":"What changed?","answer":"A result changed.","bloom_level":"factual","sources":[1]}]}"#,
            &["factual".to_string()],
            Some(1),
        )
        .expect("valid QA output should be accepted");

        assert_eq!(parsed.qa_pairs.len(), 1);
        assert_eq!(parsed.qa_pairs[0].sources.as_deref(), Some(&[1][..]));
    }

    #[test]
    fn configured_qa_model_returns_override() {
        let model = configured_qa_model(Some("OR/openai/gpt-5.6-terra".to_string()));
        assert_eq!(model.as_deref(), Some("OR/openai/gpt-5.6-terra"));
    }
}
