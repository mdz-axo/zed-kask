//! FIBO (Financial Industry Business Ontology) vocabulary bridge.
//!
//! Canonical concept URIs for financial and business analysis — competitive
//! advantage, valuation, capital allocation, risk, economic profit, financial
//! ratios, DCF, and portfolio concepts. FIBO is the OMG standard for
//! financial data, built by Goldman Sachs, Citigroup, Bloomberg, the Fed,
//! and others. We anchor to FIBO rather than inventing our own taxonomy.
//!
//! This module is the single source of truth for FIBO concepts in hKask. It
//! unifies the financial-ratio/DCF concepts (formerly in the companies
//! server's local `fibo.rs`) with the competitive-advantage/valuation
//! concepts (formerly in the corpus server's local `bridge/fibo.rs`) so that
//! a document about ROIC tagged by the corpus and the same ROIC concept in
//! the companies server resolve to the same canonical URI.
//!
//! Reference: <https://spec.edmcouncil.org/fibo/>
//!
//! Key FIBO modules used:
//! - fibo-fbc-fct-ra  — Financial Concepts: Financial Ratios (Release)
//! - fibo-sec-sec-ast — Securities: Security Assets (Release)
//! - fibo-be-le-corp  — Business Entities: Corporations (Release)
//! - fibo-fnd-gao-gao — Foundations: Geographies (Release)
//! - fibo-ind-ind-ind — Indices and Indicators: Weighted Averages
//! - fibo-ind-ir-ir   — Indicators: Interest Rates
//! - fibo-ind-ei-ei   — Indicators: Economic Indicators

/// A FIBO concept URI — the canonical identifier for a financial data concept.
pub type FiboConcept = &'static str;

// ── Business entities ───────────────────────────────────────────────────

pub const CORPORATION: FiboConcept = "fibo-be-le-corp:Corporation";
pub const LEGAL_NAME: FiboConcept = "fibo-fnd-utl-alias:legalName";
pub const TICKER_SYMBOL: FiboConcept = "fibo-sec-sec-lst:tickerSymbol";
pub const COUNTRY_OF_INCORPORATION: FiboConcept = "fibo-fnd-arr-arr:CountryOfIncorporation";
pub const INDUSTRY_SECTOR: FiboConcept = "fibo-fnd-gao-gao:IndustrySectorClassification";
pub const INDUSTRY_CLASSIFICATION: FiboConcept = "fibo-fnd-gao-gao:IndustryClassification";

// ── Market data ──────────────────────────────────────────────────────────

pub const MARKET_CAPITALIZATION: FiboConcept = "fibo-fbc-fct-ra:MarketCapitalization";

// ── Valuation multiples ──────────────────────────────────────────────────

pub const PRICE_EARNINGS_RATIO: FiboConcept = "fibo-fbc-fct-ra:PriceEarningsRatio";
pub const PRICE_TO_BOOK_RATIO: FiboConcept = "fibo-fbc-fct-ra:PriceToBookRatio";
pub const PRICE_TO_SALES_RATIO: FiboConcept = "fibo-fbc-fct-ra:PriceToSalesRatio";

// ── Profitability ───────────────────────────────────────────────────────

pub const RETURN_ON_INVESTED_CAPITAL: FiboConcept = "fibo-fbc-fct-ra:ReturnOnInvestedCapital";
pub const RETURN_ON_EQUITY: FiboConcept = "fibo-fbc-fct-ra:ReturnOnEquity";
pub const RETURN_ON_ASSETS: FiboConcept = "fibo-fbc-fct-ra:ReturnOnAssets";
pub const GROSS_PROFIT_MARGIN: FiboConcept = "fibo-fbc-fct-ra:GrossProfitMargin";
pub const OPERATING_PROFIT_MARGIN: FiboConcept = "fibo-fbc-fct-ra:OperatingProfitMargin";
pub const NET_PROFIT_MARGIN: FiboConcept = "fibo-fbc-fct-ra:NetProfitMargin";

// ── Leverage ─────────────────────────────────────────────────────────────

pub const DEBT_TO_EQUITY_RATIO: FiboConcept = "fibo-fbc-fct-ra:DebtToEquityRatio";
pub const FINANCIAL_LEVERAGE_RATIO: FiboConcept = "fibo-fbc-fct-ra:FinancialLeverageRatio";
pub const TOTAL_ASSETS: FiboConcept = "fibo-fbc-pas-fpas:TotalAssets";
pub const TOTAL_EQUITY: FiboConcept = "fibo-fbc-pas-fpas:TotalEquity";
pub const TREASURY_STOCK: FiboConcept = "fibo-fbc-pas-fpas:TreasuryStock";

// ── Income / growth ──────────────────────────────────────────────────────

pub const DIVIDEND_YIELD: FiboConcept = "fibo-fbc-fct-ra:DividendYield";
pub const REVENUE_GROWTH_RATE: FiboConcept = "fibo-fbc-fct-ra:RevenueGrowthRate";
pub const EPS_GROWTH_RATE: FiboConcept = "fibo-fbc-fct-ra:EarningsPerShareGrowthRate";

// ── DCF valuation ────────────────────────────────────────────────────────

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

// ── Portfolio concepts ──────────────────────────────────────────────────

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

// ── Comparable company analysis ────────────────────────────────────────

pub const COMPARABLE_COMPANY_ANALYSIS: FiboConcept = "fibo-fbc-fct-ra:ComparableCompanyAnalysis";
pub const ENTERPRISE_VALUE_MULTIPLE: FiboConcept = "fibo-fbc-fct-ra:EnterpriseValueMultiple";

// ── Superforecasting / Bayesian concepts ────────────────────────────────

pub const FORECAST_ID: FiboConcept = "fibo-fbc-fct-ra:ForecastIdentifier";
pub const BRIER_SCORE: FiboConcept = "fibo-fbc-fct-ra:BrierScore";
pub const SCENARIO_PROBABILITY: FiboConcept = "fibo-fbc-fct-ra:ScenarioProbability";

// ── Screening / sensitivity / Monte Carlo concepts ─────────────────────

pub const SENSITIVITY_ANALYSIS: FiboConcept = "fibo-fbc-fct-ra:SensitivityAnalysis";
pub const MONTE_CARLO_DCF: FiboConcept = "fibo-fbc-fct-ra:MonteCarloDcf";
pub const PROBABILITY_OF_UNDERVALUATION: FiboConcept =
    "fibo-fbc-fct-ra:ProbabilityOfUndervaluation";
pub const STOCK_SCREENER: FiboConcept = "fibo-fbc-fct-ra:StockScreener";

// ── Competitive advantage (from the former corpus bridge) ───────────────
//
// These use the bare `fibo:` prefix for hKask-specific extensions where FIBO
// has no exact published class. This mirrors the corpus bridge convention
// and is documented as hKask-layer FIBO extension, not OMG-standard FIBO.

/// Competitive advantage or moat.
pub const COMPETITIVE_ADVANTAGE: FiboConcept = "fibo:hasCompetitiveAdvantage";
/// Barrier to entry.
pub const BARRIER_TO_ENTRY: FiboConcept = "fibo:hasBarrierToEntry";
/// Return on capital.
pub const RETURN_ON_CAPITAL: FiboConcept = "fibo:returnOnCapital";
/// Economic profit (ROIC minus cost of capital).
pub const ECONOMIC_PROFIT: FiboConcept = "fibo:economicProfit";
/// Discounted cash flow valuation method.
pub const DCF_VALUATION: FiboConcept = "fibo:dcfValuation";
/// Intrinsic value of an asset.
pub const INTRINSIC_VALUE: FiboConcept = "fibo:intrinsicValue";
/// Cost of capital (WACC).
pub const COST_OF_CAPITAL: FiboConcept = "fibo:costOfCapital";
/// How capital is allocated across opportunities.
pub const CAPITAL_ALLOCATION: FiboConcept = "fibo:capitalAllocation";
/// Reinvestment rate.
pub const REINVESTMENT_RATE: FiboConcept = "fibo:reinvestmentRate";
/// Risk profile or risk factor.
pub const HAS_RISK: FiboConcept = "fibo:hasRisk";
/// Uncertainty in estimates or forecasts.
pub const HAS_UNCERTAINTY: FiboConcept = "fibo:hasUncertainty";

/// All FIBO concepts, for validation or iteration.
pub const ALL_CONCEPTS: &[FiboConcept] = &[
    COMPETITIVE_ADVANTAGE,
    BARRIER_TO_ENTRY,
    RETURN_ON_CAPITAL,
    ECONOMIC_PROFIT,
    DCF_VALUATION,
    INTRINSIC_VALUE,
    MARGIN_OF_SAFETY,
    COST_OF_CAPITAL,
    CAPITAL_ALLOCATION,
    REINVESTMENT_RATE,
    HAS_RISK,
    HAS_UNCERTAINTY,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fibo_dcf_concepts_exist() {
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

    #[test]
    fn fibo_competitive_advantage_concepts_exist() {
        assert_eq!(COMPETITIVE_ADVANTAGE, "fibo:hasCompetitiveAdvantage");
        assert_eq!(BARRIER_TO_ENTRY, "fibo:hasBarrierToEntry");
        assert_eq!(RETURN_ON_CAPITAL, "fibo:returnOnCapital");
        assert_eq!(ECONOMIC_PROFIT, "fibo:economicProfit");
        assert_eq!(DCF_VALUATION, "fibo:dcfValuation");
        assert_eq!(INTRINSIC_VALUE, "fibo:intrinsicValue");
        assert_eq!(COST_OF_CAPITAL, "fibo:costOfCapital");
    }

    #[test]
    fn all_concepts_list_covers_competitive_advantage_subset() {
        // The ALL_CONCEPTS list (carried from the former corpus bridge) must
        // cover the competitive-advantage subset so corpus validation still
        // works after the merge.
        for concept in [
            COMPETITIVE_ADVANTAGE,
            BARRIER_TO_ENTRY,
            RETURN_ON_CAPITAL,
            ECONOMIC_PROFIT,
            DCF_VALUATION,
            INTRINSIC_VALUE,
            MARGIN_OF_SAFETY,
            COST_OF_CAPITAL,
            CAPITAL_ALLOCATION,
            REINVESTMENT_RATE,
            HAS_RISK,
            HAS_UNCERTAINTY,
        ] {
            assert!(
                ALL_CONCEPTS.contains(&concept),
                "ALL_CONCEPTS must cover {concept}"
            );
        }
    }
}
