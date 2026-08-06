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
}
