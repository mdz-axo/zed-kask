use schemars::JsonSchema;
use serde::Deserialize;

// ── Request types for MCP tools ────────────────────────────────────────────

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BuildEventsRequest {
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
pub struct FrameRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Optional: pre-populated answers from a previous framing session
    pub prior_answers: Option<String>,
}

/// Request to structure a completed framing conversation into a FramingDocument.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FrameDocumentRequest {
    /// Subject for the scenario project
    pub subject: String,
    /// JSON object with answers from the 7-turn framing conversation.
    /// Expected keys: focal_question, decision_at_stake, time_horizon,
    /// action_deadline, in_scope, out_of_scope, stakeholders, use_case,
    /// success_criteria, constraints, surfaced_assumptions, exploration_prompts.
    pub answers: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompaniesBridgeRequest {
    /// Company symbol
    pub symbol: String,
    /// JSON output from companies.calibrate_forecast
    pub companies_output: String,
    /// Time horizon for the scenario events
    pub time_horizon: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketsBridgeRequest {
    /// JSON of an annotated MarketRecord from hkask-mcp-prediction-markets
    /// (market_lookup or market_match output; for market_match, pass the
    /// nested `market` object and set match_confidence).
    pub market_record: String,
    /// Match confidence from market_match ("high"/"medium"/"low") — omit when
    /// the caller resolved the market unambiguously (e.g. direct lookup).
    pub match_confidence: Option<String>,
}

/// One dependency edge for `scenario_from_markets_set`.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DependencySpecRequest {
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
pub struct PropagateRequest {
    /// JSON array of ScenarioEvents (the current tree's events, e.g. from a
    /// prior scenario_from_markets_set / scenario_quantify output).
    pub events: String,
    /// ID of the event whose prior is being revised.
    pub event_id: String,
    /// The new prior probability in [0, 1].
    pub new_prior: f64,
}

/// Request for `scenario_from_markets_set`: compose N market records into a
/// dependent event tree.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct MarketsSetBridgeRequest {
    /// JSON array of annotated MarketRecords from hkask-mcp-prediction-markets.
    pub market_records: String,
    /// Optional per-record match confidences ("high"/"medium"/"low" or null),
    /// parallel to the records array. Omit for direct lookups.
    pub match_confidences: Option<Vec<Option<String>>>,
    /// Caller-authored dependency edges. Omit for a flat (independent) tree.
    pub dependency_specs: Option<Vec<DependencySpecRequest>>,
}

/// One dependency edge for `scenario_from_cmp_indices`. Uses CMP index IDs
/// (`cmp:{family}:{tenor}:{orientation}`) instead of market IDs.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CmpDependencySpecRequest {
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
pub struct CmpBridgeRequest {
    /// JSON array of ProvenancedCmpIndex objects from
    /// hkask-mcp-prediction-markets (build_cmp_indices output).
    pub cmp_indices: String,
    /// The observation date (YYYY-MM-DD) the CMP indices were built. The event
    /// deadlines are observation_date + target_maturity_days.
    pub observation_date: String,
    /// Optional caller-authored dependency edges between CMP index IDs. Omit
    /// for a flat (independent) tree.
    pub dependency_specs: Option<Vec<CmpDependencySpecRequest>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct FullPipelineRequest {
    /// Subject: company ticker, industry, country, or technology domain
    pub subject: String,
    /// Events as JSON array of ScenarioEvent objects (from scenario_brainstorm or manual construction)
    pub events: String,
    /// Optional: perspectives for dragonfly-eye synthesis, as JSON array of Perspective objects
    pub perspectives: Option<String>,
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
pub struct CrossValidateRequest {
    /// Event or question identifier
    pub event_id: String,
    /// Label for the first estimate source (e.g., 'superforecasting_skill')
    pub source_a: String,
    /// First probability estimate (0.0-1.0)
    pub estimate_a: f64,
    /// Fermi sub-questions for estimate A as JSON array
    pub sub_questions_a: String,
    /// Label for the second estimate source (e.g., 'scenario_calibrate')
    pub source_b: String,
    /// Second probability estimate (0.0-1.0)
    pub estimate_b: f64,
    /// Fermi sub-questions for estimate B as JSON array
    pub sub_questions_b: String,
    /// Review threshold (default 0.15). Divergence above this triggers review.
    pub review_threshold: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct QuantifyRequest {
    /// Events as JSON array of ScenarioEvent objects
    pub events: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct UpdateRequest {
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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScoreRequest {
    /// Forecast record ID
    pub forecast_id: String,
    /// Events as JSON array of ScenarioEvent objects
    pub events: String,
    /// Outcomes: array of {event_id, occurred} objects as JSON
    pub outcomes: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrateRequest {
    /// The forecast question
    pub question: String,
    /// Fermi sub-questions as JSON array of {question, estimate, confidence}
    pub sub_questions: String,
    /// Reference class description
    pub reference_class: Option<String>,
    /// Base rate from outside view
    pub base_rate: Option<f64>,
    /// Number of reference cases considered
    pub reference_count: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SensitivityRequest {
    /// Events as JSON array of ScenarioEvent objects
    pub events: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SynthesizeRequest {
    /// Event ID to synthesize perspectives for
    pub event_id: String,
    /// Perspectives as JSON array of Perspective objects
    pub perspectives: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CalibrationRequest {
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
pub struct ResearchRequest {
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
pub struct AssessRequest {
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
