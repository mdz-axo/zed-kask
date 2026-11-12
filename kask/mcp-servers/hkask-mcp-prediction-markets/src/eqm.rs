//! Explanation Quality Markers (EQMs) for scoring forecast rationales.
//!
//! Based on Karvetski, Huang, Kučinskas et al. (2026), "Can You Judge a
//! Forecast by Its Rationale?" — Forecasting Research Institute.
//!
//! EQMs are natural-language-described reasoning patterns scored 0/1/2 by
//! the decision model — a Jev-class System One model, `kask.models.decision_model`
//! (typed score questions in, calibrated per-level probabilities out; never
//! a chat prompt). The paper defined 60 EQMs; this module implements the 12 most
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
//! 2. A decision scoring function that calls `InferencePort::decide` with
//!    typed score questions over the Decisions API — never a chat prompt
//! 3. A composite score computation (weighted sum: +good habits, -warning signs)
//! 4. Red flag / green flag classification

use hkask_types::InferencePort;
use serde::{Deserialize, Serialize};

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
    /// EQM dimensions the decision model omitted from its answers. Non-empty means
    /// the composite score is pulled toward 0 for those dimensions — the
    /// operator should treat a low score with missing EQMs as "incomplete
    /// assessment," not "low quality."
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_eqms: Vec<String>,
}

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum EqmError {
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("rationale text is empty")]
    EmptyRationale,
    #[error("decision model not configured: {0}")]
    DecisionNotConfigured(String),
    #[error(
        "private-marked context refused: the decision model is cloud-served and private content never crosses to it — no local decision model exists yet; score on a public thread"
    )]
    PrivateProvenanceRefused,
}

impl From<EqmError> for hkask_mcp_server::server::McpToolError {
    fn from(e: EqmError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match e {
            EqmError::EmptyRationale => McpToolError::invalid_argument(e.to_string()),
            EqmError::InferenceFailed(_) => McpToolError::unavailable(e.to_string()),
            // The NotConfigured refusal names the setting and the private
            // refusal names the boundary (the classifier/QA precedent —
            // permission_denied, never a silent chat-model fallback).
            EqmError::DecisionNotConfigured(_) | EqmError::PrivateProvenanceRefused => {
                McpToolError::permission_denied(e.to_string())
            }
        }
    }
}

// ── Decision request construction ──────────────────────────────────────────

/// The EQM operational scale (eqm-catalog.yaml: 0 = absent, 1 = partial,
/// 2 = clearly present) as the decision question's ordered level
/// descriptions — the same rubric the former chat prompt stated inline.
const DECISION_LEVELS: &[&str] = &[
    "Absent or not applicable",
    "Partially present",
    "Clearly and strongly present",
];

/// Build the decision state — the same context the former chat prompt
/// carried (the question, the forecast probability, the rationale), now
/// the `state` a decision model evaluates typed questions against.
fn build_decision_state(
    rationale: &str,
    forecast_probability: Option<f64>,
    question: Option<&str>,
) -> String {
    let mut state = String::new();
    if let Some(q) = question {
        state.push_str(&format!("Question being forecast: {q}\n\n"));
    }
    if let Some(p) = forecast_probability {
        state.push_str(&format!("Forecast probability: {p}\n\n"));
    }
    state.push_str(&format!("Rationale:\n{rationale}"));
    state
}

/// One typed score question per key EQM: the marker's operational
/// description (the rubric's 0-means/2-means clauses) as the instructions,
/// the catalog's uniform three-level scale as the criteria. The Jev-class
/// "Score" primitive — the model answers with a calibrated probability
/// distribution over these levels, never text.
fn decision_questions() -> Vec<hkask_types::inference_ipc::DecisionQuestion> {
    KEY_EQMS
        .iter()
        .map(|eqm| {
            let direction = match eqm.direction {
                EqmDirection::Helps => "a good habit; higher is better",
                EqmDirection::Hurts => "a warning sign; higher is worse",
            };
            hkask_types::inference_ipc::DecisionQuestion {
                id: eqm.id.to_string(),
                instructions: format!(
                    "Score the rationale on the Explanation Quality Marker \"{}\" ({}). {}",
                    eqm.name, direction, eqm.description
                ),
                criteria: DECISION_LEVELS.iter().map(|s| s.to_string()).collect(),
            }
        })
        .collect()
}

/// Map a decision answer to the integer EQM score: the most probable level
/// (argmax over the calibrated per-level probabilities) — the same 0-2
/// integer scale the composite, the flags and the skill's EQM-2 screens
/// consume. Never a fabricated value: a levelless answer reads `None` and
/// the caller records the marker as missing.
fn answer_level(answer: &hkask_types::inference_ipc::DecisionAnswer) -> Option<u8> {
    answer
        .level_probabilities
        .iter()
        .max_by(|a, b| {
            a.probability
                .partial_cmp(&b.probability)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|entry| entry.level)
}

// ── Scoring function ───────────────────────────────────────────────────────

/// Score a forecast rationale against the 12 key EQMs using the decision
/// model (`kask.models.decision_model`).
///
/// Sends the rationale as the decision state plus one typed score question
/// per marker (the Decisions API: the model returns a calibrated probability
/// distribution over each marker's 0/1/2 levels, never text); the composite,
/// the red/green screens and the interpretation stay deterministic in this
/// code (the skill's `eqm/eqm-aggregate` render computes the EQM-2 screens). An
/// unset decision model is a typed refusal naming the setting — never a
/// chat-model fallback; a private-provenance call refuses at the boundary
/// (the decision model is cloud-served; private content never crosses).
pub async fn score_rationale(
    inference_port: &dyn InferencePort,
    req: &ScoreRationaleRequest,
) -> Result<EqmResult, EqmError> {
    if req.rationale.trim().is_empty() {
        return Err(EqmError::EmptyRationale);
    }

    // The boundary guard (the D100 forward local-routing rule's tool-call
    // leg): the configured decision model is cloud-served, so a
    // private-provenance call refuses here — it never crosses. The
    // pre-adoption path crossed to the cloud chat model on every thread;
    // this closes that leg for private contexts.
    if matches!(
        hkask_mcp_server::server::current_call_provenance(),
        hkask_types::InferenceProvenance::PrivateInference
    ) {
        return Err(EqmError::PrivateProvenanceRefused);
    }

    let model = hkask_inference::model_constants::resolve_decision_model()
        .map_err(|e| EqmError::DecisionNotConfigured(e.to_string()))?;

    let state = build_decision_state(
        &req.rationale,
        req.forecast_probability,
        req.question.as_deref(),
    );
    let questions = decision_questions();

    let batch = inference_port
        .decide(&model, &state, &questions)
        .await
        .map_err(|e| EqmError::InferenceFailed(e.to_string()))?;

    // mcp-tool-review S-01 (token half): surface the measured per-call
    // usage in the tool envelope so the billed-token axis is observable
    // at the caller. The envelope aggregates every record a handler
    // makes; this and the mcda weight-and-score call are the
    // prediction-markets server's two inference calls.
    hkask_mcp_server::server::record_tool_usage(batch.usage.clone());

    // Build the EQM scores from the decision answers.
    let mut scores = Vec::new();
    let mut composite = 0.0_f64;
    let mut red_flags = Vec::new();
    let mut green_flags = Vec::new();
    let mut missing_eqms = Vec::new();

    for eqm in KEY_EQMS {
        let raw = match batch
            .answers
            .iter()
            .find(|answer| answer.id == eqm.id)
            .and_then(answer_level)
        {
            Some(level) if (level as usize) < DECISION_LEVELS.len() => level,
            _ => {
                tracing::warn!(
                    target: "hkask.mcp.prediction_markets.eqm",
                    eqm = eqm.id,
                    "decision answer unusable (absent, levelless, or out of range) — recording as missing, not zero-filling"
                );
                missing_eqms.push(eqm.id.to_string());
                0
            }
        };

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

    if !missing_eqms.is_empty() {
        tracing::warn!(
            target: "hkask.mcp.prediction_markets.eqm",
            missing = ?missing_eqms,
            missing_count = missing_eqms.len(),
            total_eqms = KEY_EQMS.len(),
            "decision answers omitted EQM dimensions — composite score is pulled toward 0 \
             for these dimensions; treat as incomplete assessment, not low quality"
        );
    }

    Ok(EqmResult {
        composite_score: composite,
        scores,
        red_flags,
        green_flags,
        interpretation,
        model: batch.model,
        caveat: "EQMs are a red-flag screen, not a green-flag detector. A low score reliably flags weak reasoning; a high score is a weak endorsement of quality. Results are correlational, not causal. Based on Karvetski et al. (2026), Forecasting Research Institute.".to_string(),
        missing_eqms,
    })
}

/// Build a human-readable interpretation of the EQM scores.
fn build_interpretation(composite: &f64, red_flags: &[String], green_flags: &[String]) -> String {
    let mut parts = Vec::new();

    if *composite >= 6.0 {
        // The skill's asymmetric-signal rule (Karvetski et al. 2026): EQMs flag
        // bad forecasts more reliably than they identify excellent ones. A
        // high composite with no red flags is a WEAK positive endorsement —
        // "strong" is reserved for forecasters whose EQM composite correlates
        // with accuracy on realized outcomes (outcome-validated, never
        // score-alone). The pre-repair label violated the rule the tool
        // itself carries in its caveat.
        parts.push("High composite — weak positive endorsement (EQMs are a red-flag screen, not a green-flag detector; 'strong' requires outcome validation)".to_string());
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

    /// B.9 (operator resolution 2026-09-28): `eqm-catalog.yaml` is the source
    /// of truth for the predictive 12; `KEY_EQMS` is its operational subset.
    /// This pins the equality — a catalog edit that drops or adds a
    /// predictive marker without updating `KEY_EQMS` (or vice versa) fails
    /// here. The catalog is `include_str!`-ed so an edit recompiles this
    /// test (the pin lives at the seam, not at a copy).
    #[test]
    fn key_eqms_match_catalog_predictive_twelve() {
        let catalog = include_str!("../../../registry/templates/eqm/eqm-catalog.yaml");
        // Marker-level ids are indented `      - id:` under a family's
        // `markers:` list; their predictive flag sits at
        // `        predictive:`. Family ids sit at `  - id:` — the scan
        // tracks only the marker level, so a family id can never be
        // collected (the 2026-10-01 extraction lesson: `alignment_and_discipline`
        // is a family, `forecast_rationale_align` is its predictive marker).
        let mut catalog_predictive: Vec<&str> = Vec::new();
        let mut current_marker: Option<&str> = None;
        for line in catalog.lines() {
            if let Some(id) = line.strip_prefix("      - id: ") {
                current_marker = Some(id);
            } else if line.trim() == "predictive: true" {
                if let Some(id) = current_marker {
                    catalog_predictive.push(id);
                }
            }
        }
        let const_ids: Vec<&str> = KEY_EQMS.iter().map(|e| e.id).collect();
        assert_eq!(
            catalog_predictive.len(),
            12,
            "the catalog's predictive set must be twelve"
        );
        assert_eq!(const_ids.len(), 12, "KEY_EQMS must carry twelve markers");
        let mut sorted_catalog = catalog_predictive.clone();
        sorted_catalog.sort();
        let mut sorted_const = const_ids.clone();
        sorted_const.sort();
        assert_eq!(
            sorted_catalog, sorted_const,
            "KEY_EQMS and the catalog's predictive-flagged markers must be the same id set"
        );
    }

    /// A port whose every call is unreachable — the refusal paths under
    /// test must never reach it (the private guard and the unset-model
    /// resolution both precede any port call).
    struct RefusingPort;
    impl hkask_types::InferencePort for RefusingPort {
        fn generate<'a>(
            &'a self,
            _prompt: &str,
            _parameters: &hkask_types::template::LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> hkask_types::InferenceFuture<'a> {
            unreachable!("no port call may precede the refusal paths under test")
        }
    }

    fn request(rationale: &str) -> ScoreRationaleRequest {
        ScoreRationaleRequest {
            rationale: rationale.to_string(),
            forecast_probability: None,
            question: None,
        }
    }

    #[test]
    fn decision_questions_carry_the_catalog_scale_and_rubric() {
        let questions = decision_questions();
        assert_eq!(questions.len(), KEY_EQMS.len());
        for (question, eqm) in questions.iter().zip(KEY_EQMS.iter()) {
            assert_eq!(
                question.id, eqm.id,
                "the answer returns under the marker id"
            );
            assert_eq!(
                question.criteria,
                vec![
                    "Absent or not applicable",
                    "Partially present",
                    "Clearly and strongly present",
                ],
                "the catalog's uniform 0/1/2 scale is the criteria"
            );
            assert!(
                question.instructions.contains(eqm.name),
                "the marker's name rides the instructions"
            );
            assert!(
                question.instructions.contains(eqm.description),
                "the operational rubric (the 0-means/2-means clauses) rides the instructions"
            );
        }
    }

    #[test]
    fn answer_level_takes_the_most_probable_level() {
        // The documented Jev score shape (research run b1f1663057722f5d):
        // probabilities {0: 0.0, 1: 0.79, 2: 0.21} → the argmax level is 1.
        let answer = hkask_types::inference_ipc::DecisionAnswer {
            id: "severity".to_string(),
            score: 1.21,
            level_probabilities: vec![
                hkask_types::inference_ipc::DecisionLevelProbability {
                    level: 0,
                    probability: 0.0,
                },
                hkask_types::inference_ipc::DecisionLevelProbability {
                    level: 1,
                    probability: 0.79,
                },
                hkask_types::inference_ipc::DecisionLevelProbability {
                    level: 2,
                    probability: 0.21,
                },
            ],
            confidence: 0.69,
        };
        assert_eq!(answer_level(&answer), Some(1));
        let levelless = hkask_types::inference_ipc::DecisionAnswer {
            id: "q".to_string(),
            score: 0.0,
            level_probabilities: Vec::new(),
            confidence: 0.0,
        };
        assert_eq!(
            answer_level(&levelless),
            None,
            "a levelless answer maps to missing, never a fabricated level"
        );
    }

    #[tokio::test]
    async fn private_provenance_refuses_before_any_call() {
        // The `with_call_provenance` seam sets the dispatch scope (the
        // production scope is set only by the dispatch decorator, from
        // the parent's `_meta` — never by tool code).
        let error = hkask_mcp_server::server::with_call_provenance(
            hkask_types::InferenceProvenance::PrivateInference,
            score_rationale(&RefusingPort, &request("rationale text")),
        )
        .await
        .expect_err("private-provenance calls refuse");
        assert!(matches!(error, EqmError::PrivateProvenanceRefused));
    }

    #[tokio::test]
    async fn unset_decision_model_refuses_naming_the_setting() {
        // The test binary carries no HKASK_DECISION_MODEL (the settings
        // chain injects it into production children); the public default
        // provenance reaches the model resolution, which refuses typed.
        let error = score_rationale(&RefusingPort, &request("rationale text"))
            .await
            .expect_err("unset decision model refuses");
        match error {
            EqmError::DecisionNotConfigured(message) => {
                assert!(message.contains("kask.models.decision_model"));
                assert!(message.contains("HKASK_DECISION_MODEL"));
            }
            other => panic!("expected DecisionNotConfigured, got {other:?}"),
        }
    }
}
