//! QA type helpers — Bloom taxonomy distribution and instructions.
//!
//! Used by `corpus_build_prompts` in `tools/corpus.rs` to generate QA prompts at
//! consecutive Bloom levels.

/// QA type corresponding to Bloom's taxonomy levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum QaType {
    Factual,
    Conceptual,
    Analyze,
    Evaluate,
    Create,
}

impl QaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Factual => "factual",
            Self::Conceptual => "conceptual",
            Self::Analyze => "analyze",
            Self::Evaluate => "evaluate",
            Self::Create => "create",
        }
    }
}

/// The typed validation failure for a type-distribution spec. The `Display`
/// text is the operator-facing explanation (it reaches the tool caller via
/// `McpToolError::invalid_argument`), so each variant names the exact defect.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub(crate) enum TypeDistributionError {
    #[error(
        "type_distribution must be exactly 5 comma-separated weights (factual, conceptual, analyze, evaluate, create); got {0}"
    )]
    WrongArity(usize),
    #[error("type_distribution weight {weight} is not a nonnegative integer: {part:?}")]
    NotAnInteger { weight: usize, part: String },
    #[error("type_distribution must request at least one QA type (all weights are zero)")]
    AllZero,
}

/// Parse a type distribution spec like "1,1,2,1,0" into a list of QaType
/// values. The 5 numbers correspond to Factual, Conceptual, Analyze,
/// Evaluate, Create. The spec must be exactly 5 comma-separated nonnegative
/// integers with at least one nonzero weight: a shifted or truncated spec
/// silently corrupts the positional Bloom mapping (a typo in entry 3 turns
/// every later level into its predecessor), so partial parsing must reject
/// rather than drop entries.
pub(crate) fn parse_type_distribution(spec: &str) -> Result<Vec<QaType>, TypeDistributionError> {
    let parts: Vec<&str> = spec.split(',').map(str::trim).collect();
    if parts.len() != 5 {
        return Err(TypeDistributionError::WrongArity(parts.len()));
    }
    let mut nums = [0usize; 5];
    for (i, part) in parts.iter().enumerate() {
        nums[i] = part
            .parse()
            .map_err(|_| TypeDistributionError::NotAnInteger {
                weight: i + 1,
                part: part.to_string(),
            })?;
    }
    if nums.iter().all(|&count| count == 0) {
        return Err(TypeDistributionError::AllZero);
    }
    let types = [
        QaType::Factual,
        QaType::Conceptual,
        QaType::Analyze,
        QaType::Evaluate,
        QaType::Create,
    ];
    let mut result = Vec::new();
    for (i, &count) in nums.iter().enumerate() {
        for _ in 0..count {
            result.push(types[i]);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_distribution_maps_positionally() {
        let rotation = parse_type_distribution("1,1,2,1,0").expect("valid spec parses");
        assert_eq!(
            rotation,
            vec![
                QaType::Factual,
                QaType::Conceptual,
                QaType::Analyze,
                QaType::Analyze,
                QaType::Evaluate
            ]
        );
    }

    #[test]
    fn non_integer_entry_is_rejected_not_shifted() {
        // The silent-corruption case: a typo in one entry previously dropped
        // it and shifted every later Bloom level into its predecessor.
        let err = parse_type_distribution("1,1,x,1,1").expect_err("typo rejected");
        assert_eq!(
            err,
            TypeDistributionError::NotAnInteger {
                weight: 3,
                part: "x".to_string()
            },
            "names the bad entry"
        );
    }

    #[test]
    fn wrong_entry_count_is_rejected() {
        assert!(parse_type_distribution("1").is_err());
        assert!(parse_type_distribution("1,1,1,1").is_err());
        assert!(parse_type_distribution("1,1,1,1,1,1").is_err());
        assert!(parse_type_distribution("").is_err());
    }

    #[test]
    fn all_zero_distribution_is_rejected() {
        assert!(parse_type_distribution("0,0,0,0,0").is_err());
    }
}

pub(crate) fn qa_type_instruction(qt: QaType) -> &'static str {
    match qt {
        QaType::Factual => {
            "Ask about one specific detail, definition, quantity, or claim explicitly stated by substantive selected evidence. Give the directly stated fact as a concise answer, with no inference, synthesis, explanation, or outside knowledge. Do not generate QA from legal notices, publication metadata, navigation, marketing, watermarks, isolated captions, or garbled text; use the quality-skip contract instead."
        }
        QaType::Conceptual => {
            "Ask for an explanation of a mechanism, causal relationship, meaningful distinction, purpose, framework, or transferable principle explicitly supported by the selected evidence. A conceptual question must not be answerable by direct recall of one name, label, list, title, number, or sentence-level paraphrase. If the evidence supports only recall, or has no explicit conceptual relationship to explain, use conceptual_support_absent through the quality-skip contract instead of generating or relabeling factual QA."
        }
        QaType::Analyze => {
            "Generate an ANALYZE question: compare or contrast ideas within the passage. Identify patterns, distinguish structural factors from situational ones, or break down the components of a system described in the text to understand how they interact."
        }
        QaType::Evaluate => {
            "Generate an EVALUATE question: assess the strength of arguments or evidence presented in the passage. Critique the reasoning. Judge whether the claims are well-supported. Consider what alternative explanations or counterarguments the passage does not address."
        }
        QaType::Create => {
            "Generate a CREATE question: synthesize ideas from the passage into a novel application, design, or hypothesis. Formulate a testable hypothesis based on the passage's concepts. Propose how the ideas could be applied in a different context. Integrate concepts from the passage into a new framework."
        }
    }
}
