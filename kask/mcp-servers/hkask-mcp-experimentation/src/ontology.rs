//! Dual-axis ontology dispatch for hkask-mcp-experimentation (P5.4).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the experimentation
//! server's mapping onto those two axes:
//!
//! - **Process axis (PKO)** — the experiment registry IS a procedure: a
//!   declaration, variant registrations, a sealed contract, fitness records,
//!   a final evaluation, and a selection verdict. Those are
//!   `pko:Procedure`, `pplan:Step`, `pko:StepExecution` and
//!   `pko:StepVerification` occurrences, not metaphors.
//! - **State axis (DC+BIBO)** — the records the tools return are datasets
//!   (measurements) and reports (the lineage/population readings).
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology` crate;
//! this module holds only the experimentation-specific tool→concept table.
//! That is the server's business, not the ontology's — the architectural
//! invariant (user directive 2026-08-05) that no ontology vocabulary is
//! defined inside an MCP server. The constants are re-exported so call sites
//! and tests reference the single fixture-guarded source.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{
    PROCEDURE, STEP, STEP_EXECUTION, STEP_VERIFICATION, WAS_GENERATED_BY,
};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a registry tool's artifact occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools`.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Declaring the experiment specifies the procedure to be executed.
        "experiment_propose" => Some(PROCEDURE),
        // A variant is one step of that procedure: a genotype to measure.
        "variant_register" => Some(STEP),
        // Sealing freezes the eval contract — a verification of the
        // baseline's headroom before any challenger is measured (P2).
        "experiment_seal" => Some(STEP_VERIFICATION),
        // Recording fitness is the step actually occurring: recorded report
        // references the caller asserts cover real rollouts (§P8.3).
        "fitness_record" => Some(STEP_EXECUTION),
        // The sealed final evaluation re-scores the leading variant on the
        // disjoint selection set (P3) — a verification, not a measurement.
        "final_evaluation_record" => Some(STEP_VERIFICATION),
        // The selection verdict verifies the procedure's outcome and is the
        // fossil that resolves the experiment (§P8.7-Q4).
        "selection_record" => Some(STEP_VERIFICATION),
        // Reading lineage traces what generated each variant.
        "lineage_read" => Some(WAS_GENERATED_BY),
        // Querying the population observes procedures already executed.
        "population_query" => Some(PROCEDURE),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a registry tool's artifact occupies.
///
/// Measurements and registry writes are datasets; the two read tools return
/// compiled readings over many records, which are reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        "experiment_propose"
        | "variant_register"
        | "experiment_seal"
        | "fitness_record"
        | "final_evaluation_record"
        | "selection_record" => Some(DATASET),
        "lineage_read" | "population_query" => Some(REPORT),
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
    &[
        "experiment_",
        "variant_",
        "fitness_",
        "selection_",
        "lineage_",
        "population_"
    ],
    &["final_evaluation_record"],
);

#[cfg(test)]
mod tests {
    use super::*;

    // The both-directions totality tests are emitted by the framework
    // crate's `dual_axis_annotate!` macro above. This module keeps the
    // experimentation-specific tests.

    /// The axes must discriminate: a registry write and a compiled reading
    /// are different state concepts, and the lifecycle phases are different
    /// process concepts. A table that collapsed to one value per axis would
    /// satisfy coverage while carrying no information.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_dc("fitness_record"), Some(DATASET));
        assert_eq!(tool_to_dc("population_query"), Some(REPORT));

        assert_eq!(tool_to_pko("experiment_propose"), Some(PROCEDURE));
        assert_eq!(tool_to_pko("variant_register"), Some(STEP));
        assert_eq!(tool_to_pko("fitness_record"), Some(STEP_EXECUTION));
        assert_eq!(tool_to_pko("selection_record"), Some(STEP_VERIFICATION));
        assert_eq!(tool_to_pko("lineage_read"), Some(WAS_GENERATED_BY));
    }

    /// The constants this server emits are the shared crate's, never
    /// server-local literals (the no-vocabulary-in-a-server invariant).
    #[test]
    fn vocabulary_comes_from_the_shared_bridge_crate() {
        assert_eq!(PROCEDURE, hkask_bridge_ontology::pko::PROCEDURE);
        assert_eq!(STEP, hkask_bridge_ontology::pko::STEP);
        assert_eq!(
            STEP_VERIFICATION,
            hkask_bridge_ontology::pko::STEP_VERIFICATION
        );
        assert_eq!(DATASET, hkask_bridge_ontology::dc_bibo::DATASET);
        assert_eq!(REPORT, hkask_bridge_ontology::dc_bibo::REPORT);
    }

    /// The annotation is additive: it adds `ontology` and preserves every
    /// field the tool already returned.
    #[test]
    fn annotate_adds_both_axes_and_preserves_the_body() {
        let body = serde_json::json!({ "status": "proposed", "experiment": { "id": "e1" } });
        let annotated = annotate("experiment_propose", body);

        assert_eq!(annotated["status"], "proposed");
        assert_eq!(annotated["experiment"]["id"], "e1");
        assert_eq!(annotated["ontology"]["pko_procedure"], PROCEDURE);
        assert_eq!(annotated["ontology"]["dc_type"], DATASET);
    }

    /// An unmapped name is returned untouched — no invented axis.
    #[test]
    fn annotate_leaves_an_unmapped_name_untouched() {
        let body = serde_json::json!({ "status": "ok" });
        let annotated = annotate("not_a_tool", body.clone());
        assert_eq!(annotated, body);
        assert!(annotated.get("ontology").is_none());
    }

    /// A non-object body (the tool contract allows any JSON) is returned
    /// unchanged rather than panicking or being wrapped.
    #[test]
    fn annotate_leaves_a_non_object_body_untouched() {
        let body = serde_json::json!([1, 2, 3]);
        assert_eq!(annotate("population_query", body.clone()), body);
    }
}
