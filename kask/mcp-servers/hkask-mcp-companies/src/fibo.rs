//! FIBO dispatch for the companies server — FMP/EODHD field mapping.
//!
//! The FIBO concept vocabulary (the canonical `fibo-*` URIs) lives in the
//! shared `hkask-bridge-ontology` crate. This module re-exports those
//! constants so the companies server's existing `fibo::CONSTANT` call sites
//! keep working, and holds the server-specific mapping from FMP/EODHD
//! provider field names to their FIBO concepts. That mapping is the
//! server's business (it knows the provider's field names), not the
//! ontology's.

// Re-export the FIBO vocabulary from the shared bridge crate.
pub(crate) use hkask_bridge_ontology::fibo::{
    ATTRIBUTION_ANALYSIS, BRIER_SCORE, CAPITAL_ALLOCATION, CAPITAL_EXPENDITURE,
    COMPARABLE_COMPANY_ANALYSIS, COMPETITIVE_ADVANTAGE, CORPORATION, COUNTRY_OF_INCORPORATION,
    DCF_VALUATION, DEBT_TO_EQUITY_RATIO, DEPRECIATION_AND_AMORTIZATION, DISCOUNT_RATE,
    DIVIDEND_YIELD, EBIT, ECONOMIC_PROFIT, ENTERPRISE_VALUE, ENTERPRISE_VALUE_MULTIPLE,
    EPS_GROWTH_RATE, EQUITY_VALUE, FINANCIAL_LEVERAGE_RATIO, FORECAST_ID, FREE_CASH_FLOW,
    GROSS_PROFIT_MARGIN, INDUSTRY_CLASSIFICATION, INDUSTRY_SECTOR, INTERNAL_RATE_OF_RETURN,
    INTRINSIC_VALUE_PER_SHARE, LEGAL_NAME, MARGIN_OF_SAFETY, MARKET_CAPITALIZATION,
    MONTE_CARLO_DCF, NET_DEBT, NET_PROFIT_MARGIN, NET_WORKING_CAPITAL, OPERATING_PROFIT_MARGIN,
    PORTFOLIO, PRICE_EARNINGS_RATIO, PRICE_TO_BOOK_RATIO, PRICE_TO_SALES_RATIO,
    PROBABILITY_OF_UNDERVALUATION, RETURN_ON_ASSETS, RETURN_ON_EQUITY, RETURN_ON_INVESTED_CAPITAL,
    REVENUE_GROWTH_RATE, SCENARIO_PROBABILITY, SENSITIVITY_ANALYSIS, STOCK_SCREENER,
    TERMINAL_GROWTH_RATE, TICKER_SYMBOL, TIME_WEIGHTED_RETURN, TOTAL_ASSETS, TOTAL_EQUITY,
    TRANSACTION_LEDGER, TREASURY_STOCK, WEIGHTED_AVERAGE,
};

// Re-export the concept type so call sites that reference `fibo::FiboConcept`
// keep resolving.
pub(crate) use hkask_bridge_ontology::fibo::FiboConcept;

// ── FMP/EODHD field → FIBO concept mapping ──────────────────────────────

/// Map an FMP/EODHD API field name to its FIBO concept URI.
/// Returns None for fields not covered by FIBO (provider-specific metadata).
pub(crate) fn fmp_field_to_fibo(field: &str) -> Option<FiboConcept> {
    match field {
        // Profile
        "symbol" => Some(TICKER_SYMBOL),
        "companyName" => Some(LEGAL_NAME),
        "sector" => Some(INDUSTRY_SECTOR),
        "industry" => Some(INDUSTRY_CLASSIFICATION),
        "country" => Some(COUNTRY_OF_INCORPORATION),
        "mktCap" => Some(MARKET_CAPITALIZATION),

        // Valuation
        "peRatio" => Some(PRICE_EARNINGS_RATIO),
        "priceToBookRatio" => Some(PRICE_TO_BOOK_RATIO),
        "priceToSalesRatio" => Some(PRICE_TO_SALES_RATIO),

        // Profitability
        "roic" => Some(RETURN_ON_INVESTED_CAPITAL),
        "roe" => Some(RETURN_ON_EQUITY),
        "roa" => Some(RETURN_ON_ASSETS),
        "grossProfitMargin" => Some(GROSS_PROFIT_MARGIN),
        "operatingProfitMargin" => Some(OPERATING_PROFIT_MARGIN),
        "netProfitMargin" => Some(NET_PROFIT_MARGIN),

        // Leverage
        "debtToEquity" => Some(DEBT_TO_EQUITY_RATIO),
        "financialLeverage" => Some(FINANCIAL_LEVERAGE_RATIO),
        "totalAssets" => Some(TOTAL_ASSETS),
        "totalEquity" => Some(TOTAL_EQUITY),
        "treasuryStock" => Some(TREASURY_STOCK),

        // Income / growth
        "dividendYield" => Some(DIVIDEND_YIELD),
        "revenueGrowth" => Some(REVENUE_GROWTH_RATE),
        "epsGrowth" => Some(EPS_GROWTH_RATE),

        // DCF valuation
        "enterpriseValue" => Some(ENTERPRISE_VALUE),
        "equityValue" => Some(EQUITY_VALUE),
        "intrinsicValuePerShare" => Some(INTRINSIC_VALUE_PER_SHARE),
        "freeCashFlow" => Some(FREE_CASH_FLOW),
        "capitalExpenditure" => Some(CAPITAL_EXPENDITURE),
        "netDebt" => Some(NET_DEBT),
        "marginOfSafety" => Some(MARGIN_OF_SAFETY),

        // Not covered by FIBO (FMP/EODHD-specific metadata)
        _ => None,
    }
}

/// Map a companies-server tool name to its top-level ontology concept URI —
/// the concept that represents *what the artifact is* (not the per-field concepts
/// in the `"fibo"` map). This is the unified `"ontology"` field the widget
/// reads for the "I" pattern dispatch and the compose-back body, AND the
/// concept tagged on the `reg.tool.*` span via `execute_tool_semantic` for
/// type-aware feedback routing.
///
/// Financial tools return FIBO concepts (`fibo-*` URIs). Non-financial
/// artifacts (notes, files, transcripts) return Dublin Core concepts
/// (`dcterms:Text`, `dcterms:Dataset`) — these are text/dataset artifacts,
/// not financial instruments, so FIBO does not cover them. Both are
/// `&'static str`, compatible with `execute_tool_semantic`'s
/// `ontology: Option<&'static str>` parameter.
///
/// Returns `None` only for tools that produce no artifact worth anchoring
/// (currently none — all 44 tools are mapped).
pub(crate) fn tool_to_ontology(tool: &str) -> Option<&'static str> {
    use hkask_bridge_ontology::dc_bibo;
    match tool {
        // Portfolio tools
        "portfolio_list" | "portfolio_delete" => Some(PORTFOLIO),
        "portfolio_comparison" => Some(COMPARABLE_COMPANY_ANALYSIS),
        "portfolio_returns" => Some(TIME_WEIGHTED_RETURN),
        "portfolio_attribution" => Some(ATTRIBUTION_ANALYSIS),
        "portfolio_characteristics" => Some(WEIGHTED_AVERAGE),
        "ledger_import" | "ledger_export" | "transaction_note_append" => Some(TRANSACTION_LEDGER),
        // Valuation tools
        "dcf_valuation" | "reverse_dcf" => Some(DCF_VALUATION),
        "ep_valuation" => Some(ECONOMIC_PROFIT),
        "expectations_gap" => Some(INTRINSIC_VALUE_PER_SHARE),
        "scenario_analysis" | "scenario_impact_valuation" => Some(SCENARIO_PROBABILITY),
        "comparable_analysis" => Some(COMPARABLE_COMPANY_ANALYSIS),
        "monte_carlo_dcf" => Some(MONTE_CARLO_DCF),
        "sensitivity_analysis" => Some(SENSITIVITY_ANALYSIS),
        "equity_duration" => Some(INTERNAL_RATE_OF_RETURN),
        // Forecast tools
        "calibrate_forecast" => Some(BRIER_SCORE),
        "forecast_record" | "forecast_get" | "forecast_list" | "forecast_persist" => {
            Some(FORECAST_ID)
        }
        "result_feedback" => Some(BRIER_SCORE),
        // Analysis tools
        "company_screener" => Some(STOCK_SCREENER),
        "moat_check" => Some(COMPETITIVE_ADVANTAGE),
        "management_scorecard" => Some(CAPITAL_ALLOCATION),
        "working_capital_cycle" => Some(NET_WORKING_CAPITAL),
        "company_research_search" => Some(CORPORATION),
        // Financial data tools
        "company_profile" => Some(CORPORATION),
        "stock_quote" | "historical_price" => Some(MARKET_CAPITALIZATION),
        "key_metrics" => Some(PRICE_EARNINGS_RATIO),
        "income_statement" => Some(EBIT),
        "balance_sheet" => Some(TOTAL_ASSETS),
        "cash_flow_statement" => Some(FREE_CASH_FLOW),
        "symbol_search" => Some(TICKER_SYMBOL),
        // Non-financial artifacts — Dublin Core (text/dataset artifacts)
        "company_transcript" | "note_add" | "note_list" | "note_delete" => Some(dc_bibo::TEXT),
        "file_attach" | "file_list" | "file_delete" => Some(dc_bibo::DATASET),

        _ => None,
    }
}

/// Inject the unified `"ontology"` key into a tool output `Value` if the tool
/// has an ontology concept mapping. Tools without a mapping are returned unchanged.
/// This is the companies-server equivalent of the media server's
/// `enrich_with_omc_and_provenance` — it bakes the ontology concept into the
/// tool output so the portfolio widget can read it for the "I" pattern dispatch
/// and the compose-back body. The `reg.tool.*` span carries the same concept
/// via `execute_tool_semantic` (wired separately).
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
