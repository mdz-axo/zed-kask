//! Data model for scenario planning and superforecasting.
//!
//! Scenario construction paradigm: MAIA event-tree — binomial events
//! with conditional dependencies. The Schwartz 2×2 axis-driven mode
//! lives in `hkask-mcp-companies` (see `docs/architecture/scenarios-companies-bridge.md`).
//!
//! Shared: Fermi decomposition, outside/inside view calibration, Bayesian updating,
//! Brier scoring, event tree computation, dragonfly-eye synthesis, calibration tracking.

use chrono::NaiveDate;
use hkask_forecast::ForecastError;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Error type ──────────────────────────────────────────────────────────────

/// Errors from scenario computation and validation.
#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("no events provided")]
    NoEvents,

    #[error("event '{0}': probability {1} not in [0, 1]")]
    InvalidProbability(String, f64),

    #[error("event '{0}': dependency on '{1}': conditional probability {2} not in [0, 1]")]
    InvalidDependencyProbability(String, String, f64),

    #[error("event '{0}': invalid dependency: {1}")]
    InvalidDependency(String, String),

    #[error("event '{0}' depends on unknown parent '{1}'")]
    UnknownParent(String, String),

    #[error("cycle detected in event dependency graph")]
    CycleDetected,

    #[error("event '{0}' not found")]
    EventNotFound(String),

    /// Wrapped forecast-engine error (Fermi validation, Brier scoring).
    #[error(transparent)]
    Forecast(#[from] ForecastError),

    #[error("cannot synthesize: fewer than 2 perspectives provided")]
    InsufficientPerspectives,

    #[error("no stored forecasts found for calibration")]
    NoForecastData,

    #[error("empty input: {0}")]
    EmptyInput(String),
}

// ── Framing (Chermack Phase 1 + Schwartz Stage 1) ──────────────────────────

/// The output of a scenario framing session — the essential parameters
/// established BEFORE brainstorming begins.
///
/// This is the "semi-structured discussion" that scopes the project:
/// what are we trying to learn, for whom, by when, and why?
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FramingDocument {
    /// The focal question or decision this scenario project informs.
    /// Schwartz: must be decision-relevant, time-bounded, scope-bounded.
    pub focal_question: String,

    /// What decision does this scenario project inform?
    /// If the answer is "nothing" — the project has no purpose.
    pub decision_at_stake: String,

    /// Time horizon for scenario events.
    pub time_horizon: TimeHorizon,

    /// When do we need to act on the insights? (May differ from event horizon.)
    pub action_deadline: Option<String>,

    /// What is explicitly in scope?
    pub in_scope: Vec<String>,

    /// What is explicitly out of scope?
    pub out_of_scope: Vec<String>,

    /// Who are the stakeholders? Whose perspectives matter?
    pub stakeholders: Vec<StakeholderConfig>,

    /// How will the scenario output be consumed?
    pub use_case: UseCase,

    /// What constitutes success for this project?
    /// Chermack: define assessment criteria BEFORE building scenarios.
    pub success_criteria: Vec<String>,

    /// Explicit constraints (budget, time, information access, confidentiality).
    pub constraints: Vec<String>,

    /// Key assumptions to surface and track.
    /// Chermack: hidden assumptions are the primary source of scenario error.
    pub surfaced_assumptions: Vec<String>,

    /// What specific questions should the personas explore?
    /// These guide the brainstorming protocol.
    pub exploration_prompts: Vec<String>,
}

/// A stakeholder whose perspective should be represented in the scenario process.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StakeholderConfig {
    /// Role or name
    pub role: String,
    /// What does this stakeholder care about most?
    pub primary_concern: String,
    /// What blind spots might this stakeholder have?
    pub likely_blind_spots: Vec<String>,
    /// Should this stakeholder be a brainstorming persona?
    pub include_as_persona: bool,
}

/// How the scenario output will be consumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UseCase {
    /// Informing a specific strategic decision (e.g., "should we enter market X?")
    StrategicDecision,
    /// Validating or stress-testing an investment thesis
    InvestmentThesis,
    /// Ongoing monitoring of early-warning indicators
    MonitoringDashboard,
    /// General exploration — understanding the landscape before committing to a decision
    LandscapeExploration,
    /// Preparing contingency plans for specific trigger events
    ContingencyPlanning,
}

impl UseCase {
    pub fn display(&self) -> &'static str {
        match self {
            Self::StrategicDecision => "Informing a specific strategic decision",
            Self::InvestmentThesis => "Validating an investment thesis",
            Self::MonitoringDashboard => "Building an early-warning monitoring dashboard",
            Self::LandscapeExploration => "General landscape exploration",
            Self::ContingencyPlanning => "Preparing contingency plans",
        }
    }
}

// ── Time horizons ──────────────────────────────────────────────────────────

/// Planning horizon for scenario construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimeHorizon {
    /// 12–18 months: near-term opportunities and challenges
    Tactical,
    /// 3–5 years: investment thesis validation
    Strategic,
    /// 7–10 years: domain-level economic potential
    LongTerm,
}

impl TimeHorizon {
    pub fn display(&self) -> &'static str {
        match self {
            Self::Tactical => "12-18 months",
            Self::Strategic => "3-5 years",
            Self::LongTerm => "7-10 years",
        }
    }
}

// ── Scenario types ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScenarioType {
    /// Quarterly or periodic company updates
    CompanyUpdate,
    /// Investment thesis construction
    CompanyAnalysis,
    /// Near-term disruption or shift
    EmergingEconomic,
    /// Long-term economic driver analysis
    EconomicPotential,
}

// ── Certainty tiers (MAIA three-level system) ──────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CertaintyTier {
    /// Already started to happen, could stop (67%+)
    Proximate,
    /// All elements exist for it to happen (33–66%)
    Probable,
    /// Could happen but unlikely (<33%)
    Possible,
}

impl CertaintyTier {
    pub fn from_probability(p: f64) -> Self {
        if p >= 0.67 {
            Self::Proximate
        } else if p >= 0.33 {
            Self::Probable
        } else {
            Self::Possible
        }
    }

    pub fn range(&self) -> &'static str {
        match self {
            Self::Proximate => "67–100%",
            Self::Probable => "33–66%",
            Self::Possible => "0–32%",
        }
    }
}

// ── Scenario Event (MAIA event-tree node) ──────────────────────────────────

/// A binomial scenario event — a yes/no question with a deadline.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ScenarioEvent {
    /// Unique event identifier
    pub id: String,
    /// Short descriptive name
    pub name: String,
    /// Yes/no framed question including specific date/deadline
    pub question: String,
    /// When the event resolves
    pub deadline: NaiveDate,
    /// Planning horizon for this event
    pub time_horizon: TimeHorizon,
    /// Type of scenario this event belongs to
    pub scenario_type: ScenarioType,
    /// Subject (company ticker, industry, country, technology domain)
    pub subject: String,

    // Probabilistic
    /// Current calibrated probability that the event occurs (0.0–1.0)
    pub probability: f64,
    /// Basis for probability estimate: "technical_feasibility" or "scaling_distribution"
    pub basis: Option<String>,

    // Dependencies (tree structure)
    /// Events this event's probability depends on
    pub depends_on: Vec<EventDependency>,

    // Calibration metadata
    /// Fermi decomposition sub-questions
    pub sub_questions: Vec<SubQuestion>,
    /// Outside-view base rate from reference class
    pub base_rate: Option<f64>,
    /// Reference class description
    pub reference_class: Option<String>,
    /// Brier score (populated after outcome is known)
    pub brier_score: Option<f64>,
    /// How many Bayesian update cycles
    pub update_count: u64,
}

/// Conditional dependency: this event's probability depends on one or more
/// parent events. Encodes the full joint conditional table as a bitmap-indexed
/// vector. Parent probabilities are assumed independent during marginalization.
///
/// # Bitmap indexing
///
/// `conditionals[i]` = P(this_event | parent truth assignment i), where bit j
/// of i corresponds to `parent_event_ids[j]`. For a single parent:
/// `conditionals[0]` = P(E | ¬parent), `conditionals[1]` = P(E | parent).
///
/// # Validation
///
/// `conditionals.len()` must equal 2^`parent_event_ids.len()`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventDependency {
    /// IDs of parent events that jointly condition this event's probability.
    pub parent_event_ids: Vec<String>,
    /// Full joint conditional table P(this_event | parent truth assignment),
    /// indexed by bitmap across all parents. See struct-level docs for encoding.
    pub conditionals: Vec<f64>,
}

// ── Fermi decomposition ─────────────────────────────────────────────────────

/// A sub-question from Fermi decomposition.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubQuestion {
    /// The sub-question text
    pub question: String,
    /// Best estimate for this sub-question (0.0–1.0)
    pub estimate: f64,
    /// Confidence in this estimate (0.0–1.0)
    pub confidence: f64,
}

// ── Calibrated forecast ────────────────────────────────────────────────────

// ── Dragonfly-Eye Synthesis (P1) ───────────────────────────────────────────

/// A single perspective on an event — one analyst's probability estimate
/// with their Fermi decomposition and rationale.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Perspective {
    /// Who or what produced this perspective (analyst name, agent ID, model name)
    pub source: String,
    /// Calibrated probability for this event (0.0–1.0)
    pub probability: f64,
    /// Fermi decomposition sub-questions backing this estimate
    pub fermi_sub_questions: Vec<SubQuestion>,
    /// Outside-view base rate for this perspective
    pub base_rate: Option<f64>,
    /// Reference class description
    pub reference_class: Option<String>,
    /// Free-text rationale for this estimate
    pub rationale: Option<String>,
    /// Historical Brier score (for empirical weighting, if available)
    pub historical_brier: Option<f64>,
}

/// Synthesized forecast from multiple independent perspectives.
/// The dragonfly has 30,000 lenses — this aggregates them into one view.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DragonflySynthesis {
    /// Which event is being synthesized
    pub event_id: String,
    /// All perspectives considered
    pub perspectives: Vec<Perspective>,
    /// Aggregated probability (empirical-Bayes weighted average)
    pub aggregated_probability: f64,
    /// 0 = perfect consensus, 1 = maximum divergence among perspectives
    pub disagreement_score: f64,
    /// Strongest counter-argument from the dissenting perspective
    pub dissent_summary: Option<String>,
    /// Weight assigned to each perspective in the aggregation
    pub perspective_weights: Vec<(String, f64)>,
    /// Interpretation of the synthesis quality
    pub synthesis_quality: String,
}

// ── Calibration Tracking (P2) ──────────────────────────────────────────────

/// A single stored forecast awaiting or having received an outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredForecastRecord {
    /// Schema version for forward-compatible deserialization (current: 1).
    /// Old files without this field default to 0.
    #[serde(default)]
    pub schema_version: u32,
    pub forecast_id: String,
    pub event_id: String,
    pub event_name: String,
    pub subject: String,
    pub probability: f64,
    pub created_at: NaiveDate,
    /// None = still pending, Some = outcome known
    pub outcome: Option<bool>,
    pub resolved_at: Option<NaiveDate>,
}

/// One bin in a calibration curve — forecasts in a probability range
/// and their actual hit rate.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CalibrationBin {
    /// Probability range (e.g. 0.7–0.8)
    pub probability_range: String,
    /// How many forecasts fall in this bin
    pub forecast_count: u64,
    /// Actual frequency of occurrence (should match bin midpoint if calibrated)
    pub hit_rate: f64,
    /// Expected rate (bin midpoint)
    pub expected_rate: f64,
    /// expected_rate - hit_rate: positive = overconfident, negative = underconfident
    pub bias: f64,
}

/// Full calibration curve across all stored forecasts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CalibrationCurve {
    pub bins: Vec<CalibrationBin>,
    pub total_forecasts: u64,
    pub resolved_forecasts: u64,
    pub overall_brier: f64,
    /// + = systematically overconfident, - = underconfident, 0 = well calibrated
    pub overconfidence_score: f64,
    pub interpretation: String,
}

// ── Triage (P4) ────────────────────────────────────────────────────────────

/// Triage assessment for a forecasting question.
/// Evaluates whether a question is worth the full superforecasting pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TriageAssessment {
    /// The original question
    pub question: String,
    /// Is this worth forecasting?
    pub is_forecastable: bool,
    /// Difficulty classification
    pub difficulty: String,
    /// How clearly is the question specified? (0–1)
    pub clarity_score: f64,
    /// How available is the data needed? (0–1)
    pub data_availability_score: f64,
    /// How clear are the resolution criteria? (0–1)
    pub resolution_criteria_clarity: f64,
    /// Overall triage recommendation
    pub recommendation: String,
}

// ── Brainstorming Protocol ────────────────────────────────────────────────

/// A persona for divergent event generation.
/// Each persona brings a distinct perspective to the brainstorming session.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PersonaConfig {
    /// Persona name (e.g., "Bull", "Bear", "Contrarian", "Domain Expert")
    pub name: String,
    /// Lens: what this persona focuses on
    pub lens: String,
    /// Prompt fragment: how this persona should think
    pub prompt: String,
}

/// A single round in the multi-round brainstorming protocol.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BrainstormRound {
    /// Round number (1-4)
    pub round: u8,
    /// Round name
    pub name: String,
    /// Cognitive mode: "diverge", "ground", "link", "prune"
    pub mode: String,
    /// Temperature guidance for the LLM
    pub temperature_guidance: String,
    /// What this round produces
    pub output_type: String,
    /// Detailed instructions for the LLM
    pub instructions: String,
    /// Quality gate: what must be true before proceeding to next round?
    pub quality_gate: Option<String>,
}

/// Full multi-round brainstorming protocol.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BrainstormProtocol {
    /// Subject being brainstormed
    pub subject: String,
    /// Time horizon
    pub time_horizon: String,
    /// Research context (from web search)
    pub research_context: String,
    /// Personas configured for this session
    pub personas: Vec<PersonaConfig>,
    /// Rounds in the protocol
    pub rounds: Vec<BrainstormRound>,
    /// Pipeline: what tools to use at each stage
    pub pipeline: Vec<String>,
}

// ── Chermack Project Assessment (P5) ──────────────────────────────────────

/// Input bundle for `assess_project`. Groups the 11 former positional
/// parameters into a single struct so callers don't need to remember
/// argument order.
#[derive(Debug)]
pub struct AssessInput<'a> {
    pub project_id: &'a str,
    pub subject: &'a str,
    pub perspective_count: usize,
    pub disagreement_score: f64,
    pub event_count: usize,
    pub events_with_deps: usize,
    pub calibration_curve: Option<&'a CalibrationCurve>,
    pub strategies_generated: usize,
    pub strategies_implemented: usize,
    pub learning_events: Vec<String>,
    pub has_early_warning_indicators: bool,
}

/// Assessment of a scenario project's effectiveness.
/// Based on Chermack's five-phase performance-based scenario system
/// (Scenario Planning in Organizations, 2011).
///
/// Evaluates not just forecast accuracy (Tetlock) but whether the
/// scenario project improved decision quality and organizational learning.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProjectAssessment {
    /// Project identifier
    pub project_id: String,
    /// Subject domain
    pub subject: String,

    // Phase 1: Preparation — was scope clear? Stakeholders engaged?
    pub preparation: PhaseScore,
    // Phase 2: Exploration — were diverse perspectives considered?
    pub exploration: PhaseScore,
    // Phase 3: Development — are scenarios internally consistent?
    pub development: PhaseScore,
    // Phase 4: Implementation — did strategies change?
    pub implementation: PhaseScore,
    // Phase 5: Project Assessment — did the project improve outcomes?
    pub project_assessment: PhaseScore,

    /// Composite score across all five phases (0-1)
    pub overall_score: f64,
    /// Overall assessment narrative
    pub overall_assessment: String,
    /// Observable learning events (Chermack: evidence of mental model change)
    pub learning_evidence: Vec<String>,
    /// Recommendations for improvement
    pub recommendations: Vec<String>,
}

/// Score for a single phase of the scenario project.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PhaseScore {
    /// Phase name
    pub phase: String,
    /// Score 0-1
    pub score: f64,
    /// What was done well
    pub strengths: Vec<String>,
    /// What needs improvement
    pub gaps: Vec<String>,
}
// ── Probability-weighted scenarios ─────────────────────────────────────────

/// An event tree node with resolved probability (after conditional computation).
/// Full event tree with resolved probabilities.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventTreeNode {
    pub event: ScenarioEvent,
    /// Resolved marginal probability after applying all dependencies
    pub marginal_probability: f64,
    /// All paths from root nodes to this node (event ID chains).
    /// For single-parent events, this is a one-element Vec.
    /// For multi-parent events, each parent produces a separate path.
    pub paths: Vec<Vec<String>>,
    /// Contribution to uncertainty (sensitivity proxy)
    pub variance_contribution: f64,
}

/// Full event tree with resolved probabilities.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EventTree {
    pub subject: String,
    pub time_horizon: TimeHorizon,
    pub scenario_type: ScenarioType,
    pub nodes: Vec<EventTreeNode>,
    /// Root nodes (no dependencies)
    pub root_ids: Vec<String>,
    /// Topologically sorted node IDs
    pub topo_order: Vec<String>,
    /// Approximate probability that all events occur, using parent-true
    /// conditionals. Multi-parent nodes use the documented average proxy.
    pub joint_probability: f64,
}

// ── Forecast outcome and Brier scoring ─────────────────────────────────────

/// Recorded outcome of a forecast.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ForecastOutcome {
    /// Forecast record ID
    pub forecast_id: String,
    /// Subject identifier
    pub subject: String,
    /// When the forecast was made
    pub forecast_date: NaiveDate,
    /// When the outcome was recorded
    pub outcome_date: NaiveDate,
    /// Per-event outcomes: event_id → occurred (true/false)
    pub event_outcomes: Vec<(String, bool)>,
    /// Brier score across all events
    pub brier_score: f64,
    /// Interpretation: excellent, good, fair, poor, worse_than_climatology
    pub brier_interpretation: String,
}
// ── Cross-Validation ────────────────────────────────────────────────────

/// Result of cross-validating two probability estimates — typically
/// an LLM-generated estimate (superforecasting skill) against a
/// server-computed estimate (scenario_calibrate tool).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CrossValidation {
    /// Event or question being validated
    pub event_id: String,
    /// First estimate (e.g., LLM-generated from superforecasting skill)
    pub estimate_a: f64,
    /// Source of estimate A
    pub source_a: String,
    /// Second estimate (e.g., server-computed from scenario_calibrate)
    pub estimate_b: f64,
    /// Source of estimate B
    pub source_b: String,
    /// Absolute divergence |P_a - P_b|
    pub divergence: f64,
    /// Does the divergence exceed the review threshold?
    pub requires_review: bool,
    /// Threshold used (default 0.15)
    pub review_threshold: f64,
    /// Which sub-questions diverge most between the two estimates?
    pub sub_question_divergences: Vec<SubQuestionDivergence>,
    /// Recommended action
    pub recommendation: String,
    /// Concrete Socratic questions for grill-me skill interrogation.
    /// Populated when requires_review is true. Each question targets
    /// a specific assumption or sub-question divergence for adversarial probing.
    pub grill_me_questions: Vec<String>,
}

/// Per-sub-question divergence between two Fermi decompositions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SubQuestionDivergence {
    /// The sub-question text
    pub question: String,
    /// Estimate from source A
    pub estimate_a: f64,
    /// Estimate from source B
    pub estimate_b: f64,
    /// Absolute divergence
    pub divergence: f64,
}

// ── Validation ─────────────────────────────────────────────────────────────

impl ScenarioEvent {
    /// Derive the certainty tier from the current probability.
    /// Always consistent — no stale-field divergence possible.
    pub fn certainty_tier(&self) -> CertaintyTier {
        CertaintyTier::from_probability(self.probability)
    }

    /// Validate probability is in [0, 1] and finite (no NaN).
    pub fn validate(&self) -> Result<(), ScenarioError> {
        if !self.probability.is_finite() || !(0.0..=1.0).contains(&self.probability) {
            return Err(ScenarioError::InvalidProbability(
                self.name.clone(),
                self.probability,
            ));
        }
        for dep in &self.depends_on {
            // Validate parent IDs are non-empty
            if dep.parent_event_ids.is_empty() {
                return Err(ScenarioError::InvalidDependency(
                    self.name.clone(),
                    "parent_event_ids must not be empty".into(),
                ));
            }
            // Validate conditionals length = 2^n
            let expected_len = 1usize << dep.parent_event_ids.len();
            if dep.conditionals.len() != expected_len {
                return Err(ScenarioError::InvalidDependency(
                    self.name.clone(),
                    format!(
                        "conditionals length {} must be 2^{} = {}",
                        dep.conditionals.len(),
                        dep.parent_event_ids.len(),
                        expected_len
                    ),
                ));
            }
            // Validate all conditional probabilities are finite and in [0, 1]
            for (i, &p) in dep.conditionals.iter().enumerate() {
                if !p.is_finite() || !(0.0..=1.0).contains(&p) {
                    return Err(ScenarioError::InvalidDependencyProbability(
                        self.name.clone(),
                        format!("conditionals[{}]", i),
                        p,
                    ));
                }
            }
        }
        Ok(())
    }
}
