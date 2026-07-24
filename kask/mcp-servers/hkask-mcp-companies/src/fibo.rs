//! FIBO ontology mapping for hkask-mcp-companies.
//!
//! Maps FMP/EODHD API field names to FIBO (Financial Industry Business Ontology)
//! standard concepts. FIBO is the OMG standard for financial data — built by
//! Goldman Sachs, Citigroup, Bloomberg, the Fed, and others. We anchor to FIBO
//! rather than inventing our own taxonomy.
//!
//! Reference: <https://spec.edmcouncil.org/fibo/>
//!
//! Key FIBO modules used:
//! - fibo-fbc-fct-ra  — Financial Concepts: Financial Ratios (Release)
//! - fibo-sec-sec-ast — Securities: Security Assets (Release)
//! - fibo-be-le-corp  — Business Entities: Corporations (Release)
//! - fibo-fnd-gao-gao — Foundations: Geographies (Release)
//! - fibo-ind-ind-ind — Indices and Indicators: Weighted Averages

/// A FIBO concept URI — the canonical identifier for a financial data concept.
pub type FiboConcept = &'static str;

// ── FIBO concept constants ──────────────────────────────────────────────

/// Business entity — the company itself.
pub const CORPORATION: FiboConcept = "fibo-be-le-corp:Corporation";
pub const LEGAL_NAME: FiboConcept = "fibo-fnd-utl-alias:legalName";
pub const TICKER_SYMBOL: FiboConcept = "fibo-sec-sec-lst:tickerSymbol";
pub const COUNTRY_OF_INCORPORATION: FiboConcept = "fibo-fnd-arr-arr:CountryOfIncorporation";
pub const INDUSTRY_SECTOR: FiboConcept = "fibo-fnd-gao-gao:IndustrySectorClassification";
pub const INDUSTRY_CLASSIFICATION: FiboConcept = "fibo-fnd-gao-gao:IndustryClassification";

/// Market data.
pub const MARKET_CAPITALIZATION: FiboConcept = "fibo-fbc-fct-ra:MarketCapitalization";

/// Valuation multiples.
pub const PRICE_EARNINGS_RATIO: FiboConcept = "fibo-fbc-fct-ra:PriceEarningsRatio";
pub const PRICE_TO_BOOK_RATIO: FiboConcept = "fibo-fbc-fct-ra:PriceToBookRatio";
pub const PRICE_TO_SALES_RATIO: FiboConcept = "fibo-fbc-fct-ra:PriceToSalesRatio";

/// Profitability.
pub const RETURN_ON_INVESTED_CAPITAL: FiboConcept = "fibo-fbc-fct-ra:ReturnOnInvestedCapital";
pub const RETURN_ON_EQUITY: FiboConcept = "fibo-fbc-fct-ra:ReturnOnEquity";
pub const RETURN_ON_ASSETS: FiboConcept = "fibo-fbc-fct-ra:ReturnOnAssets";
pub const GROSS_PROFIT_MARGIN: FiboConcept = "fibo-fbc-fct-ra:GrossProfitMargin";
pub const OPERATING_PROFIT_MARGIN: FiboConcept = "fibo-fbc-fct-ra:OperatingProfitMargin";
pub const NET_PROFIT_MARGIN: FiboConcept = "fibo-fbc-fct-ra:NetProfitMargin";

/// Leverage.
pub const DEBT_TO_EQUITY_RATIO: FiboConcept = "fibo-fbc-fct-ra:DebtToEquityRatio";
pub const FINANCIAL_LEVERAGE_RATIO: FiboConcept = "fibo-fbc-fct-ra:FinancialLeverageRatio";
pub const TOTAL_ASSETS: FiboConcept = "fibo-fbc-pas-fpas:TotalAssets";
pub const TOTAL_EQUITY: FiboConcept = "fibo-fbc-pas-fpas:TotalEquity";
pub const TREASURY_STOCK: FiboConcept = "fibo-fbc-pas-fpas:TreasuryStock";

/// Income.
pub const DIVIDEND_YIELD: FiboConcept = "fibo-fbc-fct-ra:DividendYield";
pub const REVENUE_GROWTH_RATE: FiboConcept = "fibo-fbc-fct-ra:RevenueGrowthRate";
pub const EPS_GROWTH_RATE: FiboConcept = "fibo-fbc-fct-ra:EarningsPerShareGrowthRate";

/// DCF valuation concepts.
pub const EFFECTIVE_TAX_RATE: FiboConcept = "fibo-fbc-fct-ra:EffectiveTaxRate";
pub const DISCOUNT_RATE: FiboConcept = "fibo-fbc-fct-ra:DiscountRate";
pub const TERMINAL_GROWTH_RATE: FiboConcept = "fibo-fbc-fct-ra:TerminalGrowthRate";
pub const ENTERPRISE_VALUE: FiboConcept = "fibo-fbc-fct-ra:EnterpriseValue";
pub const EQUITY_VALUE: FiboConcept = "fibo-fbc-fct-ra:EquityValue";
pub const INTRINSIC_VALUE_PER_SHARE: FiboConcept = "fibo-fbc-fct-ra:IntrinsicValuePerShare";
pub const FREE_CASH_FLOW: FiboConcept = "fibo-fbc-fct-ra:FreeCashFlow";
pub const CAPITAL_EXPENDITURE: FiboConcept = "fibo-fbc-fct-ra:CapitalExpenditure";
pub const DEPRECIATION_AND_AMORTIZATION: FiboConcept =
    "fibo-fbc-fct-ra:DepreciationAndAmortization";
pub const NET_WORKING_CAPITAL: FiboConcept = "fibo-fbc-fct-ra:NetWorkingCapital";
pub const NET_DEBT: FiboConcept = "fibo-fbc-fct-ra:NetDebt";
pub const COST_OF_GOODS_SOLD: FiboConcept = "fibo-fbc-fct-ra:CostOfGoodsSold";
pub const EBIT: FiboConcept = "fibo-fbc-fct-ra:EarningsBeforeInterestAndTaxes";
pub const NOPAT: FiboConcept = "fibo-fbc-fct-ra:NetOperatingProfitAfterTax";
pub const MARGIN_OF_SAFETY: FiboConcept = "fibo-fbc-fct-ra:MarginOfSafety";

/// Portfolio concepts.
pub const PORTFOLIO: FiboConcept = "fibo-sec-sec-ast:Portfolio";
pub const SECURITY_HOLDING: FiboConcept = "fibo-sec-sec-ast:SecurityHolding";
pub const HOLDING_WEIGHT: FiboConcept = "fibo-sec-sec-ast:holdingWeight";
pub const WEIGHTED_AVERAGE: FiboConcept = "fibo-ind-ind-ind:WeightedAverage";
pub const TRANSACTION_LEDGER: FiboConcept = "fibo-sec-sec-ast:TransactionLedger";
pub const BUY_TRANSACTION: FiboConcept = "fibo-sec-sec-ast:BuyTransaction";
pub const SELL_TRANSACTION: FiboConcept = "fibo-sec-sec-ast:SellTransaction";
pub const DIVIDEND_TRANSACTION: FiboConcept = "fibo-sec-sec-ast:DividendTransaction";
pub const DEPOSIT_TRANSACTION: FiboConcept = "fibo-sec-sec-ast:DepositTransaction";
pub const WITHDRAWAL_TRANSACTION: FiboConcept = "fibo-sec-sec-ast:WithdrawalTransaction";
pub const ATTRIBUTION_ANALYSIS: FiboConcept = "fibo-fbc-fct-ra:AttributionAnalysis";
pub const TIME_WEIGHTED_RETURN: FiboConcept = "fibo-fbc-fct-ra:TimeWeightedReturn";
pub const INTERNAL_RATE_OF_RETURN: FiboConcept = "fibo-fbc-fct-ra:InternalRateOfReturn";

/// Comparable company analysis.
pub const COMPARABLE_COMPANY_ANALYSIS: FiboConcept = "fibo-fbc-fct-ra:ComparableCompanyAnalysis";
pub const ENTERPRISE_VALUE_MULTIPLE: FiboConcept = "fibo-fbc-fct-ra:EnterpriseValueMultiple";

/// Superforecasting / Bayesian concepts.
pub const FORECAST_ID: FiboConcept = "fibo-fbc-fct-ra:ForecastIdentifier";
pub const BRIER_SCORE: FiboConcept = "fibo-fbc-fct-ra:BrierScore";
pub const SCENARIO_PROBABILITY: FiboConcept = "fibo-fbc-fct-ra:ScenarioProbability";

/// Screening / sensitivity / Monte Carlo concepts.
pub const SENSITIVITY_ANALYSIS: FiboConcept = "fibo-fbc-fct-ra:SensitivityAnalysis";
pub const MONTE_CARLO_DCF: FiboConcept = "fibo-fbc-fct-ra:MonteCarloDcf";
pub const PROBABILITY_OF_UNDERVALUATION: FiboConcept =
    "fibo-fbc-fct-ra:ProbabilityOfUndervaluation";
pub const STOCK_SCREENER: FiboConcept = "fibo-fbc-fct-ra:StockScreener";

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

/// Portfolio characteristics aggregation — FIBO-defined concepts that can be
/// aggregated across holdings as weighted averages.
pub const PORTFOLIO_AGGREGATABLE_FIELDS: &[(&str, FiboConcept)] = &[
    ("peRatio", PRICE_EARNINGS_RATIO),
    ("priceToBookRatio", PRICE_TO_BOOK_RATIO),
    ("priceToSalesRatio", PRICE_TO_SALES_RATIO),
    ("roic", RETURN_ON_INVESTED_CAPITAL),
    ("roe", RETURN_ON_EQUITY),
    ("grossProfitMargin", GROSS_PROFIT_MARGIN),
    ("operatingProfitMargin", OPERATING_PROFIT_MARGIN),
    ("netProfitMargin", NET_PROFIT_MARGIN),
    ("debtToEquity", DEBT_TO_EQUITY_RATIO),
    ("dividendYield", DIVIDEND_YIELD),
    ("revenueGrowth", REVENUE_GROWTH_RATE),
    ("epsGrowth", EPS_GROWTH_RATE),
];

/// Portfolio categorical breakdown fields — FIBO-defined concepts that are
/// aggregated as category → weight maps (not numeric weighted averages).
pub const PORTFOLIO_CATEGORICAL_FIELDS: &[(&str, FiboConcept)] = &[
    ("sector", INDUSTRY_SECTOR),
    ("industry", INDUSTRY_CLASSIFICATION),
    ("country", COUNTRY_OF_INCORPORATION),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmp_to_fibo_maps_all_aggregatable_fields() {
        for (field, _concept) in PORTFOLIO_AGGREGATABLE_FIELDS {
            assert!(
                fmp_field_to_fibo(field).is_some(),
                "aggregatable field '{field}' must have FIBO mapping"
            );
        }
    }

    #[test]
    fn fmp_to_fibo_maps_all_categorical_fields() {
        for (field, _concept) in PORTFOLIO_CATEGORICAL_FIELDS {
            assert!(
                fmp_field_to_fibo(field).is_some(),
                "categorical field '{field}' must have FIBO mapping"
            );
        }
    }

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
    fn fibo_dcf_concepts_exist() {
        // Verify all DCF-specific concepts are defined
        assert_eq!(EFFECTIVE_TAX_RATE, "fibo-fbc-fct-ra:EffectiveTaxRate");
        assert_eq!(DISCOUNT_RATE, "fibo-fbc-fct-ra:DiscountRate");
        assert_eq!(ENTERPRISE_VALUE, "fibo-fbc-fct-ra:EnterpriseValue");
        assert_eq!(EQUITY_VALUE, "fibo-fbc-fct-ra:EquityValue");
        assert_eq!(FREE_CASH_FLOW, "fibo-fbc-fct-ra:FreeCashFlow");
        assert_eq!(NET_DEBT, "fibo-fbc-fct-ra:NetDebt");
        assert_eq!(MARGIN_OF_SAFETY, "fibo-fbc-fct-ra:MarginOfSafety");
    }

    #[test]
    fn fibo_screening_concepts_exist() {
        assert_eq!(SENSITIVITY_ANALYSIS, "fibo-fbc-fct-ra:SensitivityAnalysis");
        assert_eq!(MONTE_CARLO_DCF, "fibo-fbc-fct-ra:MonteCarloDcf");
        assert_eq!(
            PROBABILITY_OF_UNDERVALUATION,
            "fibo-fbc-fct-ra:ProbabilityOfUndervaluation"
        );
        assert_eq!(STOCK_SCREENER, "fibo-fbc-fct-ra:StockScreener");
    }

    #[test]
    fn fibo_portfolio_concepts_exist() {
        assert_eq!(PORTFOLIO, "fibo-sec-sec-ast:Portfolio");
        assert_eq!(SECURITY_HOLDING, "fibo-sec-sec-ast:SecurityHolding");
        assert_eq!(HOLDING_WEIGHT, "fibo-sec-sec-ast:holdingWeight");
        assert_eq!(WEIGHTED_AVERAGE, "fibo-ind-ind-ind:WeightedAverage");
        assert_eq!(TRANSACTION_LEDGER, "fibo-sec-sec-ast:TransactionLedger");
        assert_eq!(BUY_TRANSACTION, "fibo-sec-sec-ast:BuyTransaction");
        assert_eq!(SELL_TRANSACTION, "fibo-sec-sec-ast:SellTransaction");
        assert_eq!(DIVIDEND_TRANSACTION, "fibo-sec-sec-ast:DividendTransaction");
        assert_eq!(DEPOSIT_TRANSACTION, "fibo-sec-sec-ast:DepositTransaction");
        assert_eq!(
            WITHDRAWAL_TRANSACTION,
            "fibo-sec-sec-ast:WithdrawalTransaction"
        );
        assert_eq!(ATTRIBUTION_ANALYSIS, "fibo-fbc-fct-ra:AttributionAnalysis");
        assert_eq!(TIME_WEIGHTED_RETURN, "fibo-fbc-fct-ra:TimeWeightedReturn");
        assert_eq!(
            INTERNAL_RATE_OF_RETURN,
            "fibo-fbc-fct-ra:InternalRateOfReturn"
        );
    }
}
