//! QA generation helpers — response parsing, error types, model resolution.
//!
//! Used by `corpus_generate_qa` and `corpus_generate_qa_batch` in `semantic.rs`.

use serde::{Deserialize, Serialize};

/// Preserve configuration failures at the QA tool boundary instead of labelling
/// every failure a retryable provider outage.
pub(crate) fn map_qa_inference_error(error: hkask_types::InferenceError) -> crate::McpToolError {
    use hkask_types::InferenceError;
    let message = error.to_string();
    match error {
        InferenceError::Model(_) => crate::McpToolError::invalid_argument(message),
        InferenceError::NotConfigured(_) | InferenceError::Auth(_) => {
            crate::McpToolError::permission_denied(message)
        }
        InferenceError::Connection(_)
        | InferenceError::Overloaded(_)
        | InferenceError::CircuitOpen(_) => crate::McpToolError::unavailable(message),
        InferenceError::Timeout(_) => crate::McpToolError::unavailable(message),
        InferenceError::Generation(_)
        | InferenceError::Json(_)
        | InferenceError::VisionUnsupported(_) => crate::McpToolError::internal(message),
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaGenerationResponse {
    pub qa_pairs: Vec<QaPair>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct QaPair {
    pub question: String,
    pub answer: String,
    pub bloom_level: String,
    pub evidence_quotes: Vec<hkask_types::corpus::QaEvidence>,
}

/// Typed errors for QA response parsing.
#[derive(Debug, Clone, thiserror::Error)]
pub(crate) enum QaParseError {
    #[error("QA response must be JSON with a qa_pairs array: {0}")]
    InvalidJson(String),
    #[error("QA response must contain at least one QA pair")]
    Empty,
    #[error("QA pair {index} must have non-empty question and answer")]
    EmptyField { index: usize },
    #[error("QA pair {index} has unsupported Bloom level '{level}'")]
    InvalidBloomLevel { index: usize, level: String },
    #[error("QA pair {index} has an incomplete structured citation")]
    InvalidCitation { index: usize },
    #[error("QA pair {index} cites more distinct passages than supplied ({passage_count})")]
    TooManyPassages { index: usize, passage_count: usize },
}

/// Parse model output and validate citation structure, not semantic grounding.
/// An empty citation array explicitly makes no source-verification claim.
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
        if pair
            .evidence_quotes
            .iter()
            .any(|citation| !citation.is_complete())
        {
            return Err(QaParseError::InvalidCitation { index });
        }
        if let Some(passage_count) = cross_reference_passage_count {
            let identities: std::collections::HashSet<_> = pair
                .evidence_quotes
                .iter()
                .map(|citation| (&citation.chunk_ref, &citation.source))
                .collect();
            if identities.len() > passage_count {
                return Err(QaParseError::TooManyPassages {
                    index,
                    passage_count,
                });
            }
        }
    }

    Ok(parsed)
}

/// Legacy consolidation selection only. QA generation uses the dedicated
/// `hkask_inference::model_constants::resolve_qa_generation_model` resolver.
pub(crate) fn configured_qa_model(requested_model: Option<String>) -> Option<String> {
    if let Some(m) = requested_model {
        return Some(m);
    }
    std::env::var("HKASK_QA_MODEL")
        .ok()
        .or_else(|| std::env::var("HKASK_DEFAULT_MODEL").ok())
}
