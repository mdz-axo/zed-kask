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
//! 37 tool returns.

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
        // ── Procedure specification and steps (agent/swarm creation) ──
        // Creating a swarm specifies the composition — a
        // procedure to be executed. Creating an agent specifies one
        // addressable member; cloning specifies a copy.
        "swarm_create_local_swarm" | "swarm_create_local_agent" | "swarm_clone_local_swarm" => {
            Some(PROCEDURE)
        }
        // Rostering: adding/removing a member composes one step of the
        // swarm's procedure; updating/reconfiguring revises a step.
        "swarm_add_agent_local"
        | "swarm_remove_agent_local"
        | "swarm_remove_local"
        | "swarm_delete_local_swarm"
        | "swarm_reconfigure_local_agent"
        | "swarm_update_local_swarm" => Some(STEP),
        // ── Execution (the swarm's coordinated work occurring) ────────────
        // Delegation, fan-out, pipelines, and workflow execution are the
        // steps actually occurring — real inference calls, real work.
        "swarm_delegate_in_thread_local"
        | "swarm_delegate_local"
        | "swarm_fanout_local"
        | "swarm_pipeline_local"
        | "swarm_execute_plan_local"
        | "swarm_run_workflow_local"
        | "swarm_a2a_send"
        | "swarm_a2a_broadcast" => Some(STEP_EXECUTION),
        // ── Verification (evaluating, checking, reading progress) ─────────
        // Evaluation harnesses and workflow checks verify what
        // execution produced.
        "swarm_eval_agent_local"
        | "swarm_eval_suite_local"
        | "swarm_evaluate_local"
        | "swarm_workflow_check_local" => Some(STEP_VERIFICATION),
        // ── Issue occurrences (assistant surfaces over unexpected states) ─
        // AI assist surfaces authoring help and issue-classified results.
        "swarm_ai_assist" => Some(ISSUE_OCCURRENCE),
        // ── Observations (reads over records of executions) ──────────────
        // Reading threads, task boards, history, lineage,
        // fleets, registries, knowledge, and recall observe the state
        // of executions already occurred.
        "swarm_thread_local"
        | "swarm_list_local_threads"
        | "swarm_task_board"
        | "swarm_list_local_swarms"
        | "swarm_get_local_swarm"
        | "swarm_get_local_agent"
        | "swarm_list_local_agents"
        | "swarm_generate_ontology_local"
        | "swarm_generate_prompt_local"
        | "swarm_search_knowledge_local"
        | "swarm_recall_local"
        | "swarm_observed_seams_local"
        | "swarm_fleet_digest_local"
        | "swarm_who_answers_local"
        | "swarm_a2a_card" => Some(PROCEDURE_EXECUTION),
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
        "swarm_create_local_swarm"
        | "swarm_create_local_agent"
        | "swarm_clone_local_swarm"
        | "swarm_add_agent_local"
        | "swarm_remove_agent_local"
        | "swarm_remove_local"
        | "swarm_delete_local_swarm"
        | "swarm_reconfigure_local_agent"
        | "swarm_update_local_swarm"
        | "swarm_delegate_in_thread_local"
        | "swarm_delegate_local"
        | "swarm_fanout_local"
        | "swarm_pipeline_local"
        | "swarm_execute_plan_local"
        | "swarm_run_workflow_local"
        | "swarm_a2a_send"
        | "swarm_a2a_broadcast"
        | "swarm_ai_assist"
        | "swarm_eval_agent_local"
        | "swarm_eval_suite_local"
        | "swarm_evaluate_local" => Some(DATASET),
        // Reads, checks, and generated content are compiled readings —
        // reports.
        _ => Some(REPORT),
    }
}

// The shared dual-axis machinery (the both-axes combinator, the central
// annotate stamp, and the both-directions totality tests) comes from the
// framework crate's `dual_axis_annotate!` macro — one definition across
// the nine stamped servers instead of a copy per server (the ratchet
// measured ~19 cross-server duplication clusters from the per-server
// copies before this hoist).

hkask_mcp_server::dual_axis_annotate!(
    tool_to_pko,
    tool_to_dc,
    PkoConcept,
    DcConcept,
    &["swarm_", "local_", "a2a_", "knowledge_"],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;

    /// P5.4: "Every MCP server uses BOTH axes" (operator ruling B.2
    /// 2026-10-18: every tool RESPONSE). Every registered swarm tool —
    /// derived from the live `execute_tool` surface in this server's
    /// three tool files — must carry a process AND a state identity.
    /// The list is derived by walking the tool files for
    /// `execute_tool(self, "...")` call sites at test time (the
    /// cross-seam derived-walk discipline: a name-pinned list breaks on
    /// every rename and silently misses every addition).
    #[test]
    fn dual_axis_covers_all_registered_tools() {
        let source_files = [
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
            37,
            "the live execute_tool surface must be the 37 registered tools (the \
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
        // The predicate fn's own prefix literals ("swarm_", ...) are
        // fragments, not names — a name is strictly longer than its
        // prefix (the same fragment rule as is_tool_name).
        const PREFIXES: [&str; 4] = ["swarm_", "local_", "a2a_", "knowledge_"];
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            for p in PREFIXES {
                if let Some(rest) = chunk.strip_prefix(p) {
                    if !rest.is_empty()
                        && rest
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                    {
                        mapped.push(chunk.to_string());
                    }
                }
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
