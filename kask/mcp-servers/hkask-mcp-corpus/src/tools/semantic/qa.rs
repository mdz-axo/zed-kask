//! QA generation helpers — response parsing, error types, model resolution.
//!
//! Used by `corpus_generate_qa` and `corpus_generate_qa_batch` in `semantic.rs`.

use serde::{Deserialize, Serialize};

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
pub(crate) enum QaParseError {
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

/// Resolve the QA model from request override, env, or settings default.
pub(crate) fn configured_qa_model(requested_model: Option<String>) -> Option<String> {
    if let Some(m) = requested_model {
        return Some(m);
    }
    std::env::var("HKASK_QA_MODEL")
        .ok()
        .or_else(|| std::env::var("HKASK_DEFAULT_MODEL").ok())
}
