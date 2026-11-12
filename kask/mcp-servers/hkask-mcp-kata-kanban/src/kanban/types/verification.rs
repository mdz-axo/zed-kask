use super::*;

// ── Verification Criterion ─────────────────────────────────────────────────

/// VerificationCriterion — an acceptance criterion for a task or a goal.
///
/// A natural-language statement of what "done" means. Unknown keys are
/// ignored on decode, so rows written while the type carried an
/// always-null `llm_prompt` still read.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationCriterion {
    /// Human-readable acceptance spec.
    pub description: String,
}

impl VerificationCriterion {
    /// expect: "System types preserve semantic identity and are provenance-aware"
    /// pre:  description is non-empty
    /// post: returns a VerificationCriterion carrying the description
    pub fn new(description: String) -> Self {
        Self { description }
    }
}

// ── Verification Result ────────────────────────────────────────────────────

/// Verification — result of task verification.
///
/// Produced by `task_verify`: a human-in-the-loop confirmation — the user's
/// evidence text is the pass signal (criteria guide work but do not gate
/// completion; there is no LLM-mediated evaluation path in task_verify).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    /// Whether the task passed verification.
    pub passed: bool,
    /// Human-readable reasoning for the verdict.
    pub reasoning: String,
    /// The WebID of the verifier (LLM agent or human).
    pub verifier: WebID,
    /// When the verification occurred.
    pub verified_at: DateTime<Utc>,
}

impl Verification {
    pub fn new(passed: bool, reasoning: String, verifier: WebID) -> Self {
        Self {
            passed,
            reasoning,
            verifier,
            verified_at: Utc::now(),
        }
    }
}
