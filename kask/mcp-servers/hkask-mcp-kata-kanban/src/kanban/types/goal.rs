//! Canonical verification ledger. Human closure, assessment, forecasts and
//! retrieval delivery are distinct records; none erases the original goal.
use super::*;
use schemars::JsonSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Goal {
    pub schema_version: u32,
    pub id: GoalID,
    pub goal_text: String,
    pub criteria: Vec<VerificationCriterion>,
    pub criterion_ids: Vec<String>,
    pub task_id: Option<TaskId>,
    pub source: GoalSource,
    pub supersedes: Option<GoalID>,
    pub spec_digest: String,
    /// The standing trust-economics clause on this goal (see
    /// [`GoalTrustClause`]). Present on every goal created after this
    /// field was added; `None` only on goals created before it existed.
    /// Part of the spec digest: the ΔT_trust ≥ 0 constraint is
    /// structural, not a preference weight (Butler–Giuliano–Guiso 2009;
    /// Akerlof 1970).
    #[serde(default)]
    pub trust_clause: Option<GoalTrustClause>,
    /// The goal's running trust ledger: every explicit instruction
    /// extracted at intake, checked during execution, recorded here
    /// (see [`GoalTrustEvent`]). Append-only — a correction appends,
    /// it never rewrites history.
    #[serde(default)]
    pub trust_events: Vec<GoalTrustEvent>,
    pub verdicts: Vec<GoalVerdict>,
    pub closure: Option<GoalClosure>,
    pub human_corrections: Vec<NativeGoalOutcomeCorrection>,
    pub assessments: Vec<AssessmentAttempt>,
    pub forecast: Option<GoalForecast>,
    pub forecast_status: ForecastStatus,
    pub scores: Vec<GoalForecastScore>,
    pub revision: u64,
    pub indexed_revision: u64,
    pub(crate) assessment_claim: Option<AssessmentClaim>,
    pub owner: WebID,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalSource {
    pub interaction_id: String,
    pub session_id: String,
    pub source: String,
}

/// The standing trust-economics clause on a canonical goal (the
/// systematized trust objective, anchored to the cited reference
/// models — research runs `54a08fda88a5c74f` and `bfa9794d30fd64af`):
/// every deliverable's value to the principal is `T_reliance · B`
/// (Butler–Giuliano–Guiso 2009, "The Right Amount of Trust," NBER
/// WP 15344); an unverifiable deliverable is rationally discounted to
/// zero (Akerlof 1970, "The Market for Lemons," *QJE* 90(3):488–500);
/// trust dynamics are asymmetric — competence violations are
/// recoverable (κ_c ≈ 0.15), integrity violations reset to the prior
/// (Slovic 1993, *Risk Analysis* 13(4); Kim–Ferrin–Cooper–Dirks 2004,
/// *JAP* 89(1)–(4)); violations are priced at the expectation interest
/// (Fuller–Perdue 1936–37, "The Reliance Interest in Contract
/// Damages," 46 *YLJ* 52 & 373). Every goal carries this clause; it
/// is part of the spec digest, so no worker can later claim the trust
/// constraint wasn't in scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalTrustClause {
    pub text: String,
    /// The cited reference models the clause is derived from.
    pub model_refs: Vec<String>,
    pub declared_at: DateTime<Utc>,
}

/// One instruction-compliance event on a goal's trust ledger
/// (Butler–Giuliano–Guiso 2009, NBER WP 15344 — the deliverable's
/// value is `T_reliance · B`; an instruction-violating deliverable is
/// rationally discounted to zero, per Akerlof 1970, *QJE* 90(3)).
/// The ledger is the goal's running trust account: every explicit
/// instruction extracted at intake is checked during execution, and
/// the result is recorded here. Append-only — a correction appends,
/// it never rewrites history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalTrustEvent {
    /// The instruction text as extracted at intake — verbatim, not
    /// paraphrased.
    pub instruction: String,
    /// Whether the instruction was complied with or violated.
    pub status: GoalTrustStatus,
    /// The severity class when the instruction is violated. `None`
    /// when `status` is `Complied`.
    pub severity: Option<GoalTrustSeverity>,
    /// Evidence for the compliance or violation claim — the pinned
    /// test, the committed diff, the measured outcome. A violation
    /// recorded without evidence is a fabricated citation.
    pub evidence: String,
    /// The operator's estimated cost when the instruction is
    /// violated: the as-if-complied position (the expectation
    /// interest, per Fuller–Perdue 1936–37, *YLJ* 46(1) 52 & 46(2)
    /// 373), not the fix cost (the restitution interest).
    pub operator_cost: Option<String>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalTrustStatus {
    Complied,
    Violated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalTrustSeverity {
    /// A disclosed competence failure — recoverable (Slovic 1993;
    /// Kim–Ferrin–Cooper–Dirks 2004: competence violations are
    /// forgivable when acknowledged; κ_c ≈ 0.15).
    Competence,
    /// A concealed integrity failure, discovered rather than
    /// disclosed — resets trust to the prior (κ_i ≈ 1).
    Integrity,
}

/// The goal's trust summary, computed from the ledger's events.
/// The count of integrity-severity violations is the escalation
/// signal — `has_integrity_violation` is the regulation flag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GoalTrustSummary {
    pub total_instructions: usize,
    pub complied: usize,
    pub violated_competence: usize,
    pub violated_integrity: usize,
    /// True when any event carries an integrity-severity violation.
    pub has_integrity_violation: bool,
}

/// The default trust clause text, derived from the four reference
/// models in the doc comment on [`GoalTrustClause`].
pub const GOAL_TRUST_CLAUSE_TEXT: &str = "No deliverable produced by violating an \
    explicit user instruction is admissible evidence for this goal. The value of any \
    deliverable is T_reliance · B (Butler–Giuliano–Guiso 2009): an unverifiable \
    deliverable is rationally discounted to zero (Akerlof 1970). Trust dynamics are \
    asymmetric (Slovic 1993; Kim–Ferrin–Cooper–Dirks 2004): a disclosed competence \
    failure is recoverable; a discovered integrity failure (concealment, \
    misweighting) resets trust to the prior across every future deliverable. All \
    violations are priced at the expectation interest (Fuller–Perdue 1936), not the \
    restitution interest: report the principal's as-if-complied position, not the \
    cost of the fix.";

/// The cited reference models the default trust clause derives from.
pub const GOAL_TRUST_CLAUSE_MODEL_REFS: &[&str] = &[
    "Butler–Giuliano–Guiso (2009), The Right Amount of Trust, NBER WP 15344",
    "Akerlof (1970), The Market for Lemons, QJE 90(3)",
    "Slovic (1993), Risk Analysis 13(4)",
    "Kim–Ferrin–Cooper–Dirks (2004), JAP 89",
    "Fuller–Perdue (1936–37), Yale Law Journal 46",
];

impl GoalTrustClause {
    /// The default trust clause every goal carries, stamped `declared_at`
    /// at goal creation. A caller may override the text; the clause is
    /// always present — trust is a feasibility constraint on the goal,
    /// not an optional preference weight.
    pub fn default_clause(now: DateTime<Utc>) -> Self {
        Self {
            text: GOAL_TRUST_CLAUSE_TEXT.to_string(),
            model_refs: GOAL_TRUST_CLAUSE_MODEL_REFS
                .iter()
                .map(|r| r.to_string())
                .collect(),
            declared_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalDeclaration {
    pub goal_text: String,
    pub criteria: Vec<VerificationCriterion>,
    pub task_id: Option<TaskId>,
    pub source: GoalSource,
    pub forecast: Option<GoalForecast>,
    pub supersedes: Option<GoalID>,
    /// The trust-economics clause. If absent at declaration, the
    /// default is applied at creation (see [`GoalTrustClause`]).
    pub trust_clause: Option<GoalTrustClause>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HumanClosureChoice {
    AccomplishedAchieved,
    AccomplishedNotAchieved,
    AccomplishedUnknown,
    StopAchieved,
    StopNotAchieved,
    StopUnknown,
}
impl HumanClosureChoice {
    pub fn outcome(self) -> Option<bool> {
        match self {
            Self::AccomplishedAchieved | Self::StopAchieved => Some(true),
            Self::AccomplishedNotAchieved | Self::StopNotAchieved => Some(false),
            Self::AccomplishedUnknown | Self::StopUnknown => None,
        }
    }
}

/// Constructed by authenticated host interaction, never accepted by MCP.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeGoalClosure {
    pub event_id: String,
    pub expected_spec_digest: String,
    pub source: GoalSource,
    pub occurred_at: DateTime<Utc>,
    pub choice: HumanClosureChoice,
    pub evidence_manifest: Vec<GoalEvidence>,
    /// Set only after native disclosure and an actual ActionChoice.
    /// An outcome alone never grants permission to read or share artifacts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_access: Option<LocalArtifactAccessApproval>,
}
/// Trusted native correction; original closure remains unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NativeGoalOutcomeCorrection {
    pub event_id: String,
    pub expected_spec_digest: String,
    pub source: GoalSource,
    pub occurred_at: DateTime<Utc>,
    pub supersedes_event_id: String,
    pub outcome: Option<bool>,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalClosure {
    pub input: NativeGoalClosure,
    pub recorded_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEvidence {
    pub id: String,
    pub source: String,
    pub locator: String,
    pub digest: String,
    pub reliability: String,
    pub oracle: String,
    pub limitation: String,
}

/// Versioned native consent freezes the limits shown before closure. A future
/// wider reader requires a new policy and a new explicit human approval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalArtifactAccessApproval {
    BoundedUtf8V1,
}
pub const GOAL_ARTIFACT_MAX_FILES: usize = 16;
pub const GOAL_ARTIFACT_MAX_FILE_BYTES: usize = 64 * 1024;
pub const GOAL_ARTIFACT_MAX_TOTAL_BYTES: usize = 256 * 1024;

/// Host-observed input, never accepted through a worker MCP route. Fetched
/// content proves the bytes read, not that the worker's claimed oracle ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoalArtifactSnapshot {
    pub evidence_id: String,
    pub observed_at: DateTime<Utc>,
    pub outcome: GoalArtifactReadOutcome,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalArtifactReadOutcome {
    Fetched {
        source: String,
        sha256: String,
        byte_count: u64,
        content: String,
    },
    Unavailable {
        source: Option<String>,
        reason: String,
        observed_sha256: Option<String>,
        bytes_read: u64,
    },
}

/// Deliberately contains no progress verdict, closure choice, forecast or score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalAssessmentInput {
    pub goal_id: GoalID,
    pub spec_digest: String,
    pub goal_text: String,
    pub criteria: Vec<AssessmentCriterion>,
    pub evidence_manifest: Vec<GoalEvidence>,
    /// Exact bounded content and read failures persisted before inference.
    pub artifacts: Vec<GoalArtifactSnapshot>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssessmentCriterion {
    pub id: String,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AssessmentClaim {
    pub token: String,
    pub expires_at: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentAttempt {
    pub input: GoalAssessmentInput,
    pub started_at: DateTime<Utc>,
    /// The attempt's outcome as a human-readable audit record. The `Err`
    /// message is produced across the D8 kask_bridge seam — the artifact
    /// capture and the independent assessor both live there, where String
    /// errors are the accepted contract (check-string-errors gate scope
    /// note) — and is persisted as-is; no consumer matches on variants.
    pub result: Option<Result<AssessmentResult, String>>, // string-error-ok
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssessmentResult {
    pub findings: Vec<CriterionFinding>,
    pub limitations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionEvidenceStatus {
    Supported,
    Contradicted,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CriterionFinding {
    pub criterion_id: String,
    pub status: CriterionEvidenceStatus,
    pub evidence_ids: Vec<String>,
    pub note: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GoalForecast {
    pub proposition: String,
    pub probability: f64,
    pub horizon: DateTime<Utc>,
    pub resolution_deadline: DateTime<Utc>,
    pub execution_policy: String,
    pub resources: String,
    pub resolver: GoalForecastResolver,
    pub baseline_probability: f64,
    pub cohort: String,
    pub update_policy: ForecastUpdatePolicy,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GoalForecastResolver {
    HumanAcceptance {},
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForecastUpdatePolicy {
    FixedAtDeclaration,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastStatus {
    NotDeclared,
    PendingAssessment,
    PendingOutcome,
    Unknown,
    Resolved,
    DeadlineElapsed,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalForecastScore {
    pub version: u64,
    pub outcome_event_id: String,
    pub outcome: bool,
    pub brier: f64,
    pub baseline_brier: f64,
    pub skill: Option<f64>,
    pub supersedes_version: Option<u64>,
    pub recorded_at: DateTime<Utc>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalVerdictValue {
    Done,
    Continue,
    Blocked,
}
impl GoalVerdictValue {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Continue => "continue",
            Self::Blocked => "blocked",
        }
    }
}
impl std::fmt::Display for GoalVerdictValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriterionJudgment {
    pub index: usize,
    pub passed: bool,
    pub note: String,
}
/// Task-to-goal citation. Indices and verbatim text remain valid because the
/// canonical specification is immutable and survives indexing.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriterionCitation {
    pub goal_id: GoalID,
    pub criterion_index: usize,
    pub criterion_text: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalVerdict {
    pub verdict: GoalVerdictValue,
    pub confidence: f64,
    pub criterion_results: Vec<CriterionJudgment>,
    pub reasoning: String,
    pub judged_at: DateTime<Utc>,
}

/// The artifact-locator rejection is a display message, not a dispatch
/// surface: it is re-exported across the D8 kask_bridge seam
/// (`kask_bridge/src/goals/artifacts.rs` imports this as `local_path`),
/// where String errors are the accepted contract (check-string-errors gate
/// scope note). Alias-carried marker — a trailing marker on the `fn`
/// signature line does not survive rustfmt (the build.rs precedent).
pub type LocalArtifactPathResult = Result<std::path::PathBuf, String>; // string-error-ok

/// Parse only; safe to display before approval. Never touches the filesystem.
pub fn local_artifact_path(locator: &str) -> LocalArtifactPathResult {
    let path = if locator.starts_with("file:") {
        let url = url::Url::parse(locator).map_err(|error| format!("invalid file URI: {error}"))?;
        if !matches!(url.host_str(), None | Some("localhost")) {
            return Err("file URI must name the local host, never a remote authority".into());
        }
        if url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
        {
            return Err("file URI must not contain credentials, port, query or fragment".into());
        }
        url.to_file_path()
            .map_err(|_| "only local file URIs are supported".to_string())?
    } else {
        std::path::PathBuf::from(locator)
    };
    if !path.is_absolute() || path.to_str().is_none() {
        return Err("artifact must be an absolute UTF-8 local path or local file URI".into());
    }
    Ok(path)
}
impl Goal {
    pub fn needs_assessment(&self, now: DateTime<Utc>) -> bool {
        self.closure.is_some()
            && !self
                .assessments
                .last()
                .is_some_and(|a| matches!(a.result, Some(Ok(_))))
            && !self
                .assessment_claim
                .as_ref()
                .is_some_and(|c| c.expires_at > now)
    }

    /// The goal's trust summary, computed from the running trust
    /// ledger (`trust_events`). The regulation loop reads the
    /// violation rates from here — the count of integrity-severity
    /// violations is the escalation signal (a discovered integrity
    /// failure resets trust to the prior, per Slovic 1993 and
    /// Kim–Ferrin–Cooper–Dirks 2004).
    pub fn trust_summary(&self) -> GoalTrustSummary {
        let total = self.trust_events.len();
        let complied = self
            .trust_events
            .iter()
            .filter(|e| e.status == GoalTrustStatus::Complied)
            .count();
        let violated_competence = self
            .trust_events
            .iter()
            .filter(|e| {
                matches!(
                    (e.status, e.severity),
                    (
                        GoalTrustStatus::Violated,
                        Some(GoalTrustSeverity::Competence)
                    )
                )
            })
            .count();
        let violated_integrity = self
            .trust_events
            .iter()
            .filter(|e| {
                matches!(
                    (e.status, e.severity),
                    (
                        GoalTrustStatus::Violated,
                        Some(GoalTrustSeverity::Integrity)
                    )
                )
            })
            .count();
        GoalTrustSummary {
            total_instructions: total,
            complied,
            violated_competence,
            violated_integrity,
            has_integrity_violation: violated_integrity > 0,
        }
    }
}
