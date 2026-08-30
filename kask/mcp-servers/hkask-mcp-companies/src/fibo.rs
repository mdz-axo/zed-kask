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

// Re-export the verified FIBO vocabulary from the shared bridge crate.
pub(crate) use hkask_bridge_ontology::fibo::{
    CORPORATION, INTERNAL_RATE_OF_RETURN, MARKET_CAPITALIZATION, PORTFOLIO, TICKER_SYMBOL,
};

// ── Internal metric identifiers ─────────────────────────────────────────
//
// Plain canonical names for the metrics the concept cache stores and the
// financial model projects. They double as the `"metric"` values in tool
// output JSON.

/// Profile fields.
pub(crate) const METRIC_TICKER_SYMBOL: &str = "ticker_symbol";
pub(crate) const METRIC_LEGAL_NAME: &str = "legal_name";
pub(crate) const METRIC_INDUSTRY_SECTOR: &str = "industry_sector";
pub(crate) const METRIC_INDUSTRY_CLASSIFICATION: &str = "industry_classification";
pub(crate) const METRIC_COUNTRY_OF_INCORPORATION: &str = "country_of_incorporation";
pub(crate) const METRIC_MARKET_CAPITALIZATION: &str = "market_capitalization";

/// Valuation multiples.
pub(crate) const METRIC_PRICE_EARNINGS_RATIO: &str = "price_earnings_ratio";
pub(crate) const METRIC_PRICE_TO_BOOK_RATIO: &str = "price_to_book_ratio";
pub(crate) const METRIC_PRICE_TO_SALES_RATIO: &str = "price_to_sales_ratio";

/// Profitability.
pub(crate) const METRIC_RETURN_ON_INVESTED_CAPITAL: &str = "return_on_invested_capital";
pub(crate) const METRIC_RETURN_ON_EQUITY: &str = "return_on_equity";
pub(crate) const METRIC_RETURN_ON_ASSETS: &str = "return_on_assets";
pub(crate) const METRIC_GROSS_PROFIT_MARGIN: &str = "gross_profit_margin";
pub(crate) const METRIC_OPERATING_PROFIT_MARGIN: &str = "operating_profit_margin";
pub(crate) const METRIC_NET_PROFIT_MARGIN: &str = "net_profit_margin";

/// Leverage.
pub(crate) const METRIC_DEBT_TO_EQUITY_RATIO: &str = "debt_to_equity_ratio";
pub(crate) const METRIC_FINANCIAL_LEVERAGE_RATIO: &str = "financial_leverage_ratio";
pub(crate) const METRIC_TOTAL_ASSETS: &str = "total_assets";
pub(crate) const METRIC_TOTAL_EQUITY: &str = "total_equity";
pub(crate) const METRIC_TREASURY_STOCK: &str = "treasury_stock";

/// Income / growth.
pub(crate) const METRIC_DIVIDEND_YIELD: &str = "dividend_yield";
pub(crate) const METRIC_REVENUE_GROWTH_RATE: &str = "revenue_growth_rate";
pub(crate) const METRIC_EPS_GROWTH_RATE: &str = "eps_growth_rate";

/// DCF model line items.
pub(crate) const METRIC_ENTERPRISE_VALUE: &str = "enterprise_value";
pub(crate) const METRIC_EQUITY_VALUE: &str = "equity_value";
pub(crate) const METRIC_INTRINSIC_VALUE_PER_SHARE: &str = "intrinsic_value_per_share";
pub(crate) const METRIC_FREE_CASH_FLOW: &str = "free_cash_flow";
pub(crate) const METRIC_CAPITAL_EXPENDITURE: &str = "capital_expenditure";
pub(crate) const METRIC_NET_DEBT: &str = "net_debt";
pub(crate) const METRIC_MARGIN_OF_SAFETY: &str = "margin_of_safety";
pub(crate) const METRIC_DEPRECIATION_AND_AMORTIZATION: &str = "depreciation_and_amortization";
pub(crate) const METRIC_NET_WORKING_CAPITAL: &str = "net_working_capital";
pub(crate) const METRIC_DISCOUNT_RATE: &str = "discount_rate";

// ── FMP/EODHD field → metric mapping ────────────────────────────────────

/// Map an FMP/EODHD API field name to its internal metric identifier.
/// Returns None for fields not covered (provider-specific metadata).
pub(crate) fn fmp_field_to_metric(field: &str) -> Option<&'static str> {
    match field {
        // Profile
        "symbol" => Some(METRIC_TICKER_SYMBOL),
        "companyName" => Some(METRIC_LEGAL_NAME),
        "sector" => Some(METRIC_INDUSTRY_SECTOR),
        "industry" => Some(METRIC_INDUSTRY_CLASSIFICATION),
        "country" => Some(METRIC_COUNTRY_OF_INCORPORATION),
        "mktCap" => Some(METRIC_MARKET_CAPITALIZATION),

        // Valuation
        "peRatio" => Some(METRIC_PRICE_EARNINGS_RATIO),
        "priceToBookRatio" => Some(METRIC_PRICE_TO_BOOK_RATIO),
        "priceToSalesRatio" => Some(METRIC_PRICE_TO_SALES_RATIO),

        // Profitability
        "roic" => Some(METRIC_RETURN_ON_INVESTED_CAPITAL),
        "roe" => Some(METRIC_RETURN_ON_EQUITY),
        "roa" => Some(METRIC_RETURN_ON_ASSETS),
        "grossProfitMargin" => Some(METRIC_GROSS_PROFIT_MARGIN),
        "operatingProfitMargin" => Some(METRIC_OPERATING_PROFIT_MARGIN),
        "netProfitMargin" => Some(METRIC_NET_PROFIT_MARGIN),

        // Leverage
        "debtToEquity" => Some(METRIC_DEBT_TO_EQUITY_RATIO),
        "financialLeverage" => Some(METRIC_FINANCIAL_LEVERAGE_RATIO),
        "totalAssets" => Some(METRIC_TOTAL_ASSETS),
        "totalEquity" => Some(METRIC_TOTAL_EQUITY),
        "treasuryStock" => Some(METRIC_TREASURY_STOCK),

        // Income / growth
        "dividendYield" => Some(METRIC_DIVIDEND_YIELD),
        "revenueGrowth" => Some(METRIC_REVENUE_GROWTH_RATE),
        "epsGrowth" => Some(METRIC_EPS_GROWTH_RATE),

        // DCF valuation
        "enterpriseValue" => Some(METRIC_ENTERPRISE_VALUE),
        "equityValue" => Some(METRIC_EQUITY_VALUE),
        "intrinsicValuePerShare" => Some(METRIC_INTRINSIC_VALUE_PER_SHARE),
        "freeCashFlow" => Some(METRIC_FREE_CASH_FLOW),
        "capitalExpenditure" => Some(METRIC_CAPITAL_EXPENDITURE),
        "netDebt" => Some(METRIC_NET_DEBT),
        "marginOfSafety" => Some(METRIC_MARGIN_OF_SAFETY),

        // Not covered (FMP/EODHD-specific metadata)
        _ => None,
    }
}

// ── Tool → ontology anchor mapping ──────────────────────────────────────

/// Map a companies-server tool name to its top-level ontology concept URI —
/// the concept that represents *what the artifact is* (not the per-field
/// metric identifiers). This is the unified `"ontology"` field the widget
/// reads for the "I" pattern dispatch and the compose-back body, AND the
/// concept tagged on the `reg.tool.*` span via `execute_tool` for
/// type-aware feedback routing.
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
        "portfolio_attribution"
        | "dcf_valuation"
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
        | "driver_forecast"
        | "moat_check"
        | "management_scorecard"
        | "working_capital_cycle" => Some(dc_bibo::REPORT),

        // Data outputs — structured data, not analysis → Dublin Core.
        "portfolio_characteristics"
        | "stock_screener"
        | "stock_universe"
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
        "report_save" | "report_load" | "report_list" => Some(dc_bibo::REPORT),

        _ => None,
    }
}

/// Inject the unified `"ontology"` key into a tool output `Value` if the tool
/// has an ontology concept mapping. Tools without a mapping are returned unchanged.
/// This is the companies-server equivalent of the media server's
/// `enrich_with_omc_and_provenance` — it bakes the ontology concept into the
/// tool output so the portfolio widget can read it for the "I" pattern dispatch
/// and the compose-back body. The `reg.tool.*` span carries the same concept
/// via `execute_tool` (wired separately).
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
