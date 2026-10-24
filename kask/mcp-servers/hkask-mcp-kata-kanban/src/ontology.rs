//! Dual-axis ontology dispatch for hkask-mcp-kata-kanban (P5.4, operator
//! ruling B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the kata-kanban
//! server's mapping onto those two axes, wired as the central
//! `ToolContext::annotate_response` hook via the `mcp_server!` macro's
//! annotate override — one point stamps every tool response at the
//! `execute_tool` boundary, replacing the 17 per-site flat-response
//! `ontology: kanban_type_to_pko(...)` emissions (operator ruling,
//! option (b) migration 2026-10-18).
//!
//! - **Process axis (PKO)** — follows the semantics the server's own
//!   `pko.rs` established (its type→concept table is retained for the
//!   NESTED per-item tags the hook cannot own): board lifecycle tools
//!   execute the board procedure's steps; goal tools verify and record
//!   outcomes; task-write tools change task state (creation,
//!   status change, assignment, deliverables, deletion); read tools
//!   list/observe stored records; the kata prompt asks a coaching
//!   question; spawn/delegate execute a step's delegation.
//! - **State axis (DC+BIBO)** — write responses are datasets (a stored
//!   or modified measurement); list/read responses are compiled
//!   readings over many stored records — reports.
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology`
//! crate; this module holds only the kata-kanban-specific tool→concept
//! table. That is the server's business, not the ontology's — the
//! architectural invariant (user directive 2026-08-05) that no ontology
//! vocabulary is defined inside an MCP server.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{
    CHANGE_OF_STATUS, STEP, STEP_EXECUTION, STEP_VERIFICATION, USER_QUESTION_OCCURRENCE,
    WAS_ASSOCIATED_WITH,
};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a kata-kanban tool's RESPONSE occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
///
/// The concepts here are response-level: what the CALL did, matching the
/// flat-response emissions this module replaces (the per-item nested
/// tags in `pko.rs` describe what the ITEM is — Board/Task/Goal — and
/// stay per-item).
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Board lifecycle: create/delete/import/export/list/update each
        // execute one step of the board-management procedure (the
        // persisted board record is pko:Procedure; these calls act on
        // it).
        "kanban_board_create"
        | "kanban_board_delete"
        | "kanban_board_import"
        | "kanban_board_export"
        | "kanban_board_list"
        | "kanban_board_update" => Some(STEP_EXECUTION),
        // Worker declaration/progress are verification steps; native closure
        // and recall indexing are intentionally outside the tool surface.
        "kanban_goal_create" | "kanban_goal_judge" | "kanban_goal_list" | "kanban_goal_get" => {
            Some(STEP_VERIFICATION)
        }
        // Task state changes: create/move/reopen/assign/unassign/delete/
        // comment/add-deliverable/update each change a task's state.
        "kanban_task_create"
        | "kanban_task_move"
        | "kanban_task_reopen"
        | "kanban_task_assign"
        | "kanban_task_unassign"
        | "kanban_task_delete"
        | "kanban_task_comment"
        | "kanban_task_add_deliverable"
        | "kanban_task_update" => Some(CHANGE_OF_STATUS),
        // Reading tasks/comments/delegation results observes stored
        // records — compiled readings over prior executions.
        "kanban_task_list" | "kanban_task_comments_since" | "kanban_task_delegate_result" => {
            Some(STEP)
        }
        // Task verification records the Review-column evidence — the
        // verification of a step's completion.
        "kanban_task_verify" => Some(STEP_VERIFICATION),
        // The kata prompt asks a coaching question — the canonical
        // user-question occurrence.
        "kanban_task_kata_prompt" => Some(USER_QUESTION_OCCURRENCE),
        // Recording kata state executes a step of the improvement-kata
        // routine on the task.
        "kanban_task_kata_record" => Some(STEP_EXECUTION),
        // Spawning/delegating executes a step of the parent procedure —
        // the sub-task's work is begun.
        "kanban_task_spawn" => Some(STEP_EXECUTION),
        // contract_propose_expect creates kanban tasks for contracts
        // missing expect annotations — a step of the contract
        // governance procedure (delegation from the curator).
        "contract_propose_expect" => Some(WAS_ASSOCIATED_WITH),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a kata-kanban tool's response
/// occupies.
///
/// Write responses are datasets (a stored or modified measurement);
/// list/read responses are compiled readings over many stored records
/// — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Reads: board/task/goal lists, comments, delegation results —
        // compiled readings over stored records.
        "kanban_board_list"
        | "kanban_goal_list"
        | "kanban_goal_get"
        | "kanban_task_list"
        | "kanban_task_comments_since"
        | "kanban_task_delegate_result" => Some(REPORT),
        // Writes: everything that stores or modifies a record.
        "kanban_board_create"
        | "kanban_board_delete"
        | "kanban_board_import"
        | "kanban_board_export"
        | "kanban_board_update"
        | "kanban_goal_create"
        | "kanban_goal_judge"
        | "kanban_task_create"
        | "kanban_task_move"
        | "kanban_task_reopen"
        | "kanban_task_assign"
        | "kanban_task_unassign"
        | "kanban_task_delete"
        | "kanban_task_comment"
        | "kanban_task_add_deliverable"
        | "kanban_task_update"
        | "kanban_task_verify"
        | "kanban_task_kata_prompt"
        | "kanban_task_kata_record"
        | "kanban_task_spawn"
        | "contract_propose_expect" => Some(DATASET),
        _ => None,
    }
}

// The shared dual-axis machinery (the both-axes combinator, the central
// annotate stamp, and the both-directions totality tests) comes from the
// framework crate's `dual_axis_annotate!` macro — one definition across
// the nine stamped servers instead of a copy per server.

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &["kanban_", "contract_"],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_bridge_ontology::pko::PROCEDURE;

    // The both-directions totality tests (coverage + the phantom-name
    // reverse scan) are emitted by the framework crate's
    // `dual_axis_annotate!` macro above — one definition across the nine
    // stamped servers, including its own generated TOOL_NAMES include.
    // This module keeps the kata-kanban-specific tests.

    /// The axes must discriminate: a state change and a verification and a
    /// coaching question are different process concepts, and a write and
    /// a read are different state concepts.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_pko("kanban_task_move"), Some(CHANGE_OF_STATUS));
        assert_eq!(tool_to_pko("kanban_goal_judge"), Some(STEP_VERIFICATION));
        assert_eq!(
            tool_to_pko("kanban_task_kata_prompt"),
            Some(USER_QUESTION_OCCURRENCE)
        );
        assert_eq!(tool_to_pko("kanban_task_list"), Some(STEP));

        assert_eq!(tool_to_dc("kanban_task_move"), Some(DATASET));
        assert_eq!(tool_to_dc("kanban_task_list"), Some(REPORT));
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table. Also pins the migration's consumer seam:
    /// the NESTED per-item tags (`BoardInfo.ontology`, the kanban
    /// widget's S4 sensor) survive as strings while the top-level key
    /// becomes the dual-axis object.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use crate::KanbanService;
        use crate::test_support::UnavailableWorktreeSpawn;
        use hkask_mcp_swarm::LocalAgentRegistry;
        use hkask_storage::HMemStore;
        use hkask_storage::database::sqlite::SqliteDriver;
        use hkask_types::WebID;
        use rmcp::handler::server::wrapper::Parameters;
        use std::sync::Arc;

        let driver = SqliteDriver::in_memory_driver();
        let service =
            KanbanService::new(HMemStore::from_driver(Arc::clone(&driver)).expect("hmem store"));
        let idempotency = Arc::new(
            crate::idempotency::IdempotencyStore::with_driver(driver).expect("idempotency schema"),
        );
        let server = crate::KanbanServer::new(
            WebID::new(),
            service,
            Arc::new(LocalAgentRegistry::new("/nonexistent")),
            Arc::new(UnavailableWorktreeSpawn),
            Arc::clone(&idempotency),
            idempotency,
        );

        // A write tool: the response carries both axes at the top level.
        let created = server
            .kanban_board_create(Parameters(crate::types::BoardCreateRequest {
                name: "dual-axis-pin".into(),
                columns: None,
                idempotency_key: None,
                done_requires_verification: None,
            }))
            .await
            .expect("board create succeeds");
        let value: serde_json::Value = serde_json::from_str(&created)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {created}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {created}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(STEP_EXECUTION),
            "the live kanban_board_create response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(DATASET),
            "the live kanban_board_create response carries the state axis: {content}"
        );
        // The migration seam: the removed flat-response `ontology` string
        // is gone — the top-level key is the dual-axis object, not a string.
        assert!(
            content["ontology"].is_object(),
            "the top-level ontology is the dual-axis object, never a bare string: {content}"
        );

        // A read tool: the response is a report, and the NESTED per-item
        // tags survive as strings (the widget's S4 sensor reads exactly
        // these).
        let listed = server
            .kanban_board_list(Parameters(crate::types::BoardListRequest {}))
            .await
            .expect("board list succeeds");
        let value: serde_json::Value = serde_json::from_str(&listed)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {listed}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {listed}"));
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live kanban_board_list response is a report: {content}"
        );
        assert_eq!(
            content["boards"][0]["ontology"],
            serde_json::json!(PROCEDURE),
            "the nested per-item BoardInfo.ontology string survives (the widget's S4 sensor): {content}"
        );
    }
}
