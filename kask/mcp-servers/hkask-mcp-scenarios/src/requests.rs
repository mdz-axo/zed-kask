use hkask_mcp_server::AnyJsonValue;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::types::{Perspective, ScenarioEvent, SubQuestion};

// ── Request types for MCP tools ────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BuildEventsRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Time horizon for the scenario
    pub time_horizon: Option<String>,
    /// Scenario type
    pub scenario_type: Option<String>,
    /// Natural language context about the subject
    pub context: Option<String>,
    /// Maximum number of events to generate
    pub max_events: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BrainstormRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Time horizon: "tactical", "strategic", or "long_term"
    pub time_horizon: Option<String>,
    /// Raw text from web searches about this subject
    pub research_context: Option<String>,
    /// Persona names to use (e.g., 'Bull,Bear,Contrarian'). Empty = use defaults.
    pub personas: Option<String>,
    /// Start at a specific round (1-4). Default: 1 (full protocol).
    pub start_round: Option<u8>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FrameRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Optional: pre-populated answers from a previous framing session, as a
    /// JSON object. Typed as [`AnyJsonValue`] so the generated tool input
    /// schema is the empty object `{}` rather than the bare boolean `true`
    /// schemars emits for `serde_json::Value`.
    pub prior_answers: Option<AnyJsonValue>,
}

/// Request to structure a completed framing conversation into a FramingDocument.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FrameDocumentRequest {
    /// Subject for the scenario project
    pub subject: String,
    /// JSON object with answers from the 7-turn framing conversation.
    /// Expected keys: focal_question, decision_at_stake, time_horizon,
    /// action_deadline, in_scope, out_of_scope, stakeholders, use_case,
    /// success_criteria, constraints, surfaced_assumptions, exploration_prompts.
    ///
    /// Typed as [`AnyJsonValue`] so the generated tool input schema is the
    /// empty object `{}` rather than the bare boolean `true` schemars emits
    /// for `serde_json::Value`.
    pub answers: AnyJsonValue,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MarketsBridgeRequest {
    /// An annotated MarketRecord from hkask-mcp-prediction-markets
    /// (market_lookup or market_match output; for market_match, pass the
    /// nested `market` object and set match_confidence). Typed as
    /// [`AnyJsonValue`] because `MarketRecord` is defined in the
    /// prediction-markets crate and does not derive `JsonSchema`; the tool
    /// body deserializes it into the typed struct.
    pub market_record: AnyJsonValue,
    /// Match confidence from market_match ("high"/"medium"/"low") — omit when
    /// the caller resolved the market unambiguously (e.g. direct lookup).
    pub match_confidence: Option<String>,
}

/// One dependency edge for `scenario_from_markets_set`.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DependencySpecRequest {
    /// Child market id (the `market_id` of the conditioned market).
    pub child_market_id: String,
    /// Parent market ids (the conditioning markets).
    pub parent_market_ids: Vec<String>,
    /// P(child | parent truth assignment), bitmap-ordered; length must be
    /// 2^parent_market_ids.len(). Caller-authored — the server computes
    /// marginals but never invents conditional probabilities.
    pub conditionals: Vec<f64>,
}

/// Request for `scenario_propagate`: update one event's prior and recompute
/// the whole tree (T5).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PropagateRequest {
    /// The current tree's events (e.g. from a prior
    /// scenario_from_markets_set / scenario_quantify output).
    pub events: Vec<ScenarioEvent>,
    /// ID of the event whose prior is being revised.
    pub event_id: String,
    /// The new prior probability in [0, 1].
    pub new_prior: f64,
}

/// Request for contract_price_coherence (R5 / H3 reframed).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContractCoherenceRequest {
    /// Observed market price of the contract on the same events — a
    /// parlay/joint contract for a joint comparison, or a single contract's
    /// price for a marginal comparison. Must be in [0, 1].
    pub market_price: f64,
    /// Transaction-cost band: the sum of bid-ask spreads, fees, and slippage
    /// for both legs of the arbitrage. Divergences within the band are not
    /// actionable.
    pub cost_band: f64,
    /// Tree-implied joint probability in [0, 1]. When omitted, the cached
    /// tree's joint_probability is used (from scenario_quantify,
    /// scenario_from_cmp_indices, or scenario_propagate).
    pub tree_implied: Option<f64>,
}

/// Request for `scenario_from_markets_set`: compose N market records into a
/// dependent event tree.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MarketsSetBridgeRequest {
    /// Annotated MarketRecords from hkask-mcp-prediction-markets. Typed as
    /// [`AnyJsonValue`] because `MarketRecord` is defined in the
    /// prediction-markets crate and does not derive `JsonSchema`.
    pub market_records: AnyJsonValue,
    /// Optional per-record match confidences ("high"/"medium"/"low" or null),
    /// parallel to the records array. Omit for direct lookups.
    pub match_confidences: Option<Vec<Option<String>>>,
    /// Caller-authored dependency edges. Omit for a flat (independent) tree.
    pub dependency_specs: Option<Vec<DependencySpecRequest>>,
}

/// One dependency edge for `scenario_from_cmp_indices`. Uses CMP index IDs
/// (`cmp:{family}:{tenor}:{orientation}`) instead of market IDs.
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CmpDependencySpecRequest {
    /// The child CMP index ID: `cmp:{family}:{tenor}:{orientation}`.
    pub child_id: String,
    /// The parent CMP index IDs.
    pub parent_ids: Vec<String>,
    /// P(child | parent truth assignment), bitmap-ordered; length must be
    /// 2^parent_ids.len(). Caller-authored.
    pub conditionals: Vec<f64>,
}

/// Request for `scenario_from_cmp_indices`: compose CMP indices into an
/// EventTree with optional dependency edges (R1).
#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CmpBridgeRequest {
    /// ProvenancedCmpIndex objects from hkask-mcp-prediction-markets
    /// (build_cmp_indices output). Typed as [`AnyJsonValue`] because
    /// `ProvenancedCmpIndex` is defined in the prediction-markets crate and
    /// does not derive `JsonSchema`.
    pub cmp_indices: AnyJsonValue,
    /// The observation date (YYYY-MM-DD) the CMP indices were built. The event
    /// deadlines are observation_date + target_maturity_days.
    pub observation_date: String,
    /// Optional caller-authored dependency edges between CMP index IDs. Omit
    /// for a flat (independent) tree.
    pub dependency_specs: Option<Vec<CmpDependencySpecRequest>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FullPipelineRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Events (from scenario_brainstorm or manual construction)
    pub events: Vec<ScenarioEvent>,
    /// Optional: perspectives for dragonfly-eye synthesis
    pub perspectives: Option<Vec<Perspective>>,
    /// Optional: project-level metadata for assessment
    pub perspective_count: Option<usize>,
    /// Optional: how many strategies were generated from the scenarios
    pub strategies_generated: Option<usize>,
    /// Optional: how many strategies were actually implemented
    pub strategies_implemented: Option<usize>,
    /// Optional: learning events, newline-separated
    pub learning_events: Option<String>,
    /// Optional: whether early-warning indicators were defined
    pub has_early_warning_indicators: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CrossValidateRequest {
    /// Event or question identifier
    pub event_id: String,
    /// Label for the first estimate source (e.g., 'superforecasting_skill')
    pub source_a: String,
    /// First probability estimate (0.0-1.0)
    pub estimate_a: f64,
    /// Fermi sub-questions for estimate A
    pub sub_questions_a: Vec<SubQuestion>,
    /// Label for the second estimate source (e.g., 'scenario_calibrate')
    pub source_b: String,
    /// Second probability estimate (0.0-1.0)
    pub estimate_b: f64,
    /// Fermi sub-questions for estimate B
    pub sub_questions_b: Vec<SubQuestion>,
    /// Review threshold (default 0.15). Divergence above this triggers review.
    pub review_threshold: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuantifyRequest {
    /// Events to quantify
    pub events: Vec<ScenarioEvent>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UpdateRequest {
    /// Forecast record ID
    pub forecast_id: String,
    /// Event ID being updated
    pub event_id: String,
    /// Current calibrated probability (prior)
    pub prior_probability: f64,
    /// P(evidence | hypothesis is true)
    pub evidence_likelihood: f64,
    /// P(evidence) — base rate of this evidence in general
    pub evidence_base_rate: f64,
    /// Description of the new evidence
    pub evidence_description: Option<String>,
}

/// One outcome entry for `ScoreRequest`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct OutcomeEntry {
    /// The event ID this outcome refers to.
    pub event_id: String,
    /// Whether the event occurred (true = Yes, false = No).
    pub occurred: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScoreRequest {
    /// Forecast record ID
    pub forecast_id: String,
    /// Events to score
    pub events: Vec<ScenarioEvent>,
    /// Outcomes: which events occurred.
    pub outcomes: Vec<OutcomeEntry>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrateRequest {
    /// The forecast question
    pub question: String,
    /// Fermi sub-questions
    pub sub_questions: Vec<SubQuestion>,
    /// Reference class description
    pub reference_class: Option<String>,
    /// Base rate from outside view
    pub base_rate: Option<f64>,
    /// Number of reference cases considered
    pub reference_count: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct SensitivityRequest {
    /// Events to analyze
    pub events: Vec<ScenarioEvent>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SynthesizeRequest {
    /// Event ID to synthesize perspectives for
    pub event_id: String,
    /// Perspectives to aggregate
    pub(crate) perspectives: Vec<Perspective>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CalibrationRequest {
    /// Optional: filter to a specific subject
    pub subject: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriageRequest {
    /// The forecasting question to triage
    pub question: String,
    /// Does the question have a specific deadline?
    pub has_deadline: Option<bool>,
    /// Is a reference class available?
    pub has_reference_class: Option<bool>,
    /// Are resolution criteria clear?
    pub has_resolution_criteria: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResearchRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Raw text from web searches about this subject
    pub research_text: String,
    /// Time horizon for the scenario scenario
    pub time_horizon: Option<String>,
    /// Scenario type
    pub scenario_type: Option<String>,
    /// Maximum number of events to extract
    pub max_events: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AssessRequest {
    /// Project identifier
    pub project_id: String,
    /// Subject domain
    pub subject: String,
    /// How many perspectives were engaged
    pub perspective_count: Option<usize>,
    /// Disagreement score from dragonfly-eye synthesis
    pub disagreement_score: Option<f64>,
    /// Total events in the scenario tree
    pub event_count: Option<usize>,
    /// How many events have conditional dependencies
    pub events_with_dependencies: Option<usize>,
    /// How many strategies were generated
    pub strategies_generated: Option<usize>,
    /// How many strategies were actually implemented
    pub strategies_implemented: Option<usize>,
    /// Observable learning events (free-text descriptions)
    pub learning_events: Option<String>,
    /// Whether early-warning indicators were defined
    pub has_early_warning_indicators: Option<bool>,
}

/// Empty request for scenario_status (no parameters needed).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StatusRequest {}
