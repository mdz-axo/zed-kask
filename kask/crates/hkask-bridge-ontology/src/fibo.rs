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

// ── Economic indicators (FIBO ind-ir / ind-ei modules) ─────────────────────
//
// These are the FIBO concepts for the economic factors the CMP base events
// track: policy interest rates, Treasury yields, inflation, commodity
// prices, and crypto asset prices. They anchor the semantic mapping from
// prediction-market contracts to FIBO concepts so the dual-axis graph
// proximity can identify constellations of related events.

/// A central bank's short-term policy interest rate (Fed funds, ECB refi,
/// BoE bank rate). FIBO: `fibo-ind-ir-ir:PolicyInterestRate`.
pub const POLICY_INTEREST_RATE: FiboConcept = "fibo-ind-ir-ir:PolicyInterestRate";
/// A Treasury yield at a specific maturity (2Y, 5Y, 10Y, 30Y). FIBO:
/// `fibo-ind-ir-ir:YieldCurvePoint` (hKask extension — FIBO models the yield
/// curve as a whole; this is the per-point concept).
pub const TREASURY_YIELD: FiboConcept = "fibo-ind-ir-ir:YieldCurvePoint";
/// The Fed funds target rate specifically. FIBO:
/// `fibo-ind-ir-ir:FederalFundsRate`.
pub const FEDERAL_FUNDS_RATE: FiboConcept = "fibo-ind-ir-ir:FederalFundsRate";
/// A consumer price index (CPI) — the headline inflation measure. FIBO:
/// `fibo-ind-ei-ei:ConsumerPriceIndex`.
pub const CONSUMER_PRICE_INDEX: FiboConcept = "fibo-ind-ei-ei:ConsumerPriceIndex";
/// A producer price index (PPI). FIBO: `fibo-ind-ei-ei:ProducerPriceIndex`.
pub const PRODUCER_PRICE_INDEX: FiboConcept = "fibo-ind-ei-ei:ProducerPriceIndex";
/// A commodity price index (WTI, Brent, Henry Hub). FIBO:
/// `fibo-ind-ei-ei:CommodityPriceIndex`.
pub const COMMODITY_PRICE_INDEX: FiboConcept = "fibo-ind-ei-ei:CommodityPriceIndex";
/// A market index (Bitcoin, Ethereum). FIBO: `fibo-ind-ei-ei:MarketIndex`.
pub const MARKET_INDEX: FiboConcept = "fibo-ind-ei-ei:MarketIndex";
/// Gross domestic product. FIBO: `fibo-ind-ei-ei:GrossDomesticProduct`.
pub const GROSS_DOMESTIC_PRODUCT: FiboConcept = "fibo-ind-ei-ei:GrossDomesticProduct";

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
