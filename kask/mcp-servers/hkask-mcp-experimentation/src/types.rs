//! Domain types for the experiment registry (repair plan §P8).

use serde::{Deserialize, Serialize};

/// The six artifact layers (§P8.3 + D2) — the shard map's genotype
/// media. `scientific_hypothesis` (operator ruling 2026-10-15, D2 — the
/// general lab notebook): the genotype is the operationalized hypothesis
/// plus its measurement plan; fitness comes from recorded observations
/// (harness reports, data-series reads, market resolutions), never
/// self-graded scores.
pub const LAYERS: &[&str] = &[
    "skill",
    "agent_card",
    "prompt_template",
    "tool_schema",
    "regulation_scalar",
    "scientific_hypothesis",
];

/// Experiment lifecycle (§P8.4): proposed at registration, running once the
/// first variant registers, resolved by the first selection. One selection
/// resolves an experiment; a further selection is a failed_precondition
/// conflict, never a silent overwrite.
pub const STATUS_PROPOSED: &str = "proposed";
pub const STATUS_RUNNING: &str = "running";
pub const STATUS_RESOLVED: &str = "resolved";

/// Closed status vocabulary for population queries — parse-don't-validate
/// at the tool boundary: the tool schema renders the closed set and invalid
/// values reject at deserialization instead of reaching the handler.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExperimentStatus {
    Proposed,
    Running,
    Resolved,
}

impl ExperimentStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => STATUS_PROPOSED,
            Self::Running => STATUS_RUNNING,
            Self::Resolved => STATUS_RESOLVED,
        }
    }
}

pub const VERDICT_SELECTED: &str = "selected";
pub const VERDICT_REJECTED: &str = "rejected";

/// Typed scientific outcome vocabulary (task `f1a90cd2`) — the design-family
/// resolution set that replaces `selected`/`rejected` for a design with no
/// meaningful binary baseline. Only the two genuinely binary resolutions
/// (`rejected`, `corroborated`) enter Brier calibration; the bounded and the
/// two void outcomes never test the preregistered binary claim and are
/// excluded on this typed field, never by parsing reject-reason text.
pub const SCIENTIFIC_OUTCOME_REJECTED: &str = "rejected";
pub const SCIENTIFIC_OUTCOME_CORROBORATED: &str = "corroborated";
pub const SCIENTIFIC_OUTCOME_EQUIVALENCE: &str = "evidence_of_practical_equivalence";
pub const SCIENTIFIC_OUTCOME_INCONCLUSIVE: &str = "inconclusive";
pub const SCIENTIFIC_OUTCOME_MEASUREMENT_VOID: &str = "measurement_void";
pub const SCIENTIFIC_OUTCOME_DESIGN_VOID: &str = "design_void";

pub const SCIENTIFIC_OUTCOMES: &[&str] = &[
    SCIENTIFIC_OUTCOME_REJECTED,
    SCIENTIFIC_OUTCOME_CORROBORATED,
    SCIENTIFIC_OUTCOME_EQUIVALENCE,
    SCIENTIFIC_OUTCOME_INCONCLUSIVE,
    SCIENTIFIC_OUTCOME_MEASUREMENT_VOID,
    SCIENTIFIC_OUTCOME_DESIGN_VOID,
];

/// A pre-registered prediction — the tiny controller (§P8.1). The claim is
/// scored against the experiment's measured outcome by the linked kanban
/// goal; the confidence is the experiment's own probability for the claim.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Prediction {
    /// The falsifiable claim the experiment pre-registers (e.g. "variant B
    /// passes the fixed task set at a higher rate than the baseline").
    pub claim: String,
    /// The experiment's own confidence in the claim, in [0, 1].
    pub confidence: f64,
}

/// The tagged scientific headroom criterion (task `f1a90cd2`) — the seal
/// attestation for a design family that has no meaningful binary baseline
/// pass rate (single-arm calibration, observational estimation, equivalence).
/// `design_family` is the tag; `criterion` states what makes the design
/// non-saturated in a way a pass rate cannot express; `evidence` carries the
/// recorded report refs that support it. This is the honest alternative to
/// fabricating a `baseline_pass_rate`.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ScientificHeadroom {
    /// The design family the criterion applies to (e.g.
    /// `measurement_or_calibration`, `observational_estimation`, `single_arm`,
    /// `equivalence`).
    pub design_family: String,
    /// The headroom criterion — what makes this design informative, stated so
    /// it can fail.
    pub criterion: String,
    /// Recorded report refs evidencing the criterion.
    pub evidence: Vec<String>,
}

/// Rubric kinds (the v3 create-idea rules): every declaration carries at
/// least one `metric` rubric and at least one `implementation` or
/// `grounded_report` rubric.
pub const RUBRIC_KIND_METRIC: &str = "metric";
pub const RUBRIC_KIND_IMPLEMENTATION: &str = "implementation";
pub const RUBRIC_KIND_GROUNDED_REPORT: &str = "grounded_report";

pub const RUBRIC_KINDS: &[&str] = &[
    RUBRIC_KIND_METRIC,
    RUBRIC_KIND_IMPLEMENTATION,
    RUBRIC_KIND_GROUNDED_REPORT,
];

/// A rubric's verification — deliberately the same vocabulary as
/// `swarm_evaluate_local`'s evaluator kinds (contains / not_contains /
/// regex) plus the recorded-report form: one grammar from declaration
/// (P1) to recorded run.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RubricVerification {
    /// The evidence must contain the spec string (== swarm "contains").
    Contains { spec: String },
    /// The evidence must not contain the spec string (== swarm "not_contains").
    NotContains { spec: String },
    /// The evidence must match the regex pattern (== swarm "regex").
    Regex { pattern: String },
    /// A recorded harness report names the evidence — a declaration this
    /// server validates and stores, never one it executes.
    ReportRef { harness: String },
}

/// One typed pass/fail criterion, declared before measurement (P1 — the
/// machine-checkable eval contract; the derived-registry `rubric` ruling,
/// operator 2026-10-15).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Rubric {
    /// Stable snake_case identifier (e.g. `pass_rate_threshold`).
    pub id: String,
    /// A concrete, measurable description of what passing means.
    pub description: String,
    /// metric | implementation | grounded_report.
    pub kind: String,
    /// The typed deterministic check that adjudicates the rubric.
    pub verification: RubricVerification,
}

/// Validate a declaration's rubric bundle (the v3 create-idea rules):
/// non-empty; at least one `metric` rubric; at least one `implementation`
/// or `grounded_report` rubric; known kinds; unique stable snake_case
/// ids; non-empty descriptions.
pub fn validate_rubrics(rubrics: &[Rubric]) -> Result<(), ExperimentationError> {
    if rubrics.is_empty() {
        return Err(ExperimentationError::RubricsRequired);
    }
    let mut seen_ids = std::collections::HashSet::new();
    for rubric in rubrics {
        if rubric.id.trim().is_empty() {
            return Err(ExperimentationError::Empty("rubric id"));
        }
        if !rubric
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            || rubric.id.starts_with(|c: char| c.is_ascii_digit())
        {
            return Err(ExperimentationError::RubricIdInvalid(rubric.id.clone()));
        }
        if !seen_ids.insert(rubric.id.clone()) {
            return Err(ExperimentationError::DuplicateRubricId(rubric.id.clone()));
        }
        if rubric.description.trim().is_empty() {
            return Err(ExperimentationError::Empty("rubric description"));
        }
        if !RUBRIC_KINDS.contains(&rubric.kind.as_str()) {
            return Err(ExperimentationError::UnknownRubricKind(rubric.kind.clone()));
        }
    }
    if !rubrics.iter().any(|r| r.kind == RUBRIC_KIND_METRIC) {
        return Err(ExperimentationError::RubricsMissingKind(RUBRIC_KIND_METRIC));
    }
    if !rubrics
        .iter()
        .any(|r| r.kind == RUBRIC_KIND_IMPLEMENTATION || r.kind == RUBRIC_KIND_GROUNDED_REPORT)
    {
        return Err(ExperimentationError::RubricsMissingKind(
            "implementation or grounded_report",
        ));
    }
    Ok(())
}

/// Validate an optional scientific `DesignSpec` (task `f1a90cd2`): it must be
/// a JSON object carrying a non-empty `spec_version` string, so a stored
/// design is always versioned (the versioned-artifact requirement). `None` is
/// valid — legacy artifact experiments carry no design.
pub fn validate_design_spec(
    design_spec: Option<&serde_json::Value>,
) -> Result<(), ExperimentationError> {
    let Some(spec) = design_spec else {
        return Ok(());
    };
    let Some(object) = spec.as_object() else {
        return Err(ExperimentationError::DesignSpecInvalid(
            "not a JSON object".to_string(),
        ));
    };
    match object
        .get("spec_version")
        .and_then(serde_json::Value::as_str)
    {
        Some(version) if !version.trim().is_empty() => Ok(()),
        _ => Err(ExperimentationError::DesignSpecInvalid(
            "missing or empty spec_version".to_string(),
        )),
    }
}

/// Validate a tagged scientific headroom criterion (task `f1a90cd2`): the
/// design family and criterion must be non-empty. The evidence refs may be
/// empty only when the criterion is self-evident from the design family; a
/// supplied criterion is never silently blanked.
pub fn validate_scientific_headroom(
    headroom: &ScientificHeadroom,
) -> Result<(), ExperimentationError> {
    if headroom.design_family.trim().is_empty() || headroom.criterion.trim().is_empty() {
        return Err(ExperimentationError::ScientificHeadroomIncomplete);
    }
    Ok(())
}

/// A registered experiment (§P8.4 `experiment_propose`).
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentRecord {
    pub id: String,
    pub hypothesis: String,
    pub layer: String,
    pub genotype_refs: Vec<String>,
    pub eval_set: serde_json::Value,
    pub fitness_fn: String,
    pub noise_band: serde_json::Value,
    pub prediction: Prediction,
    pub budget: serde_json::Value,
    /// The machine-readable run ceiling parsed from `budget.max_runs` at
    /// declaration (§P8.9 step 2, D-1 enforce) — the Layer-A set point the
    /// store enforces at `fitness_record` and `final_evaluation_record`.
    pub max_runs: u64,
    /// The linked kanban goal (§P8.9 step 5) — the goal loop Brier-scores
    /// the pre-registered claim against the measured outcome; this field is
    /// the explicit registry ↔ goal-loop join.
    pub linked_goal_id: Option<String>,
    /// The typed rubric bundle (P1, the agentic-scientist-v3 integration) —
    /// the machine-checkable eval contract declared before measurement.
    pub rubrics: Vec<Rubric>,
    /// The optional typed, versioned scientific `DesignSpec` (task
    /// `f1a90cd2`) — present for scientific-hypothesis declarations carrying a
    /// first-class design, `None` for legacy artifact experiments. When
    /// present it joins the seal fingerprint, so post-seal design drift is
    /// refused rather than silently reused.
    pub design_spec: Option<serde_json::Value>,
    pub status: String,
    pub created_at: String,
}

/// Variant roles (P4): the baseline is the pre-registered comparison
/// point — exactly one per experiment; a challenger is a candidate
/// replacement; an ablation derives from a parent variant to isolate one
/// component's contribution (the derived-registry `ablation` ruling,
/// operator 2026-10-15).
pub const VARIANT_ROLE_BASELINE: &str = "baseline";
pub const VARIANT_ROLE_CHALLENGER: &str = "challenger";
pub const VARIANT_ROLE_ABLATION: &str = "ablation";

pub const VARIANT_ROLES: &[&str] = &[
    VARIANT_ROLE_BASELINE,
    VARIANT_ROLE_CHALLENGER,
    VARIANT_ROLE_ABLATION,
];

/// A registered variant — one genotype configuration inside an experiment,
/// with its parent for lineage (§P8.1).
#[derive(Debug, Clone, Serialize)]
pub struct VariantRecord {
    pub id: String,
    pub experiment_id: String,
    pub genotype_config: serde_json::Value,
    pub parent_variant_id: Option<String>,
    /// baseline | challenger | ablation (P4).
    pub role: String,
    pub created_at: String,
}

/// A fitness record — recorded report refs the caller asserts name real
/// reports (§P8.3); the server stores the declaration, it does not
/// authenticate the reports.
#[derive(Debug, Clone, Serialize)]
pub struct FitnessRecord {
    pub id: String,
    pub experiment_id: String,
    pub variant_id: String,
    pub runs: Vec<String>,
    /// The number of rollouts the referenced reports cover (§P8.9 step 2) —
    /// a report reference names a harness run whose rollout count the
    /// caller states; the store sums these against `max_runs`.
    pub run_count: u64,
    pub scores: serde_json::Value,
    /// The seal this record cites (P2) — required for post-seal records;
    /// NULL for the baseline's own pre-seal measurement.
    pub seal_id: Option<String>,
    pub created_at: String,
}

/// A selection fossil — selected and rejected both retained (§P8.7-Q4).
#[derive(Debug, Clone, Serialize)]
pub struct SelectionRecord {
    pub id: String,
    pub experiment_id: String,
    pub verdict: String,
    pub selected_variant_id: Option<String>,
    pub reject_reasons: Vec<String>,
    pub algedonic_reference: Option<String>,
    /// The typed scientific outcome (task `f1a90cd2`) when this selection
    /// resolved a scientific design rather than a legacy artifact experiment.
    /// `None` for legacy `selected`/`rejected` selections; the calibration
    /// record keys on this field, never on reject-reason text.
    pub scientific_outcome: Option<String>,
    pub created_at: String,
}

/// A sealed eval contract (P2) — the frozen measurement agreement: the
/// SHA-256 fingerprint over the declaration's (eval_set, rubrics,
/// fitness_fn, noise_band), plus the headroom attestation (the baseline
/// variant, its fitness evidence, and its measured non-saturation).
#[derive(Debug, Clone, Serialize)]
pub struct SealRecord {
    pub id: String,
    pub experiment_id: String,
    /// SHA-256 (hex) over the canonical JSON of the sealed contract
    /// fields — identical declarations seal to identical fingerprints.
    pub fingerprint: String,
    /// The baseline variant the seal attests (P4 role).
    pub baseline_variant_id: String,
    /// The fitness-record ids that evidence the baseline measurement.
    pub baseline_fitness_ids: Vec<String>,
    /// The measured baseline pass rate — validated < 1.0 at seal time
    /// (the headroom pre-check, §5 Q6). `None` for a design-family seal
    /// attested by `scientific_headroom` instead.
    pub baseline_pass_rate: Option<f64>,
    /// The tagged scientific headroom criterion (task `f1a90cd2`) for a design
    /// family without a meaningful binary baseline pass rate. Exactly one of
    /// `baseline_pass_rate` / `scientific_headroom` is set.
    pub scientific_headroom: Option<ScientificHeadroom>,
    pub created_at: String,
}

/// A sealed final evaluation (P3) — the leading variant re-scored on the
/// selection set: run refs disjoint from every prior fitness record and
/// final evaluation for the experiment (the GEPA feedback/selection split
/// made mechanical), counted against the declared budget.
#[derive(Debug, Clone, Serialize)]
pub struct FinalEvaluationRecord {
    pub id: String,
    pub experiment_id: String,
    pub variant_id: String,
    /// The seal whose frozen contract this evaluation runs under.
    pub seal_id: String,
    pub runs: Vec<String>,
    pub run_count: u64,
    pub scores: serde_json::Value,
    pub created_at: String,
}

/// Per-experiment health row for the regulation snapshot (§P8.9 step 1) —
/// the Layer-A afferent view the bridge feeds to the cybernetics loop's
/// `ExperimentationHealthSensor`.
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentHealth {
    pub id: String,
    pub layer: String,
    pub status: String,
    pub created_at: String,
    pub max_runs: u64,
    pub recorded_runs: u64,
    pub prediction_confidence: Option<f64>,
    pub verdict: Option<String>,
    /// The typed scientific outcome (task `f1a90cd2`) when the first selection
    /// carried one — the calibration-exclusion key.
    pub scientific_outcome: Option<String>,
    pub reject_reasons: Vec<String>,
}

/// The registry's health snapshot: every experiment with its recorded-run
/// total, prediction confidence, and verdict.
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentationHealthSnapshot {
    pub experiments: Vec<ExperimentHealth>,
}

impl ExperimentationHealthSnapshot {
    /// Ids of running experiments that are stuck (§P8.9 step 1, D-3):
    /// unresolved past `stale_days`, or with their whole declared budget
    /// recorded but no verdict yet. An unparseable timestamp reads as
    /// stuck — visible, never silent.
    pub fn stuck_running(&self, stale_days: u64) -> Vec<String> {
        let cutoff = chrono::Utc::now() - chrono::Duration::days(stale_days as i64);
        self.experiments
            .iter()
            .filter(|experiment| experiment.status == STATUS_RUNNING)
            .filter(|experiment| {
                let stale = chrono::DateTime::parse_from_rfc3339(&experiment.created_at)
                    .map(|stamp| stamp.with_timezone(&chrono::Utc) < cutoff)
                    .unwrap_or(true);
                let spent = experiment.recorded_runs >= experiment.max_runs;
                stale || spent
            })
            .map(|experiment| experiment.id.clone())
            .collect()
    }

    /// Mean Brier over resolved non-void claims: selected → the claim held
    /// (outcome 1), rejected → refuted (outcome 0). Void experiments — the
    /// first reject reason carries a void marker, "no-headroom" (design
    /// failure: the eval set cannot discriminate) or "measurement-void"
    /// (infrastructure failure: the runs never produced a valid
    /// measurement) — never tested their claim, so they are excluded from
    /// the calibration record rather than scored as refuted. A typed
    /// scientific outcome (task `f1a90cd2`) takes precedence: only
    /// `corroborated` (held) and `rejected` (refuted) calibrate, while
    /// `evidence_of_practical_equivalence`, `inconclusive`,
    /// `measurement_void` and `design_void` are excluded on the typed field.
    /// Returns `(mean_brier, claim_count)`; `None` when no measured claims
    /// exist.
    pub fn resolved_claim_brier(&self) -> Option<(f64, u32)> {
        let mut sum = 0.0;
        let mut claims = 0u32;
        for experiment in &self.experiments {
            if experiment.status != STATUS_RESOLVED {
                continue;
            }
            let Some(confidence) = experiment.prediction_confidence else {
                continue;
            };
            // A typed scientific outcome (task `f1a90cd2`) is the
            // authoritative resolution when present: only the two genuinely
            // binary outcomes calibrate. The bounded and void outcomes are
            // excluded structurally, on the typed field itself — never by
            // parsing reject-reason text (the `is_void_marker` first-reason
            // trap).
            if let Some(outcome) = experiment.scientific_outcome.as_deref() {
                match outcome {
                    SCIENTIFIC_OUTCOME_CORROBORATED => {
                        sum += (confidence - 1.0) * (confidence - 1.0);
                        claims += 1;
                    }
                    SCIENTIFIC_OUTCOME_REJECTED => {
                        sum += confidence * confidence;
                        claims += 1;
                    }
                    _ => continue,
                }
                continue;
            }
            match experiment.verdict.as_deref() {
                Some(VERDICT_SELECTED) => {
                    sum += (confidence - 1.0) * (confidence - 1.0);
                    claims += 1;
                }
                Some(VERDICT_REJECTED) => {
                    if experiment
                        .reject_reasons
                        .first()
                        .is_some_and(|reason| is_void_marker(reason))
                    {
                        continue;
                    }
                    sum += confidence * confidence;
                    claims += 1;
                }
                _ => {}
            }
        }
        if claims == 0 {
            None
        } else {
            Some((sum / claims as f64, claims))
        }
    }
}

/// Void markers for the calibration record: a reject reason carrying one
/// of these marks an experiment whose pre-registered claim was never
/// tested — "no-headroom" (design failure: baseline saturated, nothing to
/// measure) or "measurement-void" (infrastructure failure: the runs never
/// produced a valid measurement). Such claims are excluded from
/// [`ExperimentationHealthSnapshot::resolved_claim_brier`] rather than scored as
/// refuted, so provider or harness noise cannot pollute the calibration
/// record the agenda generator calibrates against.
fn is_void_marker(reason: &str) -> bool {
    reason.contains("no-headroom") || reason.contains("measurement-void")
}

/// Registry failures, classified per-variant for MCP dispatch — never a
/// blanket internal (project rule).
#[derive(Debug, thiserror::Error)]
pub enum ExperimentationError {
    #[error("experiment {0} not found")]
    ExperimentNotFound(String),
    #[error("variant {0} not found")]
    VariantNotFound(String),
    #[error(
        "experiment {0} is already resolved — it no longer accepts variants, fitness, or selection"
    )]
    ExperimentResolved(String),
    #[error("variant {0} does not belong to experiment {1}")]
    VariantNotInExperiment(String, String),
    #[error(
        "layer must be one of skill, agent_card, prompt_template, tool_schema, regulation_scalar, scientific_hypothesis; got {0:?}"
    )]
    UnknownLayer(String),
    #[error("prediction confidence must be in [0, 1], got {0}")]
    ConfidenceOutOfRange(f64),
    #[error(
        "budget.max_runs must be a positive integer within the i64 range — the declared budget is the Layer-A set point the server enforces (§P8.9 step 2, D-1); budget was: {0}"
    )]
    BudgetMissingMaxRuns(String),
    #[error(
        "experiment {0} budget exhausted: recording {1} total runs would exceed the declared ceiling of {2} (§P8.9 step 2, D-1)"
    )]
    BudgetExhausted(String, u64, u64),
    #[error(
        "run_count must be a positive integer within the i64 range — the number of rollouts the referenced reports cover (§P8.9 step 2); got {0}"
    )]
    RunCountInvalid(u64),
    #[error("verdict must be \"selected\" or \"rejected\", got {0:?}")]
    UnknownVerdict(String),
    #[error(
        "scientific_outcome must be one of rejected, corroborated, evidence_of_practical_equivalence, inconclusive, measurement_void, design_void; got {0:?}"
    )]
    UnknownScientificOutcome(String),
    #[error(
        "supply exactly one of verdict or scientific_outcome — got verdict={0:?} scientific_outcome={1:?}"
    )]
    SelectionResolutionConflict(Option<String>, Option<String>),
    #[error(
        "experiment_seal requires exactly one of baseline_pass_rate or scientific_headroom — a design either has a meaningful binary pass rate or it does not, never both and never neither"
    )]
    SealAttestationConflict,
    #[error(
        "design_spec must be a versioned JSON object carrying a non-empty spec_version string; got {0}"
    )]
    DesignSpecInvalid(String),
    #[error("a selected experiment must name its selected_variant_id")]
    SelectedWithoutVariant,
    #[error("a rejected experiment must record at least one reject_reason")]
    RejectedWithoutReasons,
    #[error("{0} must not be empty")]
    Empty(&'static str),
    #[error("created_since must be an RFC 3339 timestamp, got {0:?}")]
    BadTimestamp(String),
    #[error(
        "new declarations must carry a non-empty rubric bundle — the typed eval contract (P1): at least one metric rubric and one implementation or grounded_report rubric"
    )]
    RubricsRequired,
    #[error("the rubric bundle must include at least one {0} rubric")]
    RubricsMissingKind(&'static str),
    #[error("rubric kind must be one of metric, implementation, grounded_report; got {0:?}")]
    UnknownRubricKind(String),
    #[error("duplicate rubric id {0:?} — rubric ids must be unique and stable")]
    DuplicateRubricId(String),
    #[error(
        "rubric id {0:?} must be stable snake_case (lowercase, digits, underscores; not digit-led)"
    )]
    RubricIdInvalid(String),
    #[error("role must be one of baseline, challenger, ablation; got {0:?}")]
    UnknownVariantRole(String),
    #[error(
        "experiment {0} already has its baseline variant {1} — exactly one baseline is allowed (P4)"
    )]
    DuplicateBaseline(String, String),
    #[error(
        "an ablation-role variant must name its parent_variant_id — the variant whose component it ablates (P4)"
    )]
    AblationWithoutParent,
    #[error(
        "experiment {0} is already sealed by {1} — the eval contract is frozen; re-declaring sealed fields is a new experiment, never an edit (P2)"
    )]
    AlreadySealed(String, String),
    #[error(
        "experiment {0} has no seal — measure the baseline variant and seal before any challenger work (P2)"
    )]
    ExperimentNotSealed(String),
    #[error(
        "experiment {0} is sealed by {1} — every post-seal fitness record must cite that seal_id (P2)"
    )]
    SealCitationMissing(String, String),
    #[error(
        "baseline saturation at pass rate {0}: the eval set cannot discriminate (no headroom) — void the experiment via selection_record with a no-headroom reject reason, redesign the eval set, and re-declare (§5 Q6)"
    )]
    NoHeadroom(f64),
    #[error(
        "scientific_headroom.design_family and .criterion must be non-empty — the headroom attestation is the seal's whole claim when no baseline pass rate exists"
    )]
    ScientificHeadroomIncomplete,
    #[error("the seal's baseline variant {0} must carry the baseline role; got role {1:?} (P2)")]
    SealBaselineMismatch(String, String),
    #[error("the baseline variant has no recorded fitness — measure it before sealing (P2)")]
    SealFitnessMissing,
    #[error("cited seal_id {0} does not match the experiment's active seal {1} (P2)")]
    SealMismatch(String, String),
    #[error("baseline_pass_rate must be in [0, 1], got {0}")]
    SealPassRateInvalid(f64),
    #[error(
        "final-evaluation run refs must be disjoint from the experiment's prior records — overlapping refs: {0:?}"
    )]
    RunRefsNotDisjoint(Vec<String>),
    #[error("database error: {0}")]
    Database(#[from] hkask_storage::database::types::DbError),
    #[error("serialization error: {0}")]
    Serialization(String),
}
