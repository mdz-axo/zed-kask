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
        // The goal loop: create/judge/score/acknowledge are the
        // verification stages of a goal's lifecycle — judging verifies
        // criteria against outcomes, scoring records the resolution,
        // acknowledging closes the curator-memory retention.
        "kanban_goal_create"
        | "kanban_goal_judge"
        | "kanban_goal_score"
        | "kanban_goal_list"
        | "kanban_goal_memory_acknowledge" => Some(STEP_VERIFICATION),
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
        | "kanban_goal_score"
        | "kanban_goal_memory_acknowledge"
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
        | "kanban_task_spawn"
        | "contract_propose_expect" => Some(DATASET),
        _ => None,
    }
}

/// Both axes for a registered tool, or `None` when the name is not one.
///
/// P5.4's dual-axis core: a caller never gets one axis without the other,
/// so a partial mapping cannot silently satisfy the contract.
pub(crate) fn tool_to_dual_axis(tool_name: &str) -> Option<(PkoConcept, DcConcept)> {
    match (tool_to_pko(tool_name), tool_to_dc(tool_name)) {
        (Some(process), Some(state)) => Some((process, state)),
        _ => None,
    }
}

/// Stamp the dual-axis identity onto a tool response.
///
/// This is the production consumer of the mapping: wired as the central
/// `ToolContext::annotate_response` hook via the `mcp_server!` macro's
/// annotate override, so every kata-kanban tool response carries the
/// process (PKO) and state (DC+BIBO) concept its artifact occupies
/// without a per-site wrap at each of the 27 tool returns. The
/// annotation is additive — the existing body is untouched, so no
/// caller contract changes beyond the removed flat-response `ontology`
/// string (this module replaces it with the dual-axis object at the
/// same key).
///
/// A name with no mapping returns the body unchanged rather than
/// inventing an axis: `dual_axis_covers_all_registered_tools` is what
/// guarantees the registered surface is total, and a silent default
/// would defeat it.
pub(crate) fn annotate(tool_name: &str, mut body: serde_json::Value) -> serde_json::Value {
    let Some((process, state)) = tool_to_dual_axis(tool_name) else {
        return body;
    };
    if let Some(object) = body.as_object_mut() {
        object.insert(
            "ontology".to_string(),
            serde_json::json!({
                "pko_procedure": process,
                "dc_type": state,
            }),
        );
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    // The generated canonical tool-name list — included from OUT_DIR, so
    // the totality test derives its expectations from the live tool
    // surface (build.rs scan), never a hardcoded list.
    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

    /// P5.4: "Every MCP server uses BOTH axes." Every registered tool must
    /// carry a process AND a state identity — a tool added without an
    /// explicit arm fails here rather than silently losing its axes.
    #[test]
    fn dual_axis_covers_all_registered_tools() {
        for name in TOOL_NAMES {
            let mapped = tool_to_dual_axis(name);
            assert!(
                mapped.is_some(),
                "no dual-axis mapping for registered tool '{name}'; add an explicit arm \
                 to tool_to_pko and tool_to_dc in src/ontology.rs"
            );
        }
        // The reverse direction: every name the mapping claims must be a
        // registered tool. Coverage alone let a phantom name sit in the
        // swarm mapping undetected (live 2026-10-18: `swarm_delete_local`,
        // a typo, matched no call site and still passed coverage).
        let ontology_src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ontology.rs"),
        )
        .expect("src/ontology.rs must be readable");
        let mut mapped: Vec<&str> = Vec::new();
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            if (chunk.starts_with("kanban_") || chunk.starts_with("contract_"))
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            {
                mapped.push(chunk);
            }
        }
        mapped.sort_unstable();
        mapped.dedup();
        assert!(
            !mapped.is_empty(),
            "the reverse scan found no mapping names — the instrument is broken"
        );
        for name in &mapped {
            assert!(
                TOOL_NAMES.contains(name),
                "mapping names '{name}', which is not a registered tool — a stale or \
                 misspelled arm in src/ontology.rs"
            );
        }
    }

    /// The mapping is defined only over the registered surface: an unknown
    /// name returns None on both axes rather than a default concept.
    #[test]
    fn unknown_tool_has_no_axes() {
        assert!(tool_to_pko("not_a_tool").is_none());
        assert!(tool_to_dc("not_a_tool").is_none());
        assert!(tool_to_dual_axis("not_a_tool").is_none());
    }

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

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"task_id": "t1", "new_status": "in_progress"});
        let stamped = annotate("kanban_task_move", body.clone());
        assert_eq!(stamped["task_id"], serde_json::json!("t1"), "{stamped:?}");
        assert_eq!(
            stamped["ontology"]["pko_procedure"],
            serde_json::json!(CHANGE_OF_STATUS)
        );
        assert_eq!(stamped["ontology"]["dc_type"], serde_json::json!(DATASET));
        // An unmapped name returns the body unchanged — never an
        // invented axis.
        let untouched = annotate("not_a_registered_tool", body.clone());
        assert_eq!(untouched, body);
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
            serde_json::json!("pko:Procedure"),
            "the nested per-item BoardInfo.ontology string survives (the widget's S4 sensor): {content}"
        );
    }
}
