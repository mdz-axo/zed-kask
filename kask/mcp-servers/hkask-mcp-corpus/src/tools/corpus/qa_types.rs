//! QA type helpers — Bloom taxonomy distribution and instructions.
//!
//! Used by `docproc_build_prompts` in `mod.rs` to generate QA prompts at
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
            "Extract ONE fact from passage. Generate FACTUAL question: identify specific capabilities, resources, metrics from passage. Direct answer from text. No explanation. No elaboration. Question asks what system has or achieves. Answer states fact. Keep output concise — caveman mode: drop filler, articles, hedging. Preserve all technical accuracy."
        }
        QaType::Conceptual => {
            "Generate a CONCEPTUAL question: explain the mechanisms linking capabilities to outcomes. How does a described capability theoretically translate into performance? What models or frameworks explain the capability-performance relationship?"
        }
        QaType::Analyze => {
            "Generate an ANALYZE question: compare capability-performance relationships across contexts. Identify patterns in where gaps emerge. Distinguish structural factors from situational ones. Break down the components of a system to understand how they interact."
        }
        QaType::Evaluate => {
            "Generate an EVALUATE question: assess explanations for capability-performance gaps. Critique the evidence. Judge whether claimed causal links are supported. Determine if an identified gap is economically significant or merely measurement noise. Consider what alternative explanations need to be ruled out."
        }
        QaType::Create => {
            "Generate a CREATE question: design interventions to close capability-performance gaps. Synthesize multi-domain strategies. Formulate testable hypotheses about what would happen if specific capabilities were deployed differently. Integrate concepts from the passage into a novel analytical framework."
        }
    }
}
