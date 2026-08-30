use super::*;

// ── Task ───────────────────────────────────────────────────────────────────

/// TaskSpec — input specification for creating a new task.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TaskSpec {
    /// Short title for the task.
    pub title: String,
    /// Optional longer description.
    pub description: Option<String>,
    /// Acceptance criteria — what "done" means.
    pub criteria: Vec<VerificationCriterion>,
    /// Goal criteria this task advances — the functional–technical join.
    /// Validated against the cited goal at creation; documentation-grade
    /// thereafter (see `CriterionCitation`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advances: Vec<CriterionCitation>,

    /// Story points for relative sizing (agile convention).
    pub story_points: Option<u32>,
    /// Estimated hours for completion.
    pub estimated_hours: Option<f64>,
    /// Labels/tags for categorization.
    pub labels: Vec<String>,
    /// Priority level.
    pub priority: Option<Priority>,
    /// Optional phase grouping.
    pub phase_id: Option<PhaseId>,
    /// Inference/API rJoule budget (250k rJoules ≈ $1 inference spend).
    pub rjoule_budget: Option<u64>,
}

impl TaskSpec {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  title is non-empty
    /// post: returns a TaskSpec with no description or criteria
    pub fn new(title: String) -> Self {
        Self {
            title,
            description: None,
            criteria: Vec::new(),
            advances: Vec::new(),

            story_points: None,
            estimated_hours: None,
            labels: Vec::new(),
            priority: None,
            phase_id: None,
            rjoule_budget: None,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is valid
    /// post: returns self with description set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_description(mut self, desc: String) -> Self {
        self.description = Some(desc);
        self
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is valid
    /// post: returns self with criteria set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_criteria(mut self, criteria: Vec<VerificationCriterion>) -> Self {
        self.criteria = criteria;
        self
    }
}
