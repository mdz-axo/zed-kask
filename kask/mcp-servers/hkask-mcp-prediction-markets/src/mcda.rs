//! MCDA weight-and-score semantic halves — criterion weights and
//! per-alternative raw scores — routed through the decision model.
//!
//! The second consumer of the Decisions API (after `eqm`): the mcda
//! skill's weight-and-score step formerly rendered the weight-and-score
//! chat template and typed the judgments on the thread model. The
//! operator's 2026-11-07 verdict (card c8a2ffaf) moved the semantic
//! elicitation to `kask.models.decision_model` (env HKASK_DECISION_MODEL,
//! default OpenRouter/typesafe/jev-1.14 — a Jev-class System One model
//! served over OpenRouter's Decisions API, never a chat endpoint): ONE
//! call carrying typed Score questions over the FIXED 1-10 answer space —
//! one weight question per criterion (its importance for the decision
//! under the named weighting method), one score question per
//! (alternative, criterion) with the cost-criterion pre-inversion stated
//! in the question text (the skill's convention: cost 1 = most
//! costly/worst, 10 = least costly/best). TEN levels per question, no
//! more: the provider's Score `criteria` accept AT MOST 10 items — the
//! former 11-level 0-10 space was rejected 422 by the live endpoint on
//! every measurement run (the length pin in the tests closes that gap).
//! The judgment per question is the chosen level's NUMBER in that 1-10
//! space: the wire keys probabilities by 0-based index into the criteria
//! (DecisionLevelProbability's own contract; the eqm probe's legend and
//! probability-weighted score field both confirm index keying), so the
//! judgment is the argmax level plus one (the eqm extraction plus the
//! renumbering).
//!
//! The output is the shape the former template produced —
//! `weights: [{criterion, weight}]` plus `raw_scores: [{alternative,
//! criterion, raw_score}]` — so the skill's deterministic halves (the
//! `(norm W)`, `(dot (norm W) <alt>)` composite, ranking, MCDA-2 and
//! min_flip lisp gates) consume it unchanged. Normalization, composites
//! and ranking are NOT computed here: they stay `lisp_eval`'s (S14); this
//! module only elicits the bounded judgments (S15).
//!
//! Refusals (the eqm pins): an unset decision model is a typed error
//! naming `kask.models.decision_model` — never the chat model; a
//! private-provenance call refuses before any dispatch (the decision
//! model is cloud-served). A question the provider omitted or answered
//! without levels is a typed refusal naming it — never a fabricated zero
//! weight or score: eqm tolerates missing markers by pulling its
//! composite toward zero (a screen's degradation), but a fabricated
//! judgment here would silently corrupt the ranking the lisp gates
//! compute from these numbers.

use hkask_types::InferencePort;
use serde::Deserialize;

// ── Request types ──────────────────────────────────────────────────────────

/// benefit | cost — the mcda skill's criterion classification
/// (identify-criteria step 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CriterionType {
    /// Higher values are better.
    Benefit,
    /// Lower raw values are better; the score question carries the
    /// pre-inverted rubric (0 = most costly/worst, 10 = least
    /// costly/best) so the weighted-sum model consumes one direction.
    Cost,
}

/// One decision criterion — the identify-criteria phase's structured
/// output, passed through. Extra identify-criteria fields (e.g.
/// `measurement_type`) are ignored.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McdaCriterion {
    /// The criterion's name.
    pub name: String,
    /// The criterion's classification: "benefit" or "cost".
    pub r#type: CriterionType,
    /// What the criterion measures and how it differentiates the
    /// alternatives.
    pub description: String,
}

/// One decision alternative.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct McdaAlternative {
    /// The alternative's name.
    pub name: String,
    /// Distinguishing context for the alternative (may be empty when the
    /// name alone carries the meaning).
    pub description: String,
}

/// The mcda weight-and-score request — the identify-criteria pass-through
/// the skill's weight-and-score step forwards.
#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WeightScoreRequest {
    /// The decision question the alternatives answer.
    pub decision_question: String,
    /// The decision criteria (the skill layer enforces 3-12 via MCDA-1;
    /// the tool only refuses an empty set).
    pub criteria: Vec<McdaCriterion>,
    /// The alternatives being weighted and scored.
    pub alternatives: Vec<McdaAlternative>,
    /// The weighting method: "direct" or "swing" (Keeney & Raiffa swing
    /// weighting — all criteria imagined at their worst level, the
    /// weight the swing in overall value).
    pub weighting_method: String,
}

// ── Result types ──────────────────────────────────────────────────────────

/// One criterion's elicited weight — the chosen level's number from the
/// weight question over the fixed 1-10 importance space.
#[derive(Debug, serde::Serialize)]
pub struct CriterionWeight {
    pub criterion: String,
    pub weight: u8,
}

/// One alternative's raw score on one criterion — the chosen level's
/// number from the score question over the fixed 1-10 performance space
/// (cost criteria pre-inverted).
#[derive(Debug, serde::Serialize)]
pub struct RawScore {
    pub alternative: String,
    pub criterion: String,
    pub raw_score: u8,
}

/// The elicitation result — the shape the former weight-and-score
/// template produced, consumed unchanged by the skill's lisp gates.
#[derive(Debug, serde::Serialize)]
pub struct WeightScoreResult {
    /// One per criterion, in request order.
    pub weights: Vec<CriterionWeight>,
    /// One per (alternative, criterion), alternative-major in request
    /// order.
    pub raw_scores: Vec<RawScore>,
}

// ── Error type ─────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum McdaError {
    #[error("inference failed: {0}")]
    InferenceFailed(String),
    #[error("invalid weight-and-score request: {0}")]
    InvalidRequest(String),
    #[error("decision model not configured: {0}")]
    DecisionNotConfigured(String),
    #[error(
        "private-marked context refused: the decision model is cloud-served and private content never crosses to it — no local decision model exists yet; weight and score on a public thread"
    )]
    PrivateProvenanceRefused,
    #[error(
        "decision answers missing or levelless for question ids {ids:?} — refusing rather than fabricating weights or scores"
    )]
    MissingJudgment { ids: Vec<String> },
}

impl From<McdaError> for hkask_mcp_server::server::McpToolError {
    fn from(e: McdaError) -> Self {
        use hkask_mcp_server::server::McpToolError;
        match e {
            McdaError::InvalidRequest(_) => McpToolError::invalid_argument(e.to_string()),
            // The NotConfigured refusal names the setting and the private
            // refusal names the boundary (the eqm precedent —
            // permission_denied, never a silent chat-model fallback); a
            // missing judgment is a failed measurement, not a permission.
            McdaError::DecisionNotConfigured(_) | McdaError::PrivateProvenanceRefused => {
                McpToolError::permission_denied(e.to_string())
            }
            McdaError::InferenceFailed(_) | McdaError::MissingJudgment { .. } => {
                McpToolError::unavailable(e.to_string())
            }
        }
    }
}

// ── Decision request construction ──────────────────────────────────────────

/// The provider's Score `criteria` accept AT MOST 10 items — the request
/// validation that rejected the 11-level 0-10 space live (a typed 422 on
/// every measurement run, at `loc body.questions.*.score.criteria`). The
/// answer space is therefore the natural TEN-level 1-10 scale, and every
/// question's level list is exactly this long (pinned by test).
const LEVELS_PER_QUESTION: usize = 10;

/// The fixed 1-10 importance answer space for weight questions (ten
/// ordered level descriptions, least to most) — the weighting rubric the
/// former template stated inline, now the decision question's criteria.
const WEIGHT_LEVELS: [&str; LEVELS_PER_QUESTION] = [
    "1 — no importance: the criterion should not influence the decision",
    "2 — very low importance",
    "3 — low importance",
    "4 — somewhat low importance",
    "5 — average importance",
    "6 — somewhat high importance",
    "7 — above-average importance",
    "8 — high importance",
    "9 — very high importance",
    "10 — decisive importance: the criterion should dominate the decision",
];

/// The fixed 1-10 performance answer space for score questions (ten
/// ordered level descriptions, least to most). Cost criteria read
/// through the pre-inverted rubric the question text states (1 = most
/// costly/worst, 10 = least costly/best), so the levels stay
/// performance-generic.
const SCORE_LEVELS: [&str; LEVELS_PER_QUESTION] = [
    "1 — worst possible performance on this criterion",
    "2 — very poor performance",
    "3 — poor performance",
    "4 — somewhat poor performance",
    "5 — average performance",
    "6 — somewhat strong performance",
    "7 — above-average performance",
    "8 — strong performance",
    "9 — very strong performance",
    "10 — best possible performance on this criterion",
];

/// The swing weighting rubric (Keeney & Raiffa) — rides every swing
/// weight question, the same sentences the former template carried.
const SWING_RUBRIC: &str = "imagine every criterion at its worst level; the criterion whose improvement from worst to best provides the greatest swing in overall value gets the highest weight";

/// Build the decision state — the same context the former template
/// carried (the decision question, the weighting method, the
/// alternatives, the criteria), now the `state` a decision model
/// evaluates typed questions against.
fn build_decision_state(req: &WeightScoreRequest) -> String {
    let mut state = format!(
        "Decision question: {}\n\nWeighting method: {} weighting\n\nAlternatives:\n",
        req.decision_question, req.weighting_method
    );
    for (i, alt) in req.alternatives.iter().enumerate() {
        state.push_str(&format!("{}. {} — {}\n", i + 1, alt.name, alt.description));
    }
    state.push_str("\nCriteria:\n");
    for (i, criterion) in req.criteria.iter().enumerate() {
        let kind = match criterion.r#type {
            CriterionType::Benefit => "benefit",
            CriterionType::Cost => "cost",
        };
        state.push_str(&format!(
            "{}. {} ({}) — {}\n",
            i + 1,
            criterion.name,
            kind,
            criterion.description
        ));
    }
    state
}

/// One typed Score question per bounded judgment, all carried by ONE
/// Decisions API call: per criterion a weight question (its importance
/// for the decision under the named weighting method), per
/// (alternative, criterion) a score question with the cost-criterion
/// pre-inversion stated in the question text. Ids are positional
/// (`w{i}`, `s{a}_{c}`) — the answer returns under the caller's key and
/// the result maps back to names by position, so duplicate names cannot
/// corrupt the join.
fn decision_questions(
    req: &WeightScoreRequest,
) -> Vec<hkask_types::inference_ipc::DecisionQuestion> {
    use hkask_types::inference_ipc::DecisionQuestion;
    let mut questions = Vec::with_capacity(req.criteria.len() * (1 + req.alternatives.len()));
    for (i, criterion) in req.criteria.iter().enumerate() {
        let kind = match criterion.r#type {
            CriterionType::Benefit => "a benefit criterion",
            CriterionType::Cost => "a cost criterion",
        };
        let instructions = match req.weighting_method.as_str() {
            "swing" => format!(
                "Assign the swing weight of criterion \"{}\" ({}; {}) for this decision. {} — score this criterion's importance by the swing in overall value its improvement from worst to best provides. 1 = no importance, 10 = decisive importance.",
                criterion.name, kind, criterion.description, SWING_RUBRIC
            ),
            _ => format!(
                "Assign the direct importance weight of criterion \"{}\" ({}; {}) for this decision — its relative importance compared to the other criteria, reflecting how much it should influence the choice. 1 = no importance, 10 = decisive importance.",
                criterion.name, kind, criterion.description
            ),
        };
        questions.push(DecisionQuestion {
            id: format!("w{i}"),
            instructions,
            criteria: WEIGHT_LEVELS.iter().map(|s| s.to_string()).collect(),
        });
    }
    for (a, alt) in req.alternatives.iter().enumerate() {
        for (c, criterion) in req.criteria.iter().enumerate() {
            let instructions = match criterion.r#type {
                CriterionType::Benefit => format!(
                    "Score alternative \"{}\" on criterion \"{}\" — a benefit criterion ({}): higher values are better. 1 = worst possible performance, 10 = best possible performance.",
                    alt.name, criterion.name, criterion.description
                ),
                CriterionType::Cost => format!(
                    "Score alternative \"{}\" on criterion \"{}\" — a cost criterion ({}), pre-inverted for the weighted-sum model: 1 = most costly (worst), 10 = least costly (best).",
                    alt.name, criterion.name, criterion.description
                ),
            };
            questions.push(DecisionQuestion {
                id: format!("s{a}_{c}"),
                instructions,
                criteria: SCORE_LEVELS.iter().map(|s| s.to_string()).collect(),
            });
        }
    }
    questions
}

/// Map a decision answer to the most probable level (argmax over the
/// calibrated per-level probabilities) — the eqm extraction. The wire
/// keys probabilities by 0-based index into the question's criteria
/// (DecisionLevelProbability's own contract; the eqm probe's legend and
/// probability-weighted score field both confirm it), so this returns
/// the INDEX, not the level's number in the 1-10 rubric — `usable_level`
/// owns that renumbering. Never a fabricated value: a levelless answer
/// reads `None` and the caller refuses naming the question.
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

/// The usable judgment for one question id: the answer present, with
/// levels, and inside the question's ten-item criteria list — mapped
/// from the wire's 0-based level index to the chosen level's NUMBER in
/// the 1-10 rubric (index + 1, the renumbering the ten-level answer
/// space requires; pinned by test).
fn usable_level(batch: &hkask_types::ports::DecisionBatch, id: &str) -> Option<u8> {
    batch
        .answers
        .iter()
        .find(|answer| answer.id == id)
        .and_then(answer_level)
        .filter(|level| (*level as usize) < LEVELS_PER_QUESTION)
        .map(|level| level + 1)
}

// ── Request validation ─────────────────────────────────────────────────────

/// Fail-visible request validation, before any provenance check or
/// dispatch: the weighting method must be one the rubric carries (a
/// made-up method would silently weight directly — a fabricated method
/// choice on a decision artifact), and the names must be non-blank.
fn validate(req: &WeightScoreRequest) -> Result<(), McdaError> {
    if req.decision_question.trim().is_empty() {
        return Err(McdaError::InvalidRequest(
            "decision_question is empty".to_string(),
        ));
    }
    if !matches!(req.weighting_method.as_str(), "direct" | "swing") {
        return Err(McdaError::InvalidRequest(format!(
            "weighting_method must be \"direct\" or \"swing\", got {:?}",
            req.weighting_method
        )));
    }
    if req.criteria.is_empty() {
        return Err(McdaError::InvalidRequest(
            "criteria is empty — the skill layer enforces 3-12 (MCDA-1)".to_string(),
        ));
    }
    if req.alternatives.is_empty() {
        return Err(McdaError::InvalidRequest(
            "alternatives is empty".to_string(),
        ));
    }
    for (i, criterion) in req.criteria.iter().enumerate() {
        if criterion.name.trim().is_empty() {
            return Err(McdaError::InvalidRequest(format!(
                "criterion {i} has an empty name"
            )));
        }
    }
    for (i, alt) in req.alternatives.iter().enumerate() {
        if alt.name.trim().is_empty() {
            return Err(McdaError::InvalidRequest(format!(
                "alternative {i} has an empty name"
            )));
        }
    }
    Ok(())
}

// ── Scoring function ───────────────────────────────────────────────────────

/// Weight the criteria and score the alternatives for an MCDA decision
/// using the decision model (`kask.models.decision_model`).
///
/// Sends the decision context as the state plus one typed score question
/// per bounded judgment (the Decisions API: the model returns a
/// calibrated probability distribution over each question's fixed
/// ten-level 1-10 scale, never text); the chosen level's number is the
/// judgment. Normalization, composites, ranking and sensitivity stay
/// deterministic in the skill's `lisp_eval` gates. An unset decision
/// model is a typed refusal naming
/// the setting — never a chat-model fallback; a private-provenance call
/// refuses at the boundary (the decision model is cloud-served; private
/// content never crosses).
pub async fn weight_score(
    inference_port: &dyn InferencePort,
    req: &WeightScoreRequest,
) -> Result<WeightScoreResult, McdaError> {
    validate(req)?;

    // The boundary guard (the D100 forward local-routing rule's tool-call
    // leg, the eqm pattern): the configured decision model is
    // cloud-served, so a private-provenance call refuses here — it never
    // crosses.
    if matches!(
        hkask_mcp_server::server::current_call_provenance(),
        hkask_types::InferenceProvenance::PrivateInference
    ) {
        return Err(McdaError::PrivateProvenanceRefused);
    }

    let model = hkask_inference::model_constants::resolve_decision_model()
        .map_err(|e| McdaError::DecisionNotConfigured(e.to_string()))?;

    let state = build_decision_state(req);
    let questions = decision_questions(req);

    let batch = inference_port
        .decide(&model, &state, &questions)
        .await
        .map_err(|e| McdaError::InferenceFailed(e.to_string()))?;

    // mcp-tool-review S-01 (token half): surface the measured per-call
    // usage in the tool envelope so the billed-token axis is observable
    // at the caller (the eqm precedent; this and the eqm call are the
    // server's two inference calls).
    hkask_mcp_server::server::record_tool_usage(batch.usage.clone());

    assemble_result(req, &batch)
}

/// Extract the judgments from the decision batch: per question the argmax
/// level, refused (naming every unusable id at once) when the provider
/// omitted an answer or answered without levels — never a fabricated
/// zero weight or score.
fn assemble_result(
    req: &WeightScoreRequest,
    batch: &hkask_types::ports::DecisionBatch,
) -> Result<WeightScoreResult, McdaError> {
    let mut missing = Vec::new();

    let mut weights = Vec::with_capacity(req.criteria.len());
    for (i, criterion) in req.criteria.iter().enumerate() {
        let id = format!("w{i}");
        match usable_level(batch, &id) {
            Some(weight) => weights.push(CriterionWeight {
                criterion: criterion.name.clone(),
                weight,
            }),
            None => missing.push(id),
        }
    }

    let mut raw_scores = Vec::with_capacity(req.alternatives.len() * req.criteria.len());
    for (a, alt) in req.alternatives.iter().enumerate() {
        for (c, criterion) in req.criteria.iter().enumerate() {
            let id = format!("s{a}_{c}");
            match usable_level(batch, &id) {
                Some(raw_score) => raw_scores.push(RawScore {
                    alternative: alt.name.clone(),
                    criterion: criterion.name.clone(),
                    raw_score,
                }),
                None => missing.push(id),
            }
        }
    }

    if !missing.is_empty() {
        tracing::warn!(
            target: "hkask.mcp.prediction_markets.mcda",
            missing = ?missing,
            "decision answers missing or levelless — refusing rather than fabricating weights or scores"
        );
        return Err(McdaError::MissingJudgment { ids: missing });
    }

    Ok(WeightScoreResult {
        weights,
        raw_scores,
    })
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A port whose every generate call is unreachable — the refusal
    /// paths under test must never reach it (validation, the private
    /// guard and the unset-model resolution all precede any port call).
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

    fn request() -> WeightScoreRequest {
        WeightScoreRequest {
            decision_question: "Choose the database for the analytics service".to_string(),
            criteria: vec![
                McdaCriterion {
                    name: "query latency".to_string(),
                    r#type: CriterionType::Benefit,
                    description: "p99 read latency under load".to_string(),
                },
                McdaCriterion {
                    name: "operating cost".to_string(),
                    r#type: CriterionType::Cost,
                    description: "monthly fully-loaded cost".to_string(),
                },
            ],
            alternatives: vec![
                McdaAlternative {
                    name: "postgres".to_string(),
                    description: "managed Postgres".to_string(),
                },
                McdaAlternative {
                    name: "duckdb".to_string(),
                    description: "embedded analytics DB".to_string(),
                },
            ],
            weighting_method: "swing".to_string(),
        }
    }

    /// An answer whose argmax sits at the 0-based criteria INDEX
    /// `argmax_index` (the wire's keying), with a second calibrated level
    /// inside the ten-item list so the argmax is a real extraction.
    fn answer(id: &str, argmax_index: u8) -> hkask_types::inference_ipc::DecisionAnswer {
        hkask_types::inference_ipc::DecisionAnswer {
            id: id.to_string(),
            score: argmax_index as f64,
            level_probabilities: vec![
                hkask_types::inference_ipc::DecisionLevelProbability {
                    level: argmax_index,
                    probability: 0.8,
                },
                hkask_types::inference_ipc::DecisionLevelProbability {
                    level: if argmax_index == 9 {
                        0
                    } else {
                        argmax_index + 1
                    },
                    probability: 0.2,
                },
            ],
            confidence: 0.8,
        }
    }

    fn batch(
        answers: Vec<hkask_types::inference_ipc::DecisionAnswer>,
    ) -> hkask_types::ports::DecisionBatch {
        hkask_types::ports::DecisionBatch {
            answers,
            model: "typesafe/jev-1.14-20261014".to_string(),
            usage: hkask_types::ports::InferenceUsage {
                prompt_tokens: 354,
                completion_tokens: 17,
                total_tokens: 371,
                reported: true,
            },
        }
    }

    #[test]
    fn decision_questions_carry_the_fixed_1_10_answer_space() {
        let req = request();
        let questions = decision_questions(&req);
        // 2 weight questions (one per criterion) + 2×2 score questions
        // (one per alternative × criterion), all in ONE call.
        assert_eq!(questions.len(), 6);
        let mut ids = Vec::new();
        for question in &questions {
            assert!(!ids.contains(&question.id), "ids must be unique");
            ids.push(question.id.clone());
            // THE CAP PIN: the provider's Score `criteria` accept AT MOST
            // 10 items — the 11-level 0-10 space was rejected 422 live on
            // every measurement run (card c8a2ffaf follow-up). The unit
            // tests mock the wire, so the provider's request validation
            // never runs here; this length assertion is the mechanical
            // stand-in for it.
            assert!(
                question.criteria.len() <= 10,
                "the provider rejects criteria lists over 10 items (live 422); got {}",
                question.criteria.len()
            );
            assert_eq!(
                question.criteria.len(),
                LEVELS_PER_QUESTION,
                "the answer space is the fixed ten-level 1-10 scale"
            );
        }

        let weight_questions: Vec<_> = questions.iter().take(2).collect();
        for (question, criterion) in weight_questions.iter().zip(req.criteria.iter()) {
            assert_eq!(
                question.criteria,
                WEIGHT_LEVELS
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
                "weight questions carry the fixed 1-10 importance space"
            );
            assert!(
                question.criteria[0].starts_with("1 — no importance"),
                "the scale starts at level 1"
            );
            assert!(
                question.criteria[LEVELS_PER_QUESTION - 1].starts_with("10 — decisive importance"),
                "the scale tops out at level 10"
            );
            assert!(
                question.instructions.contains(&criterion.name),
                "the criterion's name rides the weight question"
            );
            assert!(
                question.instructions.contains(SWING_RUBRIC),
                "the named weighting method's rubric rides the question (swing)"
            );
        }

        let score_questions: Vec<_> = questions.iter().skip(2).collect();
        for question in &score_questions {
            assert_eq!(
                question.criteria,
                SCORE_LEVELS
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>(),
                "score questions carry the fixed 1-10 performance space"
            );
            assert!(
                question.criteria[LEVELS_PER_QUESTION - 1].starts_with("10 — best possible"),
                "the scale tops out at level 10"
            );
        }
        let benefit = &score_questions[0]; // s0_0: postgres × query latency
        assert!(
            benefit.instructions.contains("query latency")
                && benefit.instructions.contains("postgres")
                && benefit.instructions.contains("higher values are better")
                && benefit
                    .instructions
                    .contains("1 = worst possible performance, 10 = best possible performance"),
            "benefit questions state the 1-10 benefit rubric: {benefit:?}"
        );
        let cost = &score_questions[1]; // s0_1: postgres × operating cost
        assert!(
            cost.instructions.contains("operating cost")
                && cost.instructions.contains("pre-inverted")
                && cost
                    .instructions
                    .contains("1 = most costly (worst), 10 = least costly (best)"),
            "cost questions state the pre-inversion in the question text: {cost:?}"
        );
    }

    #[test]
    fn answer_level_takes_the_most_probable_level() {
        // The documented Jev score shape (research run b1f1663057722f5d):
        // probabilities {0: 0.0, 1: 0.79, 2: 0.21} → the argmax level is 1.
        let answered = hkask_types::inference_ipc::DecisionAnswer {
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
        assert_eq!(answer_level(&answered), Some(1));
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

    #[test]
    fn usable_level_maps_the_wire_index_to_the_level_number() {
        // The wire keys probabilities by 0-based index into the criteria;
        // the judgment is the chosen level's NUMBER in the 1-10 rubric
        // (the renumbering pinned here — the flip point if a live answer
        // ever arrives keyed 1-10 instead).
        let answered = batch(vec![answer("w0", 7), answer("w1", 0), answer("w9", 9)]);
        assert_eq!(usable_level(&answered, "w0"), Some(8), "index 7 is level 8");
        assert_eq!(usable_level(&answered, "w1"), Some(1), "index 0 is level 1");
        assert_eq!(
            usable_level(&answered, "w9"),
            Some(10),
            "index 9 is level 10"
        );
        // An index outside the ten-item criteria list is unusable — never
        // a fabricated level 11.
        let out_of_range = batch(vec![hkask_types::inference_ipc::DecisionAnswer {
            id: "w_out".to_string(),
            score: 0.0,
            level_probabilities: vec![hkask_types::inference_ipc::DecisionLevelProbability {
                level: 10,
                probability: 1.0,
            }],
            confidence: 1.0,
        }]);
        assert_eq!(
            usable_level(&out_of_range, "w_out"),
            None,
            "a level index at/after the ten-item list is unusable, never level 11"
        );
        assert_eq!(usable_level(&out_of_range, "absent"), None);
    }

    #[tokio::test]
    async fn private_provenance_refuses_before_any_call() {
        // The `with_call_provenance` seam sets the dispatch scope (the
        // production scope is set only by the dispatch decorator, from
        // the parent's `_meta` — never by tool code).
        let error = hkask_mcp_server::server::with_call_provenance(
            hkask_types::InferenceProvenance::PrivateInference,
            weight_score(&RefusingPort, &request()),
        )
        .await
        .expect_err("private-provenance calls refuse");
        assert!(matches!(error, McdaError::PrivateProvenanceRefused));
    }

    #[tokio::test]
    async fn unset_decision_model_refuses_naming_the_setting() {
        // The test binary carries no HKASK_DECISION_MODEL (the settings
        // chain injects it into production children); the public default
        // provenance reaches the model resolution, which refuses typed.
        let error = weight_score(&RefusingPort, &request())
            .await
            .expect_err("unset decision model refuses");
        match error {
            McdaError::DecisionNotConfigured(message) => {
                assert!(message.contains("kask.models.decision_model"));
                assert!(message.contains("HKASK_DECISION_MODEL"));
            }
            other => panic!("expected DecisionNotConfigured, got {other:?}"),
        }
    }

    #[test]
    fn missing_judgments_refuse_rather_than_zero_fill() {
        let req = request();
        // Every weight answers; one score is absent entirely (s0_1) and
        // one is levelless (s1_0) — both must refuse, never zero-fill.
        // Wire indices are one below the judgments (index 7 answers
        // level 8) — the renumbering mapped end-to-end.
        let mut answers = vec![answer("w0", 7), answer("w1", 2), answer("s0_0", 5)];
        answers.push(hkask_types::inference_ipc::DecisionAnswer {
            id: "s1_0".to_string(),
            score: 0.0,
            level_probabilities: Vec::new(),
            confidence: 0.0,
        });
        answers.push(answer("s1_1", 3));
        let result = assemble_result(&req, &batch(answers))
            .expect_err("missing or levelless answers refuse");
        match result {
            McdaError::MissingJudgment { ids } => {
                assert_eq!(ids, vec!["s0_1".to_string(), "s1_0".to_string()]);
            }
            other => panic!("expected MissingJudgment, got {other:?}"),
        }
    }

    #[test]
    fn assembled_output_shape_matches_the_former_template() {
        let req = request();
        // Wire indices are one below the judgments (index 7 answers
        // level 8) — the renumbering mapped end-to-end.
        let answers = vec![
            answer("w0", 7),
            answer("w1", 2),
            answer("s0_0", 5),
            answer("s0_1", 8),
            answer("s1_0", 1),
            answer("s1_1", 3),
        ];
        let result = assemble_result(&req, &batch(answers)).expect("complete batch assembles");
        for weight in &result.weights {
            assert!(
                (1..=10).contains(&weight.weight),
                "weights are level numbers in 1-10"
            );
        }
        for raw in &result.raw_scores {
            assert!(
                (1..=10).contains(&raw.raw_score),
                "raw scores are level numbers in 1-10"
            );
        }
        // The exact shape the former template produced — keys, order and
        // no extras — so the lisp halves and downstream skill steps
        // consume it unchanged.
        assert_eq!(
            serde_json::to_value(&result).expect("result serializes"),
            serde_json::json!({
                "weights": [
                    {"criterion": "query latency", "weight": 8},
                    {"criterion": "operating cost", "weight": 3},
                ],
                "raw_scores": [
                    {"alternative": "postgres", "criterion": "query latency", "raw_score": 6},
                    {"alternative": "postgres", "criterion": "operating cost", "raw_score": 9},
                    {"alternative": "duckdb", "criterion": "query latency", "raw_score": 2},
                    {"alternative": "duckdb", "criterion": "operating cost", "raw_score": 4},
                ],
            }),
            "weights in criterion order, raw_scores alternative-major in request order"
        );
    }

    #[tokio::test]
    async fn invalid_requests_refuse_before_any_dispatch() {
        let mut req = request();
        req.weighting_method = "weighted".to_string();
        let error = weight_score(&RefusingPort, &req)
            .await
            .expect_err("an unknown weighting method refuses");
        assert!(
            matches!(error, McdaError::InvalidRequest(_)),
            "got {error:?}"
        );

        let mut req = request();
        req.criteria.clear();
        let error = weight_score(&RefusingPort, &req)
            .await
            .expect_err("empty criteria refuse");
        assert!(
            matches!(error, McdaError::InvalidRequest(_)),
            "got {error:?}"
        );

        let mut req = request();
        req.decision_question = "  ".to_string();
        let error = weight_score(&RefusingPort, &req)
            .await
            .expect_err("a blank decision question refuses");
        assert!(
            matches!(error, McdaError::InvalidRequest(_)),
            "got {error:?}"
        );
    }
}
