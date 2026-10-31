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
        | "curator_memory_extract"
        | "trust_ledger_record_instruction"
        | "trust_ledger_record_outcome"
        | "trust_ledger_state"
        | "trust_ledger_steer" => Some(STEP_EXECUTION),
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
        | "curator_memory_extract"
        | "trust_ledger_record_instruction"
        | "trust_ledger_record_outcome" => Some(DATASET),
        "curator_memory_recall"
        | "curator_semantic_search"
        | "curator_federated_search"
        | "curator_consult"
        | "curator_algedonic_log"
        | "reg_query"
        | "curator_ping"
        | "trust_ledger_state"
        | "trust_ledger_steer" => Some(REPORT),
        "curator_report_skill_use_issue" => Some(DATASET),
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
    &["curator_", "memory_", "reg_"],
    &[],
);
