//! Dual-axis ontology dispatch for hkask-mcp-curator (P5.4, operator ruling
//! B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the curator
//! server's mapping onto those two axes:
//!
//! - **Process axis (PKO)** — the curator's memory is a curation
//!   procedure: recall/search/consult observe the executions of the
//!   ingestion and turn-storage procedures that produced the stored
//!   knowledge; insert/update/resolve/prune/dedup/backfill/extract are
//!   curation steps executing; report-skill-use-issue records an issue
//!   occurrence from a failed or unexpected tool result; ping verifies
//!   the server's readiness.
//! - **State axis (DC+BIBO)** — write responses are datasets (a stored or
//!   modified measurement); read responses are compiled readings over
//!   many stored records — reports.
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology` crate;
//! this module holds only the curator-specific tool→concept table. That
//! is the server's business, not the ontology's — the architectural
//! invariant (user directive 2026-08-05) that no ontology vocabulary is
//! defined inside an MCP server. The constants are re-exported so call
//! sites and tests reference the single fixture-guarded source.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{
    ISSUE_OCCURRENCE, PROCEDURE_EXECUTION, STEP_EXECUTION, STEP_VERIFICATION,
};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a curator tool's response occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Recalling an entity, searching by meaning, federated retrieval,
        // and consulting all observe the executions of the procedures
        // (turn storage, ingestion, distillation) that produced the
        // stored knowledge — compiled readings over many records.
        "curator_memory_recall"
        | "curator_semantic_search"
        | "curator_federated_search"
        | "curator_consult" => Some(PROCEDURE_EXECUTION),
        // Recording a skill-use issue is the canonical issue occurrence:
        // an error or unexpected output an agent encountered during
        // execution, filed for review.
        "curator_report_skill_use_issue" => Some(ISSUE_OCCURRENCE),
        // Reading the algedonic log or the regulation ledger observes
        // logged regulatory events — executions already occurred.
        "curator_algedonic_log" | "reg_query" => Some(PROCEDURE_EXECUTION),
        // Memory writes are curation steps executing: storing,
        // confidence-combining, contradiction resolution, forgetting,
        // deduplication, embedding backfill, and candidate extraction
        // are each one step of the curation procedure.
        "memory_insert"
        | "memory_update"
        | "memory_resolve_contradiction"
        | "curator_memory_prune"
        | "curator_memory_dedup"
        | "curator_memory_backfill_embeddings"
        | "curator_memory_extract" => Some(STEP_EXECUTION),
        // The liveness check verifies the server's readiness — the
        // stores are attached and responsive.
        "curator_ping" => Some(STEP_VERIFICATION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a curator tool's response occupies.
///
/// Write responses are datasets (a stored or modified measurement);
/// read responses are compiled readings over many records — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        "memory_insert"
        | "memory_update"
        | "memory_resolve_contradiction"
        | "curator_memory_prune"
        | "curator_memory_dedup"
        | "curator_memory_backfill_embeddings"
        | "curator_memory_extract" => Some(DATASET),
        "curator_memory_recall"
        | "curator_semantic_search"
        | "curator_federated_search"
        | "curator_consult"
        | "curator_algedonic_log"
        | "reg_query"
        | "curator_ping" => Some(REPORT),
        "curator_report_skill_use_issue" => Some(DATASET),
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
/// This is the production consumer of the mapping: every curator tool
/// routes its JSON body through here, so each response carries the
/// process (PKO) and state (DC+BIBO) concept its artifact occupies. The
/// annotation is additive — the existing body is untouched, so no caller
/// contract changes.
///
/// A name with no mapping returns the body unchanged rather than inventing
/// an axis: `dual_axis_covers_all_registered_tools` is what guarantees the
/// registered surface is total, and a silent default would defeat it.
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

    /// P5.4: "Every MCP server uses BOTH axes" (operator ruling B.2
    /// 2026-10-18: every tool RESPONSE). Every registered curator tool
    /// must carry a process AND a state identity — a tool added without
    /// an explicit arm fails here rather than silently losing its axes.
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
    }

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"count": 3, "results": []});
        let stamped = annotate("curator_semantic_search", body.clone());
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
}
