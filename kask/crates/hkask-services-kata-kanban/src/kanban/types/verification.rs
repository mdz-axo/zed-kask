use super::*;

// ── Verification Criterion ─────────────────────────────────────────────────

/// VerificationCriterion — an acceptance criterion for task completion.
///
/// Holds a natural-language specification of what "done" means for this task,
/// plus an optional LLM evaluation prompt for automated verification.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCriterion {
    /// Human-readable acceptance spec.
    pub description: String,
    /// Optional prompt for LLM-mediated evaluation.
    pub llm_prompt: Option<String>,
}

impl VerificationCriterion {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  description is non-empty
    /// post: returns a VerificationCriterion with no LLM prompt
    pub fn new(description: String) -> Self {
        Self {
            description,
            llm_prompt: None,
        }
    }

    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  self is valid; llm_prompt is non-empty
    /// post: returns self with llm_prompt set
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_llm_prompt(mut self, prompt: String) -> Self {
        self.llm_prompt = Some(prompt);
        self
    }
}

// ── Verification Result ────────────────────────────────────────────────────

/// Verification — result of task verification.
///
/// Produced by `task_verify`: either an LLM-mediated evaluation against
/// the task's acceptance criteria, or a human-in-the-loop confirmation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    /// Whether the task passed verification.
    pub passed: bool,
    /// Human-readable reasoning for the verdict.
    pub reasoning: String,
    /// The WebID of the verifier (LLM userpod or human).
    pub verifier: WebID,
    /// When the verification occurred.
    pub verified_at: DateTime<Utc>,
}

impl Verification {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  verifier is a valid WebID
    /// post: returns Verification with verified_at=now
    pub fn new(passed: bool, reasoning: String, verifier: WebID) -> Self {
        Self {
            passed,
            reasoning,
            verifier,
            verified_at: Utc::now(),
        }
    }
}
