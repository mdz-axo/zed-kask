//! Dual-axis ontology dispatch for hkask-mcp-swarm (P5.4, operator ruling
//! B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the swarm
//! server's mapping onto those two axes:
//!
//! - **Process axis (PKO)** — the swarm surface IS a procedure: agent
//!   creation/hiring/rostering are procedure specifications and steps;
//!   delegation, fan-out, pipelines, and workflows are step executions
//!   (the swarm's coordinated work actually occurring); evaluation,
//!   publish-checks, task boards, and gate readings are step
//!   verifications; issue-classified ai-assist surfaces are issue
//!   occurrences; reading threads/run-status/history observes
//!   executions already occurred.
//! - **State axis (DC+BIBO)** — write responses are datasets (a stored
//!   or modified measurement); read/list/status responses are compiled
//!   readings over records — reports.
//!
//! The vocabulary lives in the shared `hkask-bridge-ontology` crate;
//! this module holds only the swarm-specific tool→concept table (the
//! architectural invariant — user directive 2026-08-05 — that no
//! ontology vocabulary is defined inside an MCP server). The stamp
//! lands centrally in `hkask-mcp-server`'s `ToolContext::
//! annotate_response` hook — one point every tool already routes
//! through (`execute_tool`), instead of a per-site wrap at each of the
//! 91 tool returns.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{
    ISSUE_OCCURRENCE, PROCEDURE, PROCEDURE_EXECUTION, STEP, STEP_EXECUTION, STEP_VERIFICATION,
};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a swarm tool's response occupies.
///
/// Every registered tool has an explicit arm — `dual_axis_covers_all_
/// registered_tools` (below) fails when a tool is added without one.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // ── Procedure specification and steps (agent/swarm/app creation) ──
        // Creating a swarm or an App specifies the composition — a
        // procedure to be executed. Creating an agent specifies one
        // addressable member; cloning and pushing specify copies.
        "swarm_create_swarm"
        | "swarm_create_local_swarm"
        | "swarm_create_agent"
        | "swarm_create_local_agent"
        | "swarm_create_app"
        | "swarm_create_app_direct"
        | "swarm_clone_to_local"
        | "swarm_clone_local_swarm"
        | "swarm_fork_agent"
        | "swarm_fork_workspace_to_app"
        | "swarm_push_to_cloud"
        | "swarm_push_local_swarm"
        | "swarm_pull_swarm_to_local" => Some(PROCEDURE),
        // Rostering: adding/removing a member composes one step of the
        // swarm's procedure; updating/reconfiguring revises a step.
        "swarm_add_agent_local"
        | "swarm_remove_agent_local"
        | "swarm_fire"
        | "swarm_hire"
        | "swarm_delete_agent"
        | "swarm_remove_local"
        | "swarm_delete_swarm"
        | "swarm_delete_local_swarm"
        | "swarm_archive_app"
        | "swarm_update_agent"
        | "swarm_update_app"
        | "swarm_reconfigure_local_agent"
        | "swarm_update_local_swarm" => Some(STEP),
        // ── Execution (the swarm's coordinated work occurring) ────────────
        // Delegation, fan-out, pipelines, and workflow execution are the
        // steps actually occurring — real inference calls, real work.
        "swarm_delegate"
        | "swarm_delegate_and_wait"
        | "swarm_delegate_in_thread_local"
        | "swarm_delegate_local"
        | "swarm_fanout"
        | "swarm_fanout_local"
        | "swarm_pipeline_local"
        | "swarm_execute_plan_local"
        | "swarm_execute_agent"
        | "swarm_run_workflow_local"
        | "swarm_a2a_send"
        | "swarm_a2a_broadcast" => Some(STEP_EXECUTION),
        // ── Verification (evaluating, checking, reading progress) ─────────
        // Evaluation harnesses, publish checks, workflow checks, gate
        // readings, and the selection ranking verify what execution
        // produced. Consent records verify an authorized spend.
        "swarm_eval_agent_local"
        | "swarm_eval_suite_local"
        | "swarm_evaluate_local"
        | "swarm_publish_checks"
        | "swarm_publish_agent"
        | "swarm_publish_app"
        | "swarm_workflow_check_local"
        | "swarm_gate_readings"
        | "swarm_select_agent_local"
        | "swarm_request_consent"
        | "swarm_authorize_session" => Some(STEP_VERIFICATION),
        // ── Issue occurrences (assistant surfaces over unexpected states) ─
        // AI assist surfaces authoring help and issue-classified results.
        "swarm_ai_assist" => Some(ISSUE_OCCURRENCE),
        // ── Observations (reads over records of executions) ──────────────
        // Reading threads, run status, task boards, history, lineage,
        // fleets, registries, knowledge, and recall observe the state
        // of executions already occurred.
        "swarm_run_status"
        | "swarm_thread_local"
        | "swarm_list_local_threads"
        | "swarm_task_board"
        | "swarm_list_local_swarms"
        | "swarm_get_local_swarm"
        | "swarm_get_swarm"
        | "swarm_get_local_agent"
        | "swarm_get_agent"
        | "swarm_list_local_agents"
        | "swarm_list_agents"
        | "swarm_list_apps"
        | "swarm_get_app"
        | "swarm_get_app_schema"
        | "swarm_list_app_workspaces"
        | "swarm_spawn_app_workspace"
        | "swarm_hire_cost"
        | "swarm_generate_ontology"
        | "swarm_generate_ontology_local"
        | "swarm_generate_prompt"
        | "swarm_generate_prompt_local"
        | "swarm_ontology_templates"
        | "swarm_search_knowledge"
        | "swarm_search_knowledge_local"
        | "swarm_recall_local"
        | "swarm_observed_seams_local"
        | "swarm_fleet_digest_local"
        | "swarm_who_answers_local"
        | "swarm_a2a_card"
        | "swarm_workspace_list_actions"
        | "swarm_workspace_list_annotations"
        | "swarm_workspace_list_files"
        | "swarm_workspace_pending_actions"
        | "swarm_workspace_read_file"
        | "swarm_workspace_accept_action"
        | "swarm_workspace_reject_action"
        | "swarm_workspace_fork_state"
        | "swarm_workspace_annotate"
        | "swarm_workspace_mutate_document"
        | "swarm_workspace_write_file" => Some(PROCEDURE_EXECUTION),
        // The Xaman Ek curator consultation observes curated composition
        // knowledge — an execution read with generative overlap; the
        // honest coarse-grained arm is a procedure-execution observation.
        "swarm_xaman" => Some(PROCEDURE_EXECUTION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a swarm tool's response occupies.
///
/// Write responses are datasets (a stored or modified measurement);
/// read/list/status responses are compiled readings — reports. Assistant
/// surfaces produce issue reports — datasets of the recorded report.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        "swarm_create_swarm"
        | "swarm_create_local_swarm"
        | "swarm_create_agent"
        | "swarm_create_local_agent"
        | "swarm_create_app"
        | "swarm_create_app_direct"
        | "swarm_clone_to_local"
        | "swarm_clone_local_swarm"
        | "swarm_fork_agent"
        | "swarm_fork_workspace_to_app"
        | "swarm_push_to_cloud"
        | "swarm_push_local_swarm"
        | "swarm_pull_swarm_to_local"
        | "swarm_add_agent_local"
        | "swarm_remove_agent_local"
        | "swarm_fire"
        | "swarm_hire"
        | "swarm_delete_agent"
        | "swarm_remove_local"
        | "swarm_delete_swarm"
        | "swarm_delete_local_swarm"
        | "swarm_archive_app"
        | "swarm_update_agent"
        | "swarm_update_app"
        | "swarm_reconfigure_local_agent"
        | "swarm_update_local_swarm"
        | "swarm_delegate"
        | "swarm_delegate_and_wait"
        | "swarm_delegate_in_thread_local"
        | "swarm_delegate_local"
        | "swarm_fanout"
        | "swarm_fanout_local"
        | "swarm_pipeline_local"
        | "swarm_execute_plan_local"
        | "swarm_execute_agent"
        | "swarm_run_workflow_local"
        | "swarm_a2a_send"
        | "swarm_a2a_broadcast"
        | "swarm_ai_assist"
        | "swarm_eval_agent_local"
        | "swarm_eval_suite_local"
        | "swarm_evaluate_local"
        | "swarm_publish_agent"
        | "swarm_publish_app"
        | "swarm_workspace_accept_action"
        | "swarm_workspace_reject_action"
        | "swarm_workspace_fork_state"
        | "swarm_workspace_annotate"
        | "swarm_workspace_mutate_document"
        | "swarm_workspace_write_file"
        | "swarm_request_consent"
        | "swarm_authorize_session"
        | "swarm_spawn_app_workspace" => Some(DATASET),
        // Reads, checks, and generated content are compiled readings —
        // reports.
        _ => Some(REPORT),
    }
}

/// Both axes for a registered tool, or `None` when the name is not one.
///
/// P5.4's dual-axis core: a caller never gets one axis without the other.
pub(crate) fn tool_to_dual_axis(tool_name: &str) -> Option<(PkoConcept, DcConcept)> {
    match (tool_to_pko(tool_name), tool_to_dc(tool_name)) {
        (Some(process), Some(state)) => Some((process, state)),
        _ => None,
    }
}

/// The server-level response stamp consumed by the framework's central
/// `ToolContext::annotate_response` hook (`execute_tool`) — every swarm
/// tool response carries both axes without a per-site wrap at each of
/// the 91 tool returns.
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

    /// P5.4: "Every MCP server uses BOTH axes" (operator ruling B.2
    /// 2026-10-18: every tool RESPONSE). Every registered swarm tool —
    /// derived from the live `execute_tool` surface in this server's
    /// four tool files — must carry a process AND a state identity.
    /// The list is derived by walking the four tool files for
    /// `execute_tool(self, "...")` call sites at test time (the
    /// cross-seam derived-walk discipline: a name-pinned list breaks on
    /// every rename and silently misses every addition).
    #[test]
    fn dual_axis_covers_all_registered_tools() {
        let source_files = [
            "src/cloud_swarm_tools.rs",
            "src/local_tools.rs",
            "src/a2a_tools.rs",
            "src/knowledge_tools.rs",
        ];
        let mut live: Vec<String> = Vec::new();
        for file in source_files {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
            let src = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let flattened: String = src.split_whitespace().collect::<Vec<_>>().join(" ");
            let marker = "execute_tool(";
            let mut rest = flattened.as_str();
            while let Some(found) = rest.find(marker) {
                rest = &rest[found + marker.len()..];
                // The arguments are always `self, "<tool_name>"` —
                // single- or multi-line at the source, single-spaced when
                // flattened. The tool name is the first quoted string in
                // the remainder.
                let Some(first_quote) = rest.find('"') else {
                    break;
                };
                let after = &rest[first_quote + 1..];
                let Some(end) = after.find('"') else { break };
                let name = after[..end].to_string();
                if !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                {
                    live.push(name);
                }
                rest = &rest[end..];
            }
        }
        live.sort();
        live.dedup();
        assert_eq!(
            live.len(),
            91,
            "the live execute_tool surface must be the 91 registered tools (the \
             tool_surface pin count); found {}: {live:?}",
            live.len()
        );
        for name in &live {
            assert!(
                tool_to_dual_axis(name).is_some(),
                "no dual-axis mapping for registered tool '{name}'; add an explicit arm \
                 to tool_to_pko and tool_to_dc in src/ontology.rs"
            );
        }
        // The reverse direction: every name the mapping claims must be a
        // registered tool. Coverage alone let a phantom name sit in the
        // mapping undetected (live 2026-10-18: `swarm_delete_local`, a typo
        // for `swarm_delete_local_swarm`, matched no `execute_tool` call site
        // and still passed the coverage assertion). Scanned from the same
        // source the axes are defined in, so no new registry is introduced.
        let ontology_src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ontology.rs"),
        )
        .expect("src/ontology.rs must be readable");
        let mut mapped: Vec<String> = Vec::new();
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            if !chunk.is_empty()
                && chunk.starts_with("swarm_")
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
            {
                mapped.push(chunk.to_string());
            }
        }
        mapped.sort();
        mapped.dedup();
        for name in &mapped {
            assert!(
                live.contains(name),
                "mapping names '{name}', which no `execute_tool` call site registers — \
                 a stale or misspelled arm in src/ontology.rs"
            );
        }
    }

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"count": 3, "results": []});
        let stamped = annotate("swarm_list_local_swarms", body.clone());
        assert_eq!(stamped["count"], serde_json::json!(3), "{stamped:?}");
        assert_eq!(
            stamped["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION)
        );
        assert_eq!(stamped["ontology"]["dc_type"], serde_json::json!(REPORT));
        // An unmapped name returns the body unchanged — never an
        // invented axis.
        let untouched = annotate("not_a_registered_tool", body);
        assert!(untouched.get("ontology").is_none(), "{untouched:?}");
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        let (_dir, server, _recorder) =
            crate::test_support::recording_thread_server("test-passphrase");
        let response = server
            .swarm_list_local_swarms(rmcp::handler::server::wrapper::Parameters(
                crate::request_types::ListLocalSwarmsRequest {},
            ))
            .await
            .expect("list local swarms succeeds");
        let value: serde_json::Value = serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {response}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {response}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION),
            "the live swarm_list_local_swarms response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live swarm_list_local_swarms response carries the state axis: {content}"
        );
    }
}
