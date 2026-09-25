//! FIBO dispatch + internal metric vocabulary for the companies server.
//!
//! The verified FIBO concept constants live in the shared
//! `hkask-bridge-ontology` crate (fixture-pinned against the FIBO master
//! ontology). This module re-exports them, defines the server's internal
//! metric identifiers, and holds the tool → ontology anchor mapping.
//!
//! The `METRIC_*` constants are hKask-internal canonical metric names —
//! NOT ontology URIs and not FIBO terms. FIBO publishes no terms for
//! financial ratios, DCF line items, or valuation methods (verified
//! against the FIBO master ontology 2026-08-29); these keys identify
//! metrics in the concept cache and the financial model and claim no
//! external standard.
//!
//! Tool anchors follow the operator decision (2026-08-29): tools whose
//! concept FIBO actually publishes anchor on FIBO; analysis-family tools
//! with no FIBO equivalent anchor on Dublin Core (analysis outputs are
//! reports, data outputs are datasets) — never an invented FIBO URI.

// Re-export the verified FIBO vocabulary from the shared bridge crate
// (the terms this server anchors on).
pub(crate) use hkask_bridge_ontology::fibo::{CORPORATION, MARKET_CAPITALIZATION, TICKER_SYMBOL};

// Internal metric identifiers used by the canonical DCF sensitivity output.
pub(crate) const METRIC_GROSS_PROFIT_MARGIN: &str = "gross_profit_margin";
pub(crate) const METRIC_REVENUE_GROWTH_RATE: &str = "revenue_growth_rate";
pub(crate) const METRIC_CAPITAL_EXPENDITURE: &str = "capital_expenditure";
pub(crate) const METRIC_DEPRECIATION_AND_AMORTIZATION: &str = "depreciation_and_amortization";
pub(crate) const METRIC_NET_WORKING_CAPITAL: &str = "net_working_capital";
pub(crate) const METRIC_DISCOUNT_RATE: &str = "discount_rate";
// ── Tool → ontology anchor mapping ──────────────────────────────────────

/// Map a companies-server tool name to its top-level ontology concept URI —
/// the concept that represents *what the artifact is* (not the per-field
/// metric identifiers). This is the unified `"ontology"` field the widget
/// reads for the "I" pattern dispatch and the compose-back body.
///
/// Anchoring policy (operator decision 2026-08-29): tools whose concept
/// FIBO actually publishes anchor on the verified FIBO URI; analysis-family
/// tools with no FIBO equivalent anchor on Dublin Core — analysis outputs
/// are reports (`bibo:Report`), data outputs are datasets
/// (`dcterms:Dataset`), text artifacts are `dcterms:Text`. No invented
/// FIBO URIs.
///
/// Returns `None` only for tools that produce no artifact worth anchoring
/// (currently none — all tools are mapped).
pub(crate) fn tool_to_ontology(tool: &str) -> Option<&'static str> {
    use hkask_bridge_ontology::dc_bibo;
    match tool {
        // Real FIBO anchors — FIBO publishes these concepts.
        "company_profile" | "company_research_search" => Some(CORPORATION),
        "stock_quote" | "historical_price" => Some(MARKET_CAPITALIZATION),
        "symbol_search" | "resolve_symbol" => Some(TICKER_SYMBOL),

        // Analysis-family tools — no FIBO equivalent (verified 2026-08-29);
        // their outputs are analysis reports → Dublin Core.
        "dcf_valuation"
        | "reverse_dcf"
        | "ep_valuation"
        | "expectations_gap"
        | "scenario_analysis"
        | "scenario_impact_valuation"
        | "comparable_analysis"
        | "monte_carlo_dcf"
        | "sensitivity_analysis"
        | "equity_duration"
        | "calibrate_forecast"
        | "moat_check"
        | "management_scorecard"
        | "working_capital_cycle" => Some(dc_bibo::REPORT),

        // Data outputs — structured data, not analysis → Dublin Core.
        "stock_screener"
        | "company_screener"
        | "key_metrics"
        | "income_statement"
        | "balance_sheet"
        | "cash_flow_statement"
        | "forecast_record"
        | "forecast_get"
        | "forecast_list"
        | "forecast_persist"
        | "result_feedback" => Some(dc_bibo::DATASET),

        // Non-financial artifacts — Dublin Core (text/dataset artifacts).
        "company_transcript" | "note_add" | "note_list" | "note_delete" => Some(dc_bibo::TEXT),
        "file_attach" | "file_list" | "file_delete" => Some(dc_bibo::DATASET),
        "report_save" | "report_load" | "report_list" | "company_verification_packet_check" => {
            Some(dc_bibo::REPORT)
        }

        _ => None,
    }
}

/// Inject the unified `"ontology"` key into a tool output `Value` if the tool
/// has an ontology concept mapping. Tools without a mapping are returned unchanged.
/// This is the companies-server equivalent of the media server's
/// `enrich_with_omc_and_provenance` — it bakes the ontology concept into the
/// tool output so the portfolio widget can read it for the "I" pattern dispatch
/// and the compose-back body.
pub(crate) fn enrich_with_ontology(mut result: serde_json::Value, tool: &str) -> serde_json::Value {
    if let Some(concept) = tool_to_ontology(tool) {
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "ontology".to_string(),
                serde_json::Value::String(concept.to_string()),
            );
        }
    }
    result
}
