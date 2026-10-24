use super::*;

/// The durable Improvement Kata state for a task — the minimum the next
/// session needs to continue the routine instead of restarting from zero:
/// the target condition (step 3's artifact), the one focus obstacle
/// selected against it, and the last PDCA experiment with its prediction
/// and observed outcome (step 4's running record). The authoritative
/// methodology lives in the `kata-improvement` skill; this is the durable
/// state it works over, recorded through `kanban_task_kata_record` and read
/// back by every kata prompt (`service_impl/kata.rs`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct TaskKataState {
    /// The measurable target condition the learner is converging toward.
    pub target_condition: String,
    /// The ONE obstacle currently being worked (`None`: none recorded yet).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus_obstacle: Option<String>,
    /// The last PDCA experiment against the focus obstacle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_experiment: Option<KataExperimentRecord>,
}

/// One PDCA experiment record: the next step taken, the prediction it
/// tested, and the observed outcome once checked (`None` until checked —
/// never filled from a plan).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct KataExperimentRecord {
    /// The next step taken (one change at a time).
    pub next_step: String,
    /// The prediction the step tests — what you expect to learn.
    pub prediction: String,
    /// The observed outcome once checked; `None` until checked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}
