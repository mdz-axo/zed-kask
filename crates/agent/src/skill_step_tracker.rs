//! Skill step tracker — verifies that a skill invocation executed its
//! declared steps, producing a trust verdict the consumer of the skill
//! output can calibrate against.
//!
//! ## The trust loop
//!
//! ```text
//! DECLARE  (SKILL.md frontmatter: `steps:` — what the skill says it will do)
//!    ↓
//! EXECUTE  (model follows the SKILL.md adaptively — every tool call recorded)
//!    ↓
//! VERIFY   (tracker compares actual tool calls against declared steps)
//!    ↓
//! VERDICT  (Verified | Incomplete { missing } | NoDeclaration)
//!    ↓
//! SURFACE  (verdict travels with ThreadTurnRecord → reg span → consumer reads it)
//!    ↓
//! FEEDBACK (incomplete verdict → reg.curation span with missing steps →
//!           curator's regulation loop detects the gap)
//! ```
//!
//! The model remains the adaptive executor. The tracker is observability
//! infrastructure — it does not gate execution. The consumer of the skill
//! output reads the verdict to calibrate trust: a `Verified` output can be
//! trusted; an `Incomplete` output is suspect and the missing steps tell
//! you what was skipped.

use std::collections::HashMap;
use std::sync::Mutex;

pub use agent_skills::SkillStepDeclaration as DeclaredStep;

/// Per-thread tracker for tool calls made during a skill invocation.
///
/// Lives on `Thread` alongside `ToolRetryTracker`. Activated by
/// `handle_tool_use_event` when the tool name is `skill` (the model called
/// the skill tool) or by `send_skill_invocation` (slash command path).
/// Deactivated and consumed at turn end by `run_turn`.
///
/// Never persisted — lives and dies with the thread, like `ToolRetryTracker`.
pub struct SkillStepTracker {
    /// The name of the skill currently being executed, if any.
    skill_name: Option<String>,
    /// Steps declared in the skill's frontmatter, looked up from the
    /// process-global registry at activation time. `None` when the skill
    /// has no step declaration — the report will carry `NoDeclaration`.
    declared_steps: Option<Vec<DeclaredStep>>,
    /// Ordered sequence of tool names called during the active skill
    /// invocation. Empty when no skill is active.
    tool_call_sequence: Vec<String>,
}

/// The trust verdict for a completed skill invocation.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SkillVerificationVerdict {
    /// All declared steps were executed — every required tool was called
    /// at least once. The consumer can trust the skill output.
    Verified,
    /// Some declared steps were not executed. The consumer should treat
    /// the skill output as suspect — the missing steps indicate what the
    /// model skipped.
    Incomplete {
        /// Step IDs that were not fully executed (missing at least one
        /// required tool).
        missing_steps: Vec<String>,
    },
    /// The skill has no step declaration in its frontmatter. The tracker
    /// recorded the tool-call sequence but cannot produce a verdict.
    /// The consumer has raw calibration data but no trust signal.
    NoDeclaration,
}

/// Calibration report for a completed skill invocation. Travels with
/// the `ThreadTurnRecord` into memory ingestion, where the bridge emits
/// a `reg.curation.skill_verification` span and stores the report.
///
/// The consumer of a skill output reads this report to calibrate trust.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillStepReport {
    /// The skill that was invoked (e.g. "gemba-walk", "algedonic-review").
    pub skill_name: String,
    /// The trust verdict — the "answer" the consumer reads.
    pub verdict: SkillVerificationVerdict,
    /// Ordered sequence of tool names called during the skill invocation.
    /// The raw execution trace for calibration.
    pub tool_call_sequence: Vec<String>,
}

impl SkillStepTracker {
    pub fn new() -> Self {
        Self {
            skill_name: None,
            declared_steps: None,
            tool_call_sequence: Vec::new(),
        }
    }

    /// Activate tracking for a skill invocation. Looks up the skill's
    /// declared steps from the process-global registry. Called when the
    /// `skill` tool runs or when `send_skill_invocation` fires.
    pub fn activate(&mut self, skill_name: String) {
        let declared_steps = lookup_skill_steps(&skill_name);
        self.skill_name = Some(skill_name);
        self.declared_steps = declared_steps;
        self.tool_call_sequence.clear();
    }

    /// Record a tool call. No-op when no skill is active.
    pub fn record(&mut self, tool_name: &str) {
        if self.skill_name.is_some() {
            self.tool_call_sequence.push(tool_name.to_string());
        }
    }

    /// Whether a skill is currently being tracked.
    pub fn is_active(&self) -> bool {
        self.skill_name.is_some()
    }

    /// Consume the tracker and produce a report. Resets to dormant state.
    /// Returns `None` when no skill was active (normal conversation turn).
    pub fn finalize(&mut self) -> Option<SkillStepReport> {
        let skill_name = self.skill_name.take()?;
        let declared_steps = self.declared_steps.take();
        let calls = std::mem::take(&mut self.tool_call_sequence);

        let verdict = match &declared_steps {
            None => SkillVerificationVerdict::NoDeclaration,
            Some(steps) => {
                let missing: Vec<String> = steps
                    .iter()
                    .filter(|step| {
                        !step.tools.is_empty()
                            && !step
                                .tools
                                .iter()
                                .all(|tool| calls.iter().any(|c| c == tool))
                    })
                    .map(|step| step.id.clone())
                    .collect();
                if missing.is_empty() {
                    SkillVerificationVerdict::Verified
                } else {
                    SkillVerificationVerdict::Incomplete {
                        missing_steps: missing,
                    }
                }
            }
        };

        Some(SkillStepReport {
            skill_name,
            verdict,
            tool_call_sequence: calls,
        })
    }
}

impl Default for SkillStepTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ── Process-global step registry ───────────────────────────────────────────
//
// Populated when skills are loaded (the skill catalog build path calls
// `register_skill_steps`). Read by `SkillStepTracker::activate` via
// `lookup_skill_steps`. Same pattern as `METACOGNITION_PROVIDER` and the
// other process-global hooks — the composition root populates, the
// per-thread consumer reads.

static SKILL_STEPS_REGISTRY: std::sync::OnceLock<Mutex<HashMap<String, Vec<DeclaredStep>>>> =
    std::sync::OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, Vec<DeclaredStep>>> {
    SKILL_STEPS_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a skill's declared steps. Called from the skill loading path
/// when the frontmatter contains a `steps` field. Re-registerable —
/// reloading a skill replaces its steps.
pub fn register_skill_steps(skill_name: String, steps: Vec<DeclaredStep>) {
    if let Ok(mut reg) = registry().lock() {
        reg.insert(skill_name, steps);
    }
}

fn lookup_skill_steps(skill_name: &str) -> Option<Vec<DeclaredStep>> {
    registry().lock().ok()?.get(skill_name).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, tools: &[&str]) -> DeclaredStep {
        DeclaredStep {
            id: id.to_string(),
            tools: tools.iter().map(|t| t.to_string()).collect(),
        }
    }

    #[test]
    fn test_skill_step_tracker_verifies_all_steps_present() {
        register_skill_steps(
            "test-verified".to_string(),
            vec![
                step("sense", &["curator_escalations", "curator_algedonic_log"]),
                step("synthesize", &["render_template"]),
                step("converge", &["lisp_eval"]),
            ],
        );
        let mut tracker = SkillStepTracker::new();
        tracker.activate("test-verified".to_string());
        tracker.record("curator_escalations");
        tracker.record("curator_algedonic_log");
        tracker.record("render_template");
        tracker.record("lisp_eval");
        let report = tracker.finalize().expect("report should be produced");
        assert_eq!(report.skill_name, "test-verified");
        assert!(matches!(report.verdict, SkillVerificationVerdict::Verified));
        assert_eq!(report.tool_call_sequence.len(), 4);
    }

    #[test]
    fn test_skill_step_tracker_reports_incomplete() {
        register_skill_steps(
            "test-incomplete".to_string(),
            vec![
                step("sense", &["curator_escalations", "curator_algedonic_log"]),
                step("synthesize", &["render_template"]),
                step("converge", &["lisp_eval"]),
            ],
        );
        let mut tracker = SkillStepTracker::new();
        tracker.activate("test-incomplete".to_string());
        // Skip curator_algedonic_log and lisp_eval entirely
        tracker.record("curator_escalations");
        tracker.record("render_template");
        let report = tracker.finalize().expect("report should be produced");
        match &report.verdict {
            SkillVerificationVerdict::Incomplete { missing_steps } => {
                assert!(
                    missing_steps.contains(&"sense".to_string()),
                    "sense should be missing because curator_algedonic_log was not called"
                );
                assert!(
                    missing_steps.contains(&"converge".to_string()),
                    "converge should be missing because lisp_eval was not called"
                );
                assert!(
                    !missing_steps.contains(&"synthesize".to_string()),
                    "synthesize should not be missing"
                );
            }
            other => panic!("expected Incomplete, got {:?}", other),
        }
    }

    #[test]
    fn test_skill_step_tracker_no_declaration() {
        let mut tracker = SkillStepTracker::new();
        tracker.activate("skill-with-no-steps".to_string());
        tracker.record("curator_escalations");
        tracker.record("render_template");
        let report = tracker.finalize().expect("report should be produced");
        assert!(matches!(
            report.verdict,
            SkillVerificationVerdict::NoDeclaration
        ));
        assert_eq!(report.tool_call_sequence.len(), 2);
    }

    #[test]
    fn test_skill_step_tracker_inactive_returns_none() {
        let mut tracker = SkillStepTracker::new();
        // No activation — normal conversation turn
        tracker.record("grep");
        tracker.record("read_file");
        assert!(
            tracker.finalize().is_none(),
            "finalize without activation should return None"
        );
    }

    #[test]
    fn test_skill_step_tracker_finalize_resets_to_dormant() {
        register_skill_steps("test-reset".to_string(), vec![step("one", &["grep"])]);
        let mut tracker = SkillStepTracker::new();
        tracker.activate("test-reset".to_string());
        tracker.record("grep");
        let _ = tracker.finalize();
        // After finalize, tracker should be dormant — second finalize returns None
        assert!(
            tracker.finalize().is_none(),
            "second finalize after reset should return None"
        );
        assert!(!tracker.is_active());
    }

    #[test]
    fn test_skill_step_tracker_step_with_no_tools_always_verified() {
        // A step with empty tools list is always considered executed
        register_skill_steps(
            "test-empty-tools".to_string(),
            vec![
                step("action", &["grep"]),
                step("reasoning", &[]), // no tool dependency
            ],
        );
        let mut tracker = SkillStepTracker::new();
        tracker.activate("test-empty-tools".to_string());
        tracker.record("grep");
        let report = tracker.finalize().expect("report");
        assert!(
            matches!(report.verdict, SkillVerificationVerdict::Verified),
            "step with no tools should always be verified"
        );
    }
}
