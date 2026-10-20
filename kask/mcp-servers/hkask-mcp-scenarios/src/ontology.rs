//! Dual-axis ontology dispatch for hkask-mcp-scenarios (P5.4, operator
//! ruling B.2 2026-10-18: every tool RESPONSE carries both axes).
//!
//! Every artifact in hKask has both a state identity (what it is) and a
//! process identity (how it came to be). This module is the scenarios
//! server's mapping onto those two axes, wired as the central
//! `ToolContext::annotate_response` hook via the `mcp_server!` macro's
//! annotate override — one point stamps every tool response at the
//! `execute_tool` boundary.
//!
//! SUBTRACTIVE MIGRATION (the kata-kanban option-(b) precedent, operator
//! ruling 2026-10-18): this server previously emitted a process-axis-only
//! `ontology` STRING at 10 per-site top-level emission points (via the
//! `ontology_anchor` helper, with a `dc_bibo::DATASET` catch-all). The
//! canonical hook writes its dual-axis OBJECT at the same key, so the
//! string is REPLACED, not added beside: the 10 per-site emissions and
//! the `ontology_anchor` helper are deleted in the same change. The
//! prior values are preserved semantically — every tool the old mapping
//! classified as a procedure/dataset now carries that concept as one of
//! its two axes.
//!
//! - **Process axis (PKO)** — scenario work is a forecasting procedure:
//!     framing/brainstorming/building specify the procedure and its
//!     steps; quantifying/calibrating/synthesizing/scoring execute its
//!     steps; propagation and posterior recomputation execute Bayesian
//!     update steps; status/calibration/list read prior executions;
//!     assess verifies project effectiveness; forecast persistence and
//!     recording execute the scoring loop's steps.
//! - **State axis (DC+BIBO)** — write responses are datasets (a stored
//!   or modified measurement — a persisted forecast, a recorded
//!   resolution, a recomputed posterior); read responses are compiled
//!   readings over stored records — reports.
//!
//! The vocabulary itself lives in the shared `hkask-bridge-ontology`
//! crate; this module holds only the scenarios-specific tool→concept
//! table. That is the server's business, not the ontology's — the
//! architectural invariant (user directive 2026-08-05) that no ontology
//! vocabulary is defined inside an MCP server.

pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
pub use hkask_bridge_ontology::pko::{
    PROCEDURE, PROCEDURE_EXECUTION, STEP_EXECUTION, STEP_VERIFICATION,
};

use hkask_bridge_ontology::dc_bibo::DcConcept;
use hkask_bridge_ontology::pko::PkoConcept;

/// The process-axis (PKO) concept a scenarios tool's response occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Procedure specification: the framing conversation, the framing
        // document, and brainstorming specify the scenario procedure
        // (the old `ontology_anchor` classified these as pko:Procedure
        // — preserved).
        "scenario_frame" | "scenario_frame_document" | "scenario_brainstorm" => Some(PROCEDURE),
        // Tree construction and market bridging build the procedure's
        // steps; quantify resolves the tree (a step execution);
        // build scaffolds the event tree (a step).
        "scenario_build" | "scenario_from_markets_set" => Some(PROCEDURE),
        "scenario_quantify" | "scenario_declare" => Some(STEP_EXECUTION),
        // Calibration, cross-validation, and synthesis execute the
        // Tetlock quantification stages.
        "scenario_calibrate" | "scenario_cross_validate" | "scenario_synthesize" => {
            Some(STEP_EXECUTION)
        }
        // Bayesian updates: propagation and posterior recomputation
        // execute revision steps.
        "scenario_propagate" | "scenario_recompute_posteriors" => Some(STEP_EXECUTION),
        // The risk measure computes over scenario branches — a
        // computation step.
        "scenario_risk_measure" => Some(STEP_EXECUTION),
        // Scoring closes a forecast against known outcomes; assessment
        // verifies project effectiveness — verification steps.
        "scenario_score" | "scenario_assess" => Some(STEP_VERIFICATION),
        // Triage classifies a forecasting question — a verification of
        // question admissibility (Tetlock Commandment 1).
        "scenario_triage" => Some(STEP_VERIFICATION),
        // Reads: status, the calibration curve, the forecast list, and
        // calibration lookup observe prior executions.
        "scenario_status" | "scenario_calibration" | "forecast_list" => Some(PROCEDURE_EXECUTION),
        // Forecast loop writes: persisting a target and recording a
        // resolution execute the Brier-scoring loop's steps.
        "forecast_persist" | "forecast_record" => Some(STEP_EXECUTION),
        // Revising a scenario event's prior — an update step.
        "scenario_update" => Some(STEP_EXECUTION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a scenarios tool's response
/// occupies.
///
/// Write responses are datasets (a stored or modified measurement — a
/// persisted forecast, a recorded resolution, a revised prior, a
/// recomputed posterior); read responses are compiled readings over
/// stored records — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Reads: status, calibration curve, forecast list — reports.
        "scenario_status" | "scenario_calibration" | "forecast_list" => Some(REPORT),
        // Writes and computations: framing, brainstorming, building,
        // bridging, quantification, calibration, cross-validation,
        // synthesis, propagation, posteriors, risk measure, scoring,
        // assessment, triage, forecast persistence/recording, revision —
        // datasets.
        "scenario_frame"
        | "scenario_frame_document"
        | "scenario_brainstorm"
        | "scenario_build"
        | "scenario_from_markets_set"
        | "scenario_declare"
        | "scenario_quantify"
        | "scenario_calibrate"
        | "scenario_cross_validate"
        | "scenario_synthesize"
        | "scenario_propagate"
        | "scenario_recompute_posteriors"
        | "scenario_risk_measure"
        | "scenario_score"
        | "scenario_assess"
        | "scenario_triage"
        | "forecast_persist"
        | "forecast_record"
        | "scenario_update" => Some(DATASET),
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
    &["scenario_", "forecast_"],
    &[],
);

#[cfg(test)]
mod tests {
    use super::*;

    // The both-directions totality tests (coverage + the phantom-name
    // reverse scan) are emitted by the framework crate's
    // `dual_axis_annotate!` macro above (its own `dual_axis_tests`
    // module) — one definition across the nine stamped servers, including
    // its own generated TOOL_NAMES include. This module keeps the
    // scenarios-specific tests.

    /// The axes must discriminate: a procedure specification and a step
    /// execution are different process concepts, and a read and a write
    /// are different state concepts.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_pko("scenario_frame"), Some(PROCEDURE));
        assert_eq!(tool_to_pko("scenario_quantify"), Some(STEP_EXECUTION));
        assert_eq!(tool_to_pko("scenario_score"), Some(STEP_VERIFICATION));

        assert_eq!(tool_to_dc("forecast_list"), Some(REPORT));
        assert_eq!(tool_to_dc("forecast_persist"), Some(DATASET));
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table. `scenario_status` needs no credential: it
    /// reads the in-memory stores only.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use rmcp::handler::server::wrapper::Parameters;
        use std::sync::{Arc, Mutex};

        let server = crate::ScenariosServer::new(
            hkask_types::WebID::new(),
            Arc::new(Mutex::new(crate::superforecast::ForecastStore::new(None))),
            Arc::new(Mutex::new(crate::superforecast::JournalStore::new(None))),
            Mutex::new(None),
            Mutex::new(crate::superforecast::ProjectStore::new(None)),
        );
        let response = server
            .scenario_status(Parameters(crate::requests::StatusRequest {}))
            .await
            .expect("status succeeds on a fresh server");
        let value: serde_json::Value = serde_json::from_str(&response)
            .unwrap_or_else(|error| panic!("response is JSON: {error}: {response}"));
        let content = value
            .get("content")
            .cloned()
            .unwrap_or_else(|| panic!("envelope carries content: {response}"));
        assert_eq!(
            content["ontology"]["pko_procedure"],
            serde_json::json!(PROCEDURE_EXECUTION),
            "the live scenario_status response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live scenario_status response carries the state axis: {content}"
        );
    }
}
