//! Dual-axis ontology mapping for prediction-market records.
//!
//! Q-O1 resolution (2026-08-05): `hkask-bridge-ontology` is the canonical
//! vocabulary crate — the `dcterms:*` / `bibo:*` / `pko:*` constants defined
//! there are reused here, not re-declared. Q-O2 resolution: no `hkask:`
//! forecasting namespace exists in the workspace; calibration vocabulary is
//! domain-supplement tier and lives in this module until a second consumer
//! materializes (ADR-042 port-promotion rule).
//!
//! Both the per-record `ontology` blocks and the `market_ontology_map` tool
//! output (T4b) are generated from the constants in this module so they
//! cannot drift.

/// Bumped when the mapping shape changes; consumers compare against the
/// per-record `ontology.mapping_version` to detect evolution.
pub const MAPPING_VERSION: u32 = 1;

/// Polymarket market lifecycle stages (arXiv:2604.20421 §1), mapped onto the
/// PKO process axis. `dispute` is load-bearing: 2604.20421's oracle-risk
/// finding shows markets trade within 24h of a dispute anchor, so consumers
/// must be able to distrust prices at this stage without knowing UMA internals.
/// Used from T4's contract onward; consumed by `mapping_document()` so the
/// T4b tool output and per-record ontology blocks cannot drift.
pub const LIFECYCLE_STAGES: [&str; 6] = [
    "creation",
    "trading",
    "oracle_request",
    "proposal",
    "dispute",
    "settlement",
];

/// The mapping document returned by the `market_ontology_map` tool (T4b).
/// Built from the same constants that populate per-record `ontology` blocks
/// so the two cannot drift (pinned by test).
pub fn mapping_document() -> serde_json::Value {
    serde_json::json!({
        "mapping_version": MAPPING_VERSION,
        "process_axis": {
            "ontology": "PKO (Procedural Knowledge Ontology)",
            "vocabulary_crate": "hkask-bridge-ontology",
            "record_type": hkask_bridge_ontology::pko::PROCEDURE_EXECUTION,
            "probability_role": hkask_bridge_ontology::pko::WAS_GENERATED_BY,
            "lifecycle_stages": LIFECYCLE_STAGES,
            "stage_note": "distrust prices in the `dispute` stage (arXiv:2604.20421 oracle-risk finding: markets trade within 24h of a dispute anchor)"
        },
        "state_axis": {
            "ontology": "Dublin Core (dcterms)",
            "vocabulary_crate": "hkask-bridge-ontology",
            "fields": {
                "identifier": format!(
                    "{} = {{source}}:{{market_id}}",
                    hkask_bridge_ontology::dc_bibo::IDENTIFIER
                ),
                "title": format!("{} ← market question", hkask_bridge_ontology::dc_bibo::TITLE),
                "description": format!(
                    "{} ← market description (500-char cap)",
                    hkask_bridge_ontology::dc_bibo::DESCRIPTION
                ),
                "temporal": format!(
                    "{} ← deadline (drives horizon effects)",
                    hkask_bridge_ontology::dc_bibo::TEMPORAL
                ),
                "provenance": format!(
                    "{} ← resolution_source (uma_oracle | kalshi_exchange)",
                    hkask_bridge_ontology::dc_bibo::PROVENANCE
                ),
            }
        },
        "domain_supplement": {
            "note": "calibration vocabulary (brier, sample_size, liquidity/spread tier) is kask domain-supplement tier — no standard ontology covers forecast scoring (Q-O2 resolution 2026-08-05)"
        }
    })
}

// ── Tool-response dual-axis dispatch (P5.4) ────────────────────────────
//
// Operator ruling B.2 2026-10-18: every tool RESPONSE carries both axes.
// Wired as the central `ToolContext::annotate_response` hook via the
// `mcp_server!` macro's annotate override — one point stamps every tool
// response at the `execute_tool` boundary. The per-record `ontology`
// blocks this module already documents (the CMP mapping above) are
// NESTED inside market-record items, a different key path the hook does
// not touch — domain supplement, not replacement.

use hkask_bridge_ontology::dc_bibo::DcConcept;
pub use hkask_bridge_ontology::dc_bibo::{DATASET, REPORT};
use hkask_bridge_ontology::pko::PkoConcept;
pub use hkask_bridge_ontology::pko::{PROCEDURE_EXECUTION, STEP_EXECUTION};

/// The process-axis (PKO) concept a prediction-markets tool's response
/// occupies.
///
/// Returns `None` only for a name that is not a registered tool — every
/// registered tool has an explicit arm, pinned by
/// `dual_axis_covers_all_registered_tools` (below) and the tool-surface
/// pin over the live router.
pub(crate) fn tool_to_pko(tool_name: &str) -> Option<PkoConcept> {
    match tool_name {
        // Matching a scenario question to candidate markets and reading
        // a market's ladder observe the markets' prior executions —
        // compiled readings over provider records.
        "market_match" | "market_ladder" | "market_lookup" => Some(PROCEDURE_EXECUTION),
        // Market history reads price history — a reading over the
        // provider's executed trades.
        "market_history" => Some(PROCEDURE_EXECUTION),
        // Checking resolutions scans open and newly resolved markets and
        // snapshots resolution states — the loop's sensing step.
        "market_check_resolutions" => Some(PROCEDURE_EXECUTION),
        // Recording a resolution stores a calibration observation — a
        // verification step executing.
        "market_record_resolution" => Some(STEP_EXECUTION),
        // Calibration reads the measured Brier record for a bucket.
        "market_calibration" => Some(PROCEDURE_EXECUTION),
        // The residual decomposition computes a measurement over two
        // histories — a step executing.
        "market_residual" => Some(STEP_EXECUTION),
        // EQM rationale scoring executes the marker pass — an evaluation
        // step.
        "market_score_rationale" => Some(STEP_EXECUTION),
        // Volatility computes the DR-AS structural forecast — a
        // computation step.
        "market_volatility" => Some(STEP_EXECUTION),
        // Subscribing to resolutions registers an observation listener —
        // a step executing.
        "market_subscribe_resolutions" => Some(STEP_EXECUTION),
        // The ontology map documents this very mapping — a reading.
        "market_ontology_map" => Some(PROCEDURE_EXECUTION),
        // Server status is a liveness/reading check.
        "prediction_markets_status" => Some(PROCEDURE_EXECUTION),
        // Economic-data providers (DBnomics, FRED, World Bank): search and
        // list read provider catalogs — readings; series/dataset/indicator
        // fetches execute retrieval steps; observations fetch readings.
        "dbnomics_search" | "dbnomics_list_providers" => Some(PROCEDURE_EXECUTION),
        "dbnomics_get_dataset" | "dbnomics_get_series" => Some(STEP_EXECUTION),
        "fred_search_series" | "fred_list_categories" => Some(PROCEDURE_EXECUTION),
        "fred_get_release" => Some(PROCEDURE_EXECUTION),
        "fred_get_series_info" | "fred_get_observations" => Some(STEP_EXECUTION),
        "wb_list_topics" | "wb_list_countries" | "wb_search_indicators" => {
            Some(PROCEDURE_EXECUTION)
        }
        "wb_get_indicator_info" | "wb_get_observations" => Some(STEP_EXECUTION),
        _ => None,
    }
}

/// The state-axis (DC+BIBO) concept a prediction-markets tool's
/// response occupies.
///
/// Write responses are datasets (a stored or modified measurement);
/// read responses are compiled readings over stored records — reports.
pub(crate) fn tool_to_dc(tool_name: &str) -> Option<DcConcept> {
    match tool_name {
        // Reads: matches, lookups, ladders, history, resolution checks,
        // calibration reads, status, the mapping document, provider
        // catalog searches and listings, release metadata — reports.
        "market_match"
        | "market_lookup"
        | "market_ladder"
        | "market_history"
        | "market_check_resolutions"
        | "market_calibration"
        | "market_ontology_map"
        | "prediction_markets_status"
        | "dbnomics_search"
        | "dbnomics_list_providers"
        | "fred_search_series"
        | "fred_list_categories"
        | "fred_get_release" => Some(REPORT),
        // Writes and computations: resolution records, residuals,
        // rationale scores, volatility forecasts, subscriptions,
        // series/dataset/indicator/observation fetches — datasets.
        "market_record_resolution"
        | "market_residual"
        | "market_score_rationale"
        | "market_volatility"
        | "market_subscribe_resolutions"
        | "dbnomics_get_dataset"
        | "dbnomics_get_series"
        | "fred_get_series_info"
        | "fred_get_observations"
        | "wb_list_topics"
        | "wb_list_countries"
        | "wb_search_indicators"
        | "wb_get_indicator_info"
        | "wb_get_observations" => Some(DATASET),
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
        "market_",
        "prediction_markets_status",
        "dbnomics_",
        "fred_",
        "wb_"
    ],
    &["market_id"],
);

#[cfg(test)]
mod tests {
    use super::*;

    // The both-directions totality tests (coverage + the phantom-name
    // reverse scan) are emitted by the framework crate's
    // `dual_axis_annotate!` macro above (its own `dual_axis_tests`
    // module) — one definition across the nine stamped servers, including
    // its own generated TOOL_NAMES include. This module keeps the
    // prediction-markets-specific tests.

    /// The axes must discriminate: a market reading and a resolution
    /// record are different process concepts, and a read and a write are
    /// different state concepts.
    #[test]
    fn axes_discriminate_tool_families() {
        assert_eq!(tool_to_pko("market_match"), Some(PROCEDURE_EXECUTION));
        assert_eq!(
            tool_to_pko("market_record_resolution"),
            Some(STEP_EXECUTION)
        );

        assert_eq!(tool_to_dc("market_match"), Some(REPORT));
        assert_eq!(tool_to_dc("market_record_resolution"), Some(DATASET));
    }

    /// End-to-end: a real tool invocation through `execute_tool` carries
    /// the dual-axis stamp on its response — the central hook works, not
    /// just the mapping table. `prediction_markets_status` needs no
    /// credential and no network.
    #[tokio::test]
    async fn a_live_tool_response_carries_the_dual_axis_stamp() {
        use rmcp::handler::server::wrapper::Parameters;
        use std::collections::HashSet;
        use std::sync::{Arc, Mutex};

        // The shared noop inference double (one definition across the
        // servers), not a per-server copy.
        use hkask_mcp_server::server::test_support::NoopInferencePort as NoopPort;

        let server = crate::PredictionMarketsServer::new(
            hkask_types::WebID::new(),
            reqwest::Client::new(),
            crate::DEFAULT_CACHE_TTL_SECS,
            Arc::new(Mutex::new(crate::calibration::CalibrationStore::new())),
            crate::cache::TtlCache::new(crate::DEFAULT_CACHE_TTL_SECS),
            None,
            Mutex::new(HashSet::new()),
            None,
            Arc::new(NoopPort),
        );
        let response = server
            .prediction_markets_status(Parameters(crate::StatusRequest {}))
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
            "the live prediction_markets_status response carries the process axis: {content}"
        );
        assert_eq!(
            content["ontology"]["dc_type"],
            serde_json::json!(REPORT),
            "the live prediction_markets_status response carries the state axis: {content}"
        );
    }
}
