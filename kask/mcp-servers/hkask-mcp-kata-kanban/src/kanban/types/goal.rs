//! Canonical verification ledger. Human closure, assessment, forecasts and
//! retrieval delivery are distinct records; none erases the original goal.
use super::*;
use schemars::JsonSchema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub schema_version: u32,
    pub id: GoalID,
    pub goal_text: String,
    pub criteria: Vec<VerificationCriterion>,
    pub criterion_ids: Vec<String>,
    pub task_id: Option<TaskId>,
    pub source: Option<GoalSource>,
    pub supersedes: Option<GoalID>,
    pub spec_digest: String,
    pub verdicts: Vec<GoalVerdict>,
    pub closure: Option<GoalClosure>,
    pub human_corrections: Vec<NativeGoalOutcomeCorrection>,
    pub assessment_pending: bool,
    pub assessments: Vec<AssessmentAttempt>,
    pub forecast: Option<GoalForecast>,
    pub observations: Vec<ForecastObservation>,
    pub forecast_status: ForecastStatus,
    pub scores: Vec<GoalForecastScore>,
    pub projections: Vec<GoalProjection>,
    pub revision: u64,
    pub legacy: Option<LegacyGoalArchive>,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalDeclaration {
    pub goal_text: String,
    pub criteria: Vec<VerificationCriterion>,
    pub task_id: Option<TaskId>,
    pub source: GoalSource,
    pub forecast: Option<GoalForecast>,
    pub supersedes: Option<GoalID>,
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
    /// Set only after native disclosure and an actual ActionChoice. Historical
    /// closure is not retroactive permission to read or share local artifacts.
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
    pub payload_digest: String,
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
    /// Empty historical inputs retain their original serialization/digest.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<GoalArtifactSnapshot>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssessmentCriterion {
    pub id: String,
    pub description: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentLeaseRequest {
    pub attempt_id: String,
    pub assessor_id: String,
    pub model_version: String,
    pub tool_version: String,
    pub lease_until: DateTime<Utc>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentAttempt {
    pub lease: AssessmentLeaseRequest,
    pub input: GoalAssessmentInput,
    pub input_digest: String,
    pub started_at: DateTime<Utc>,
    pub completion: Option<AssessmentCompletion>,
    pub failure: Option<AssessmentFailure>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentCompletion {
    pub attempt_id: String,
    pub input_digest: String,
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
pub struct CriterionFinding {
    pub criterion_id: String,
    pub status: CriterionEvidenceStatus,
    pub evidence_ids: Vec<String>,
    pub note: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentFailure {
    pub attempt_id: String,
    pub input_digest: String,
    pub reason: String,
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
    HumanAcceptance,
    MachineEvidence { source: String, oracle: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ForecastUpdatePolicy {
    FixedAtDeclaration,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForecastObservation {
    pub event_id: String,
    pub source: String,
    pub oracle: String,
    pub occurred_at: DateTime<Utc>,
    pub outcome: Option<bool>,
    pub evidence_ids: Vec<String>,
    pub correction_of: Option<String>,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForecastStatus {
    NotDeclared,
    PendingAssessment,
    PendingOutcome,
    Unknown,
    Disputed,
    Resolved,
    DeadlineElapsed,
    LegacyUnscored,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalProjection {
    pub event_id: String,
    pub revision: u64,
    pub payload_digest: String,
    pub payload: serde_json::Value,
    pub delivered_receipt: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalProjectionReceipt {
    pub event_id: String,
    pub revision: u64,
    pub payload_digest: String,
    pub receipt_id: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LegacyGoalArchive {
    pub raw: serde_json::Value,
    pub limitation: String,
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
/// canonical specification is immutable and survives retrieval receipts.
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
