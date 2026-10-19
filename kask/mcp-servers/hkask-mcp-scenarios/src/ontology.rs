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
        "scenario_quantify" => Some(STEP_EXECUTION),
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
/// annotate override, so every scenarios tool response carries the
/// process (PKO) and state (DC+BIBO) concept its artifact occupies
/// without a per-site wrap at each of the 21 tool returns. The
/// annotation is additive at the call surface: the per-site
/// top-level `ontology` strings this replaces emitted the same key, so
/// the hook's object takes the key they vacated.
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
mod dual_axis_tests {
    use super::*;

    // The generated canonical tool-name list — included from OUT_DIR, so
    // the totality test derives its expectations from the live tool
    // surface (build.rs scan), never a hardcoded list.
    include!(concat!(env!("OUT_DIR"), "/tool_names.gen.rs"));

    /// P5.4: every registered tool must carry a process AND a state
    /// identity — a tool added without an explicit arm fails here rather
    /// than silently losing its axes.
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
        // The stamp's own JSON keys are not tool names — excluded by
        // name, not by pattern (a bare contains-'_' filter would catch
        // them). All scenarios tools carry the scenario_/forecast_
        // prefixes.
        const STAMP_KEYS: [&str; 3] = ["ontology", "pko_procedure", "dc_type"];
        for chunk in ontology_src.split('"').skip(1).step_by(2) {
            if !chunk.is_empty()
                && (chunk.starts_with("scenario_") || chunk.starts_with("forecast_"))
                && chunk
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
                && !STAMP_KEYS.contains(&chunk)
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

    /// The mapping is defined only over the registered surface: an
    /// unknown name returns None on both axes rather than a default
    /// concept.
    #[test]
    fn unknown_tool_has_no_axes() {
        assert!(tool_to_pko("not_a_tool").is_none());
        assert!(tool_to_dc("not_a_tool").is_none());
        assert!(tool_to_dual_axis("not_a_tool").is_none());
    }

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

    /// The stamp is additive: the body's own fields survive, and both
    /// axis concepts appear under `ontology`.
    #[test]
    fn annotate_stamps_both_axes_without_disturbing_the_body() {
        let body = serde_json::json!({"event_id": "e1", "probability": 0.62});
        let stamped = annotate("scenario_quantify", body.clone());
        assert_eq!(stamped["event_id"], serde_json::json!("e1"), "{stamped:?}");
        assert_eq!(
            stamped["ontology"]["pko_procedure"],
            serde_json::json!(STEP_EXECUTION)
        );
        assert_eq!(stamped["ontology"]["dc_type"], serde_json::json!(DATASET));
        // An unmapped name returns the body unchanged — never an
        // invented axis.
        let untouched = annotate("not_a_registered_tool", body.clone());
        assert_eq!(untouched, body);
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
            Mutex::new(std::collections::HashSet::new()),
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
