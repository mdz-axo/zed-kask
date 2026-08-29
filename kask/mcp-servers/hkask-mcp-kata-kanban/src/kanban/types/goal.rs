use super::*;

// ── Goal (functional target condition) ────────────────────────────────────

/// Goal — a functional goal with observable verification criteria.
///
/// The native persistence for the four-moves interaction loop
/// (`kask/docs/architecture/functional-interaction-spec.md`): the goal is
/// the kata *target condition* — the user's functional requirement, in the
/// user's words, with Fermi-decomposed observable criteria. The agent's
/// intake prediction (`probability the goal is achieved`) is Brier-scored
/// at resolution, so the agent's functional understanding becomes a
/// calibrated, measurable signal across sessions.
///
/// Schema lifted from the `goal-analysis` skill (`create.j2` / `judge.j2`):
/// `goal_text` + observable `criteria` + verdict semantics
/// `done` / `continue` / `blocked` with confidence.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    /// Unique goal identifier.
    pub id: GoalID,
    /// The functional goal in the user's words — what the user will be able
    /// to do, or what stops being a problem. The agent interprets this; it
    /// never revises it.
    pub goal_text: String,
    /// Observable criteria (Fermi-decomposed from the goal): 2–4 conditions
    /// phrased functionally ("the user can do X", "Y no longer breaks").
    pub criteria: Vec<VerificationCriterion>,
    /// Optional link to the kanban task executing this goal.
    pub task_id: Option<TaskId>,
    /// The agent's intake prediction: probability (0.0–1.0) that the goal
    /// will be achieved. `None` when no prediction was recorded — then
    /// `goal_score` reports `brier: null` with a note, never a synthetic 0.
    pub prediction: Option<f64>,
    /// Judge history — every recorded verdict, newest last. The history IS
    /// the learning: drift shows up as repeated `continue` verdicts.
    pub verdicts: Vec<GoalVerdict>,
    /// Resolution, once scored.
    pub resolution: Option<GoalResolution>,
    /// The agent who created this goal (P12).
    pub owner: WebID,
    /// When the goal was created.
    pub created_at: DateTime<Utc>,
    /// When the goal was last updated.
    pub updated_at: DateTime<Utc>,
}

impl Goal {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  id assigned by the service; goal_text non-empty; 1–4 criteria
    /// post: returns a Goal with empty verdicts and no resolution
    pub fn new(
        id: GoalID,
        goal_text: String,
        criteria: Vec<VerificationCriterion>,
        owner: WebID,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            goal_text,
            criteria,
            task_id: None,
            prediction: None,
            verdicts: Vec::new(),
            resolution: None,
            owner,
            created_at: now,
            updated_at: now,
        }
    }
}

// ── Goal Verdict ──────────────────────────────────────────────────────────

/// GoalVerdictValue — the judge verdict, lifted from `goal-analysis`'s
/// `judge.j2` semantics.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GoalVerdictValue {
    /// All criteria satisfied — the goal is achieved.
    Done,
    /// Criteria not yet satisfied — work continues.
    Continue,
    /// The goal is unachievable as stated or needs user input.
    Blocked,
}

impl GoalVerdictValue {
    /// Wire-format name (matches the serde lowercase rename and the
    /// `goal-analysis` judge vocabulary).
    pub fn as_str(&self) -> &'static str {
        match self {
            GoalVerdictValue::Done => "done",
            GoalVerdictValue::Continue => "continue",
            GoalVerdictValue::Blocked => "blocked",
        }
    }
}

impl std::fmt::Display for GoalVerdictValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// CriterionJudgment — the judge's result for one criterion.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CriterionJudgment {
    /// Index into `Goal::criteria` this judgment refers to.
    pub index: usize,
    /// Whether the criterion is satisfied by the observed outcome.
    pub passed: bool,
    /// Evidence-grounded note for this criterion.
    pub note: String,
}

/// GoalVerdict — one recorded judgment of the goal against its criteria.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalVerdict {
    /// The verdict.
    pub verdict: GoalVerdictValue,
    /// Confidence in the verdict (0.0–1.0).
    pub confidence: f64,
    /// Per-criterion results.
    pub criterion_results: Vec<CriterionJudgment>,
    /// Overall reasoning, grounded in the observed outcome.
    pub reasoning: String,
    /// When the verdict was recorded.
    pub judged_at: DateTime<Utc>,
}

// ── Goal Resolution ────────────────────────────────────────────────────────

/// GoalResolution — the scored outcome of a goal.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalResolution {
    /// Whether the goal was achieved (the user's ground truth).
    pub achieved: bool,
    /// Brier score of the intake prediction against the realized outcome.
    /// `None` when no prediction was recorded — surfaced, never faked.
    pub brier: Option<f64>,
    /// When the resolution was recorded.
    pub resolved_at: DateTime<Utc>,
}
