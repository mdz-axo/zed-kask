//! QA type helpers — Bloom taxonomy distribution and instructions.
//!
//! Used by `corpus_build_prompts` in `tools/corpus.rs` to generate QA prompts at
//! consecutive Bloom levels.

/// QA type corresponding to Bloom's taxonomy levels.
#[derive(Debug, Clone, Copy)]
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

pub(crate) fn qa_type_str(qt: QaType) -> &'static str {
    qt.as_str()
}

/// Parse a type distribution spec like "1,1,2,1,0" into a list of QaType
/// values. The 5 numbers correspond to Factual, Conceptual, Analyze,
/// Evaluate, Create. Empty or invalid specs default to [Factual].
pub(crate) fn parse_type_distribution(spec: &str) -> Vec<QaType> {
    let nums: Vec<usize> = spec
        .split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect();
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
            if i < types.len() {
                result.push(types[i]);
            }
        }
    }
    if result.is_empty() {
        vec![QaType::Factual]
    } else {
        result
    }
}

pub(crate) fn qa_type_instruction(qt: QaType) -> &'static str {
    match qt {
        QaType::Factual => {
            "Extract ONE fact from the passage. Generate a FACTUAL question that asks about a specific detail, definition, quantity, or claim stated in the text. The answer must be directly stated in the passage — no inference, no synthesis. No explanation. No elaboration. Answer states the fact concisely."
        }
        QaType::Conceptual => {
            "Generate a CONCEPTUAL question: explain a mechanism, relationship, or framework described in the passage. How does one concept described in the text connect to another? What theoretical model does the passage present, and how do its components interact?"
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
