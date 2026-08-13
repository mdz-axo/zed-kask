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
pub use hkask_bridge_ontology::fibo::{
    ALL_CONCEPTS, ATTRIBUTION_ANALYSIS, BARRIER_TO_ENTRY, BRIER_SCORE, BUY_TRANSACTION,
    CAPITAL_ALLOCATION, CAPITAL_EXPENDITURE, COMPARABLE_COMPANY_ANALYSIS, COMPETITIVE_ADVANTAGE,
    CORPORATION, COST_OF_CAPITAL, COST_OF_GOODS_SOLD, COUNTRY_OF_INCORPORATION, DCF_VALUATION,
    DEBT_TO_EQUITY_RATIO, DEPOSIT_TRANSACTION, DEPRECIATION_AND_AMORTIZATION, DISCOUNT_RATE,
    DIVIDEND_TRANSACTION, DIVIDEND_YIELD, EBIT, ECONOMIC_PROFIT, EFFECTIVE_TAX_RATE,
    ENTERPRISE_VALUE, ENTERPRISE_VALUE_MULTIPLE, EPS_GROWTH_RATE, EQUITY_VALUE,
    FINANCIAL_LEVERAGE_RATIO, FORECAST_ID, FREE_CASH_FLOW, GROSS_PROFIT_MARGIN, HAS_RISK,
    HAS_UNCERTAINTY, HOLDING_WEIGHT, INDUSTRY_CLASSIFICATION, INDUSTRY_SECTOR,
    INTERNAL_RATE_OF_RETURN, INTRINSIC_VALUE, INTRINSIC_VALUE_PER_SHARE, LEGAL_NAME,
    MARGIN_OF_SAFETY, MARKET_CAPITALIZATION, MONTE_CARLO_DCF, NET_DEBT, NET_PROFIT_MARGIN,
    NET_WORKING_CAPITAL, NOPAT, OPERATING_PROFIT_MARGIN, PORTFOLIO, PRICE_EARNINGS_RATIO,
    PRICE_TO_BOOK_RATIO, PRICE_TO_SALES_RATIO, PROBABILITY_OF_UNDERVALUATION, REINVESTMENT_RATE,
    RETURN_ON_ASSETS, RETURN_ON_CAPITAL, RETURN_ON_EQUITY, RETURN_ON_INVESTED_CAPITAL,
    REVENUE_GROWTH_RATE, SCENARIO_PROBABILITY, SECURITY_HOLDING, SELL_TRANSACTION,
    SENSITIVITY_ANALYSIS, STOCK_SCREENER, TERMINAL_GROWTH_RATE, TICKER_SYMBOL,
    TIME_WEIGHTED_RETURN, TOTAL_ASSETS, TOTAL_EQUITY, TRANSACTION_LEDGER, TREASURY_STOCK,
    WEIGHTED_AVERAGE, WITHDRAWAL_TRANSACTION,
};

// Re-export the concept type so call sites that reference `fibo::FiboConcept`
// keep resolving.
pub use hkask_bridge_ontology::fibo::FiboConcept;

// ── FMP/EODHD field → FIBO concept mapping ──────────────────────────────

/// Map an FMP/EODHD API field name to its FIBO concept URI.
/// Returns None for fields not covered by FIBO (provider-specific metadata).
pub fn fmp_field_to_fibo(field: &str) -> Option<FiboConcept> {
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
pub fn tool_to_ontology(tool: &str) -> Option<&'static str> {
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
        "forecast_record" | "forecast_get" | "forecast_list" => Some(FORECAST_ID),
        "result_feedback" => Some(BRIER_SCORE),
        // Analysis tools
        "company_screener" => Some(STOCK_SCREENER),
        "moat_check" => Some(COMPETITIVE_ADVANTAGE),
        "management_scorecard" => Some(CAPITAL_ALLOCATION),
        "working_capital_cycle" => Some(NET_WORKING_CAPITAL),
        "research_search" => Some(CORPORATION),
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
pub fn enrich_with_ontology(mut result: serde_json::Value, tool: &str) -> serde_json::Value {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn financial_leverage_maps_to_its_ratio_concept() {
        assert_eq!(
            fmp_field_to_fibo("financialLeverage"),
            Some(FINANCIAL_LEVERAGE_RATIO)
        );
    }

    #[test]
    fn fibo_unknown_field_returns_none() {
        assert!(fmp_field_to_fibo("someFmpSpecificMetadata").is_none());
        assert!(fmp_field_to_fibo("").is_none());
    }

    #[test]
    fn tool_to_ontology_maps_portfolio_tools() {
        assert_eq!(tool_to_ontology("portfolio_list"), Some(PORTFOLIO));
        assert_eq!(
            tool_to_ontology("portfolio_returns"),
            Some(TIME_WEIGHTED_RETURN)
        );
        assert_eq!(tool_to_ontology("ledger_import"), Some(TRANSACTION_LEDGER));
        assert_eq!(
            tool_to_ontology("portfolio_comparison"),
            Some(COMPARABLE_COMPANY_ANALYSIS)
        );
    }

    #[test]
    fn tool_to_ontology_maps_valuation_tools() {
        assert_eq!(tool_to_ontology("dcf_valuation"), Some(DCF_VALUATION));
        assert_eq!(tool_to_ontology("ep_valuation"), Some(ECONOMIC_PROFIT));
        assert_eq!(
            tool_to_ontology("comparable_analysis"),
            Some(COMPARABLE_COMPANY_ANALYSIS)
        );
        assert_eq!(tool_to_ontology("monte_carlo_dcf"), Some(MONTE_CARLO_DCF));
        assert_eq!(
            tool_to_ontology("sensitivity_analysis"),
            Some(SENSITIVITY_ANALYSIS)
        );
    }

    #[test]
    fn tool_to_ontology_maps_analysis_tools() {
        assert_eq!(tool_to_ontology("company_screener"), Some(STOCK_SCREENER));
        assert_eq!(tool_to_ontology("research_search"), Some(CORPORATION));
        assert_eq!(
            tool_to_ontology("portfolio_attribution"),
            Some(ATTRIBUTION_ANALYSIS)
        );
    }

    #[test]
    fn tool_to_ontology_maps_financial_data_tools() {
        assert_eq!(tool_to_ontology("company_profile"), Some(CORPORATION));
        assert_eq!(tool_to_ontology("stock_quote"), Some(MARKET_CAPITALIZATION));
        assert_eq!(tool_to_ontology("income_statement"), Some(EBIT));
        assert_eq!(tool_to_ontology("balance_sheet"), Some(TOTAL_ASSETS));
        assert_eq!(
            tool_to_ontology("cash_flow_statement"),
            Some(FREE_CASH_FLOW)
        );
        assert_eq!(tool_to_ontology("symbol_search"), Some(TICKER_SYMBOL));
    }

    #[test]
    fn tool_to_ontology_maps_forecast_tools() {
        // calibrate_forecast is about scoring accuracy → BRIER_SCORE.
        assert_eq!(tool_to_ontology("calibrate_forecast"), Some(BRIER_SCORE));
        // forecast_record/get/list are about forecast identity.
        assert_eq!(tool_to_ontology("forecast_record"), Some(FORECAST_ID));
        assert_eq!(tool_to_ontology("forecast_get"), Some(FORECAST_ID));
        assert_eq!(tool_to_ontology("forecast_list"), Some(FORECAST_ID));
        // result_feedback is about forecast accuracy feedback.
        assert_eq!(tool_to_ontology("result_feedback"), Some(BRIER_SCORE));
    }

    #[test]
    fn tool_to_ontology_maps_non_financial_artifacts_to_dc() {
        // Notes and transcripts are text artifacts — Dublin Core, not FIBO.
        assert_eq!(
            tool_to_ontology("note_add"),
            Some(hkask_bridge_ontology::dc_bibo::TEXT)
        );
        assert_eq!(
            tool_to_ontology("company_transcript"),
            Some(hkask_bridge_ontology::dc_bibo::TEXT)
        );
        // Files are datasets.
        assert_eq!(
            tool_to_ontology("file_attach"),
            Some(hkask_bridge_ontology::dc_bibo::DATASET)
        );
    }

    #[test]
    fn tool_to_ontology_unknown_returns_none() {
        assert!(tool_to_ontology("").is_none());
        assert!(tool_to_ontology("nonexistent_tool").is_none());
    }

    #[test]
    fn enrich_with_ontology_injects_concept() {
        let result = enrich_with_ontology(serde_json::json!({"status": "ok"}), "portfolio_list");
        assert_eq!(result["ontology"], "fibo-sec-sec-ast:Portfolio");
    }

    #[test]
    fn enrich_with_ontology_injects_dc_concept_for_notes() {
        let result = enrich_with_ontology(serde_json::json!({"status": "ok"}), "note_add");
        assert_eq!(result["ontology"], "dcterms:Text");
    }

    #[test]
    fn enrich_with_ontology_no_mapping_leaves_result_unchanged() {
        let result = enrich_with_ontology(serde_json::json!({"status": "ok"}), "nonexistent_tool");
        assert!(result.get("ontology").is_none());
    }
}
