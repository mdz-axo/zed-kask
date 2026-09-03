//! PKO (Procedural Knowledge Ontology) mapping for hkask-mcp-kata-kanban.
//!
//! Maps kanban types and field names to PKO standard concepts. PKO is the
//! ISWC 2024 process ontology from the EU PERKS project (Grant 101120323).
//! PKO extends P-Plan and PROV-O via soft reuse.
//!
//! Reference: <https://w3id.org/pko> — Carriero et al., arXiv:2503.20634
//!
//! The PKO/PROV-O *vocabulary* lives in the shared bridge crate
//! (`hkask_bridge_ontology::pko`) — this module is only the kanban-specific
//! type→concept table, so the vocabulary cannot drift between crates. Key
//! PKO modules used:
//! - pko:Procedure                 — procedure specification (board)
//! - pko:Step / pko:MultiStep      — atomic/composite steps (tasks)
//! - pko:StepExecution             — step occurrence (in-progress task)
//! - pko:StepVerification          — verification of step completion
//! - pko:ChangeOfStatus             — status transitions
//! - pko:UserFeedbackOccurrence    — feedback left during execution
//! - pko:UserQuestionOccurrence    — questions asked during execution
//! - pko:IssueOccurrence           — errors/problems encountered
//! - ppro:RoleInTime               — agent role scoped to time period
//! - prov:Agent / prov:Activity    — PROV-O provenance base

// The vocabulary is the bridge crate's — re-exported so call sites that
// reference `pko::<CONSTANT>` keep resolving unchanged.
pub use hkask_bridge_ontology::pko::{
    AGENT, CHANGE_OF_STATUS, ERROR, HAS_EXPECTED_DURATION, ISSUE_OCCURRENCE, PROCEDURE,
    PROCEDURE_EXECUTION, PROCEDURE_EXECUTION_STATUS, STEP, STEP_EXECUTION, STEP_VERIFICATION, USED,
    USER_FEEDBACK_OCCURRENCE, USER_QUESTION_OCCURRENCE, WAS_ASSOCIATED_WITH, WAS_GENERATED_BY,
};

/// A PKO concept curie — namespace-prefixed short form.
/// e.g. "pko:StepExecution", "pko:UserFeedbackOccurrence"
pub type PkoConcept = &'static str;

// ── Kanban type → PKO concept mapping ──────────────────────────────────────

/// Map a kanban type name to its PKO concept URI.
/// Returns None for internal types without a PKO equivalent.
pub(crate) fn kanban_type_to_pko(type_name: &str) -> Option<PkoConcept> {
    match type_name {
        "Board"
        | "kanban_board_create"
        | "kanban_board_list"
        | "kanban_board_delete"
        | "kanban_board_export"
        | "kanban_board_import" => Some(PROCEDURE),
        "Task" | "kanban_task_create" | "kanban_task_list" => Some(STEP),
        "Goal" | "kanban_goal_create" | "kanban_goal_list" => Some(STEP),
        "kanban_goal_judge" | "kanban_goal_score" | "GoalVerdict" | "GoalResolution" => {
            Some(STEP_VERIFICATION)
        }
        "Task.decomposed" | "kanban_task_decompose" => Some(hkask_bridge_ontology::pko::MULTI_STEP),
        "Task.execution" | "Task.in_progress" | "kanban_task_spawn" => Some(STEP_EXECUTION),
        "Board.execution" => Some(PROCEDURE_EXECUTION),
        "Column" | "TaskStatus" => Some(PROCEDURE_EXECUTION_STATUS),
        "kanban_task_move" | "kanban_task_reopen" => Some(CHANGE_OF_STATUS),
        "kanban_task_verify" | "Verification" | "VerificationCriterion" => Some(STEP_VERIFICATION),
        "Comment" | "kanban_task_comment" => Some(USER_FEEDBACK_OCCURRENCE),
        "Comment.question"
        | "kanban_task_kata_prompt" => Some(USER_QUESTION_OCCURRENCE),
        "UnjamItem" | "kanban_unjam" => Some(ISSUE_OCCURRENCE),
        "UnjamItem.error" => Some(ERROR),
        "Assignee" | "kanban_task_assign" => Some(WAS_ASSOCIATED_WITH),
        "Deliverable" | "kanban_task_add_deliverable" => Some(WAS_GENERATED_BY),
        "SpendEntry" | "kanban_task_add_rjoules" => Some(USED),
        "estimated_hours" => Some(HAS_EXPECTED_DURATION),
        "Agent" | "assignee" => Some(AGENT),
        _ => None,
    }
}
