//! Skill step tracker — records tool calls made during a skill invocation
//! so the consumer of the skill output has calibration information about
//! which steps were actually executed.
//!
//! Activated when the `skill` tool runs (model-driven) or when
//! `send_skill_invocation` fires (slash command). Every subsequent tool
//! call in the same turn is recorded. At turn end, the tracker produces a
//! `SkillStepReport` that travels with the `ThreadTurnRecord` into memory
//! ingestion and reg span emission.
//!
//! This is observability, not enforcement — the model remains the executor
//! and can adapt the flow. The report gives the consumer (the operator, a
//! downstream skill, the curator) the ground truth of which tools were
//! called, in what order, so they can calibrate trust in the skill output.

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
    /// `None` when no skill is active (normal conversation turns).
    skill_name: Option<String>,
    /// Ordered sequence of tool names called during the active skill
    /// invocation. Empty when no skill is active.
    tool_call_sequence: Vec<String>,
}

impl SkillStepTracker {
    /// Construct a dormant tracker (no skill active).
    pub fn new() -> Self {
        Self {
            skill_name: None,
            tool_call_sequence: Vec::new(),
        }
    }

    /// Activate tracking for a skill invocation. Called when the `skill`
    /// tool runs or when `send_skill_invocation` fires. Resets any prior
    /// state so a second skill invocation in the same turn starts clean.
    pub fn activate(&mut self, skill_name: String) {
        self.skill_name = Some(skill_name);
        self.tool_call_sequence.clear();
    }

    /// Record a tool call. No-op when no skill is active — normal
    /// conversation turns (no skill invoked) don't accumulate tool calls.
    pub fn record(&mut self, tool_name: &str) {
        if self.skill_name.is_some() {
            self.tool_call_sequence.push(tool_name.to_string());
        }
    }

    /// Whether a skill is currently being tracked.
    pub fn is_active(&self) -> bool {
        self.skill_name.is_some()
    }

    /// Consume the tracker and produce a report. Resets to dormant state
    /// so the next turn starts fresh. Returns `None` when no skill was
    /// active (normal conversation turn — no calibration info to report).
    pub fn finalize(&mut self) -> Option<SkillStepReport> {
        let name = self.skill_name.take()?;
        let calls = std::mem::take(&mut self.tool_call_sequence);
        Some(SkillStepReport {
            skill_name: name,
            tool_call_sequence: calls,
        })
    }
}

impl Default for SkillStepTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// Calibration report for a completed skill invocation. Travels with
/// the `ThreadTurnRecord` into memory ingestion, where the bridge emits
/// a `reg.curation.skill_verification` span and stores the report for
/// `curator_memory_recall`.
///
/// The consumer of a skill output reads this report to calibrate trust:
/// if the SKILL.md declared steps that don't appear in `tool_call_sequence`,
/// the output may be incomplete. This is calibration, not a gate — the
/// consumer decides whether to trust the output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SkillStepReport {
    /// The skill that was invoked (e.g. "gemba-walk", "algedonic-review").
    pub skill_name: String,
    /// Ordered sequence of tool names called during the skill invocation.
    /// The consumer compares this against the SKILL.md's declared steps
    /// to identify skipped or extra tool calls.
    pub tool_call_sequence: Vec<String>,
}
