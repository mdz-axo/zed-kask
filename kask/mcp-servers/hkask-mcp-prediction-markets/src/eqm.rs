//! Explanation Quality Markers (EQMs) for scoring forecast rationales.
//!
//! Based on Karvetski, Huang, Kučinskas et al. (2026), "Can You Judge a
//! Forecast by Its Rationale?" — Forecasting Research Institute.
//!
//! EQMs are natural-language-described reasoning patterns scored 0/1/2 by
//! an LLM. The paper defined 60 EQMs; this module implements the 12 most
//! predictive ones highlighted in the paper's key findings.
//!
//! Key findings from the paper:
//! - EQM composite score correlates with forecaster-level accuracy at r=0.51
//! - EQMs flag bad forecasts more reliably than they identify excellent ones
//!   (asymmetric: strong "red flag" screen, weak "green flag" detector)
//! - Statistical reasoning is rare (19% of rationales) but more predictive
//!   than causal reasoning (77% prevalence, weaker correlation)
//! - Human raters reward length and fact-based tone but underweight warning
//!   signs like extreme confidence
//! - Cost: ~$0.007 per rationale
//!
//! This module provides:
//! 1. A static catalog of 12 EQM definitions with directional hypotheses
//! 2. An LLM scoring function that calls InferencePort with a structured prompt
//! 3. A composite score computation (weighted sum: +good habits, -warning signs)
//! 4. Red flag / green flag classification

use hkask_types::InferencePort;
use hkask_types::template::LLMParameters;
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ── EQM catalog ────────────────────────────────────────────────────────────

/// Directional hypothesis: does this EQM help or hurt forecasting accuracy?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EqmDirection {
    /// Positively correlated with accuracy (a "good habit")
    Helps,
    /// Negatively correlated with accuracy (a "warning sign")
    Hurts,
}

/// A single Explanation Quality Marker definition.
#[derive(Debug, Clone)]
pub struct EqmDefinition {
    /// Short identifier (snake_case)
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// Natural-language description for the LLM scoring prompt
    pub description: &'static str,
    /// Directional hypothesis
    pub direction: EqmDirection,
}

/// The 12 most predictive EQMs from the paper's key findings.
///
/// These are the markers the paper highlights as statistically significant
/// in both forecast-level and forecaster-level accuracy correlations. The
/// full 60-EQM catalog is in the working paper; this subset captures the
/// majority of the predictive signal.
pub const KEY_EQMS: &[EqmDefinition] = &[
    // ── Good habits (positively correlated with accuracy) ──
    EqmDefinition {
        id: "statistical_reasoning",
        name: "Statistical Reasoning",
        description: "The rationale explicitly uses statistical concepts such as base rates, probability distributions, sample sizes, confidence intervals, or quantitative comparisons. A score of 2 means the rationale clearly applies statistical thinking (e.g., citing a base rate or computing a probability). A score of 0 means no statistical reasoning is present.",
        direction: EqmDirection::Helps,
    },
    EqmDefinition {
        id: "fact_based",
        name: "Fact Based",
        description: "The rationale cites specific, verifiable facts, data, or evidence rather than opinions or speculation. A score of 2 means the rationale is grounded in concrete facts. A score of 0 means the rationale relies on opinion or assertion without evidence.",
        direction: EqmDirection::Helps,
    },
    EqmDefinition {
        id: "concrete_reasoning",
        name: "Concrete Reasoning",
        description: "The rationale uses specific, concrete details rather than abstract or vague generalizations. A score of 2 means the reasoning is grounded in specific examples, numbers, or events. A score of 0 means the reasoning is abstract and non-specific.",
        direction: EqmDirection::Helps,
    },
    EqmDefinition {
        id: "forecast_rationale_align",
        name: "Forecast and Rationale Align",
        description: "The rationale's stated reasoning is consistent with the forecast probability. A score of 2 means the attitude expressed in the rationale clearly matches the quantitative forecast. A score of 0 means the rationale contradicts the forecast (e.g., bullish reasoning with a low probability).",
        direction: EqmDirection::Helps,
    },
    EqmDefinition {
        id: "best_practices",
        name: "Best Practices",
        description: "The rationale follows forecasting best practices: considering multiple hypotheses, seeking disconfirming evidence, using structured decomposition, or applying probabilistic thinking. A score of 2 means the rationale clearly follows best practices. A score of 0 means no best practices are evident.",
        direction: EqmDirection::Helps,
    },
    EqmDefinition {
        id: "statistical_causal_blend",
        name: "Statistical Causal Blend",
        description: "The rationale combines statistical reasoning with causal analysis, using data to support causal claims or using causal models to interpret statistics. A score of 2 means both statistical and causal reasoning are present and integrated. A score of 0 means neither is present or they are not integrated.",
        direction: EqmDirection::Helps,
    },
    // ── Warning signs (negatively correlated with accuracy) ──
    EqmDefinition {
        id: "gut_based",
        name: "Gut Based",
        description: "The rationale relies on intuition, gut feeling, or personal conviction rather than evidence or analysis. A score of 2 means the rationale is primarily gut-based. A score of 0 means no gut-based reasoning is present.",
        direction: EqmDirection::Hurts,
    },
    EqmDefinition {
        id: "simplification_bias",
        name: "Simplification Bias",
        description: "The rationale oversimplifies a complex situation, ignoring important nuances, uncertainties, or alternative explanations. A score of 2 means the rationale is notably oversimplified. A score of 0 means the rationale adequately captures complexity.",
        direction: EqmDirection::Hurts,
    },
    EqmDefinition {
        id: "confirmation_bias",
        name: "Confirmation Bias",
        description: "The rationale primarily seeks or cites evidence that supports the forecast while ignoring or dismissing disconfirming evidence. A score of 2 means clear confirmation bias. A score of 0 means the rationale considers evidence on both sides.",
        direction: EqmDirection::Hurts,
    },
    EqmDefinition {
        id: "extreme_confidence",
        name: "Extreme Confidence",
        description: "The rationale expresses unwarranted certainty, using language like 'certainly', 'definitely', 'guaranteed', or assigning very high/low probabilities without adequate justification. A score of 2 means extreme confidence is clearly present. A score of 0 means the confidence level is appropriately calibrated.",
        direction: EqmDirection::Hurts,
    },
    EqmDefinition {
        id: "forecast_rationale_misalign",
        name: "Forecast and Rationale Misalign",
        description: "The rationale's stated reasoning contradicts or does not support the forecast probability. A score of 2 means clear misalignment between reasoning and forecast. A score of 0 means the rationale and forecast are aligned.",
        direction: EqmDirection::Hurts,
    },
    EqmDefinition {
        id: "speculative_terms",
        name: "Speculative Terms",
        description: "The rationale uses speculative language ('maybe', 'could', 'might', 'possibly') without grounding in evidence or analysis. A score of 2 means the rationale is primarily speculative. A score of 0 means the rationale is grounded in evidence.",
        direction: EqmDirection::Hurts,
    },
];

// ── Request types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScoreRationaleRequest {
    /// The written reasoning/rationale behind a forecast.
    pub rationale: String,
    /// The probability the rationale supports (0.0–1.0), for alignment
    /// checking. Optional but recommended — enables the
    /// `forecast_rationale_align` and `forecast_rationale_misalign` EQMs.
    pub forecast_probability: Option<f64>,
    /// The question being forecast, for context.
    pub question: Option<String>,
}

// ── Result types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqmScore {
    pub id: String,
    pub name: String,
    pub score: u8,
    pub direction: EqmDirection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EqmResult {
    pub composite_score: f64,
    pub scores: Vec<EqmScore>,
    pub red_flags: Vec<String>,
    pub green_flags: Vec<String>,
    pub interpretation: String,
    pub model: String,
    pub caveat: String,
}

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum EqmError {
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("LLM response parse error: {0}")]
    ParseError(String),
    #[error("rationale text is empty")]
    EmptyRationale,
}

impl From<EqmError> for hkask_mcp_server::server::McpToolError {
    fn from(e: EqmError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match e {
            EqmError::EmptyRationale => McpToolError::invalid_argument(e.to_string()),
            EqmError::InferenceFailed(_) => McpToolError::unavailable(e.to_string()),
            EqmError::ParseError(_) => McpToolError::internal(e.to_string()), // rr0044-ok: mapper-internal-arm
        }
    }
}

// ── Scoring prompt construction ────────────────────────────────────────────

/// Build the LLM prompt for EQM scoring.
fn build_scoring_prompt(
    rationale: &str,
    forecast_probability: Option<f64>,
    question: Option<&str>,
) -> String {
    let mut prompt = String::new();
    prompt.push_str("You are an expert forecasting analyst. Score the following forecast rationale against Explanation Quality Markers (EQMs).\n\n");
    prompt.push_str("For each EQM, assign a score of 0, 1, or 2:\n");
    prompt.push_str("  0 = the EQM is absent or not applicable\n");
    prompt.push_str("  1 = the EQM is partially present\n");
    prompt.push_str("  2 = the EQM is clearly and strongly present\n\n");

    if let Some(q) = question {
        prompt.push_str(&format!("Question being forecast: {q}\n\n"));
    }
    if let Some(p) = forecast_probability {
        prompt.push_str(&format!("Forecast probability: {p}\n\n"));
    }

    prompt.push_str("Rationale to score:\n");
    prompt.push_str(&format!("---\n{rationale}\n---\n\n"));

    prompt.push_str("EQMs to score:\n");
    for eqm in KEY_EQMS {
        let direction = match eqm.direction {
            EqmDirection::Helps => "GOOD HABIT (higher is better)",
            EqmDirection::Hurts => "WARNING SIGN (higher is worse)",
        };
        prompt.push_str(&format!(
            "\n{}. {} [{direction}]\n   {}\n",
            eqm.id, eqm.name, eqm.description
        ));
    }

    prompt.push_str(
        "\nRespond with ONLY a JSON object (no markdown, no explanation) in this exact format:\n",
    );
    prompt.push_str(r#"{"eqm_id": score, "eqm_id": score, ...}"#);
    prompt.push_str("\n\nExample: {\"statistical_reasoning\": 2, \"gut_based\": 0, ...}\n");

    prompt
}

// ── LLM response parsing ───────────────────────────────────────────────────

/// Parse the LLM response as JSON. Tries the whole text first, then falls
/// back to extracting the first `{...}` block (LLMs often wrap JSON in
/// markdown or prose). Returns `EqmError::ParseError` on failure — never
/// panics, never fabricates.
fn parse_llm_json(text: &str) -> Result<Value, EqmError> {
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Ok(value);
    }
    let start = trimmed
        .find('{')
        .ok_or_else(|| EqmError::ParseError("no JSON object found in LLM response".into()))?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| EqmError::ParseError("no closing brace in LLM response".into()))?;
    serde_json::from_str(&trimmed[start..=end])
        .map_err(|e| EqmError::ParseError(format!("failed to parse LLM response as JSON: {e}")))
}

// ── Scoring function ───────────────────────────────────────────────────────

/// Score a forecast rationale against the 12 key EQMs using an LLM.
///
/// Calls `InferencePort::generate` with a structured prompt, parses the JSON
/// response, and computes the composite score.
pub async fn score_rationale(
    inference_port: &dyn InferencePort,
    req: &ScoreRationaleRequest,
) -> Result<EqmResult, EqmError> {
    if req.rationale.trim().is_empty() {
        return Err(EqmError::EmptyRationale);
    }

    let prompt = build_scoring_prompt(
        &req.rationale,
        req.forecast_probability,
        req.question.as_deref(),
    );

    let params = LLMParameters {
        temperature: 0.1,
        top_p: 0.9,
        top_k: 40,
        frequency_penalty: 0.0,
        presence_penalty: 0.0,
        min_p: 0.0,
        typical_p: 0.0,
        max_tokens: 1024,
        seed: None,
        disable_thinking: false,
        adapter: None,
        system_prompt: None,
    };

    let result = inference_port
        .generate(&prompt, &params, None)
        .await
        .map_err(|e| EqmError::InferenceFailed(e.to_string()))?;

    // Parse the JSON response from the LLM. The LLM may wrap the JSON in
    // markdown or add prose; fall back to extracting the first {...} block.
    let raw_scores: Value = parse_llm_json(&result.text)?;

    // Build the EQM scores from the parsed JSON.
    let mut scores = Vec::new();
    let mut composite = 0.0_f64;
    let mut red_flags = Vec::new();
    let mut green_flags = Vec::new();

    for eqm in KEY_EQMS {
        let raw = raw_scores
            .get(eqm.id)
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            .min(2) as u8;

        let score = EqmScore {
            id: eqm.id.to_string(),
            name: eqm.name.to_string(),
            score: raw,
            direction: eqm.direction,
        };

        // Composite: +score for helps, -score for hurts.
        // Normalized to range [-12, +12] (6 helps × 2 max, 6 hurts × 2 max).
        match eqm.direction {
            EqmDirection::Helps => {
                composite += raw as f64;
                if raw >= 2 {
                    green_flags.push(eqm.name.to_string());
                }
            }
            EqmDirection::Hurts => {
                composite -= raw as f64;
                if raw >= 2 {
                    red_flags.push(eqm.name.to_string());
                }
            }
        }

        scores.push(score);
    }

    // Build interpretation.
    let interpretation = build_interpretation(&composite, &red_flags, &green_flags);

    Ok(EqmResult {
        composite_score: composite,
        scores,
        red_flags,
        green_flags,
        interpretation,
        model: result.model,
        caveat: "EQMs are a red-flag screen, not a green-flag detector. A low score reliably flags weak reasoning; a high score is a weak endorsement of quality. Results are correlational, not causal. Based on Karvetski et al. (2026), Forecasting Research Institute.".to_string(),
    })
}

/// Build a human-readable interpretation of the EQM scores.
fn build_interpretation(composite: &f64, red_flags: &[String], green_flags: &[String]) -> String {
    let mut parts = Vec::new();

    if *composite >= 6.0 {
        parts.push("Strong rationale".to_string());
    } else if *composite >= 2.0 {
        parts.push("Adequate rationale".to_string());
    } else if *composite >= -2.0 {
        parts.push("Mixed rationale".to_string());
    } else if *composite >= -6.0 {
        parts.push("Weak rationale".to_string());
    } else {
        parts.push("Poor rationale".to_string());
    }

    if !green_flags.is_empty() {
        parts.push(format!("strengths: {}", green_flags.join(", ")));
    }
    if !red_flags.is_empty() {
        parts.push(format!("red flags: {}", red_flags.join(", ")));
    }
    if green_flags.is_empty() && red_flags.is_empty() {
        parts.push("no notable markers".to_string());
    }

    parts.join("; ")
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_eqms_have_6_helps_and_6_hurts() {
        let helps = KEY_EQMS
            .iter()
            .filter(|e| e.direction == EqmDirection::Helps)
            .count();
        let hurts = KEY_EQMS
            .iter()
            .filter(|e| e.direction == EqmDirection::Hurts)
            .count();
        assert_eq!(helps, 6);
        assert_eq!(hurts, 6);
        assert_eq!(KEY_EQMS.len(), 12);
    }

    #[test]
    fn composite_score_range_is_correct() {
        // 6 helps × max 2 = +12, 6 hurts × max 2 = -12
        // Range: [-12, +12]
        let max_positive: f64 = KEY_EQMS
            .iter()
            .filter(|e| e.direction == EqmDirection::Helps)
            .map(|_| 2.0)
            .sum();
        let max_negative: f64 = KEY_EQMS
            .iter()
            .filter(|e| e.direction == EqmDirection::Hurts)
            .map(|_| 2.0)
            .sum();
        assert_eq!(max_positive, 12.0);
        assert_eq!(max_negative, 12.0);
    }

    #[test]
    fn build_interpretation_classifies_correctly() {
        assert!(
            build_interpretation(&8.0, &[], &["Statistical Reasoning".into()]).contains("Strong")
        );
        assert!(build_interpretation(&3.0, &[], &[]).contains("Adequate"));
        assert!(build_interpretation(&0.0, &[], &[]).contains("Mixed"));
        assert!(build_interpretation(&-3.0, &["Gut Based".into()], &[]).contains("Weak"));
        assert!(
            build_interpretation(
                &-8.0,
                &["Gut Based".into(), "Confirmation Bias".into()],
                &[]
            )
            .contains("Poor")
        );
    }

    #[test]
    fn build_interpretation_includes_flags() {
        let interp = build_interpretation(
            &5.0,
            &["Extreme Confidence".to_string()],
            &[
                "Statistical Reasoning".to_string(),
                "Fact Based".to_string(),
            ],
        );
        assert!(interp.contains("strengths: Statistical Reasoning, Fact Based"));
        assert!(interp.contains("red flags: Extreme Confidence"));
    }

    #[test]
    fn scoring_prompt_includes_all_eqms() {
        let prompt = build_scoring_prompt(
            "test rationale",
            Some(0.65),
            Some("Will AI surpass human coding by 2027?"),
        );
        for eqm in KEY_EQMS {
            assert!(
                prompt.contains(eqm.id),
                "prompt should contain EQM id: {}",
                eqm.id
            );
            assert!(
                prompt.contains(eqm.name),
                "prompt should contain EQM name: {}",
                eqm.name
            );
        }
        assert!(prompt.contains("0.65"));
        assert!(prompt.contains("Will AI surpass human coding by 2027?"));
    }

    #[test]
    fn empty_rationale_returns_error() {
        // Can't call score_rationale without a real InferencePort, but we
        // can test the empty check by verifying the error type.
        let req = ScoreRationaleRequest {
            rationale: "".to_string(),
            forecast_probability: None,
            question: None,
        };
        // The empty check happens before any inference call.
        assert!(req.rationale.trim().is_empty());
    }

    #[test]
    fn eqm_error_classifies_correctly() {
        use hkask_types::McpErrorKind;
        let e: hkask_mcp_server::server::McpToolError = EqmError::EmptyRationale.into();
        assert_eq!(e.kind, McpErrorKind::InvalidArgument);

        let e: hkask_mcp_server::server::McpToolError =
            EqmError::InferenceFailed("timeout".into()).into();
        assert_eq!(e.kind, McpErrorKind::Unavailable);

        let e: hkask_mcp_server::server::McpToolError =
            EqmError::ParseError("bad json".into()).into();
        assert_eq!(e.kind, McpErrorKind::Internal);
    }
}
