//! FIBO Financial Industry Business Ontology bridge.
//!
//! Canonical concept URIs for financial and business analysis — competitive
//! advantage, valuation, capital allocation, risk, and economic profit.
//! Used by docproc extract_triples for expository passages on finance,
//! investing, and business strategy.
//!
//! Reference: FIBO (Financial Industry Business Ontology), EDM Council
//! Canonical namespace: <https://spec.edmcouncil.org/fibo/>
//!
//! Pattern: thin mapping layer — canonical URI constants, no dependencies,
//! no reasoners, no overhead. Mirrors hkask-bridge-dublincore and
//! hkask-bridge-pko.

/// A FIBO concept URI.
pub type FiboConcept = &'static str;

// ── Competitive advantage ─────────────────────────────────────────────────

/// Competitive advantage or moat.
pub const COMPETITIVE_ADVANTAGE: FiboConcept = "fibo:hasCompetitiveAdvantage";
/// Barrier to entry.
pub const BARRIER_TO_ENTRY: FiboConcept = "fibo:hasBarrierToEntry";
/// Return on invested capital.
pub const RETURN_ON_CAPITAL: FiboConcept = "fibo:returnOnCapital";
/// Economic profit (ROIC minus cost of capital).
pub const ECONOMIC_PROFIT: FiboConcept = "fibo:economicProfit";

// ── Valuation ─────────────────────────────────────────────────────────────

/// Discounted cash flow valuation method.
pub const DCF_VALUATION: FiboConcept = "fibo:dcfValuation";
/// Intrinsic value of an asset.
pub const INTRINSIC_VALUE: FiboConcept = "fibo:intrinsicValue";
/// Margin of safety (price below intrinsic value).
pub const MARGIN_OF_SAFETY: FiboConcept = "fibo:marginOfSafety";
/// Cost of capital (WACC).
pub const COST_OF_CAPITAL: FiboConcept = "fibo:costOfCapital";

// ── Capital allocation ────────────────────────────────────────────────────

/// How capital is allocated across opportunities.
pub const CAPITAL_ALLOCATION: FiboConcept = "fibo:capitalAllocation";
/// Reinvestment rate.
pub const REINVESTMENT_RATE: FiboConcept = "fibo:reinvestmentRate";

// ── Risk and uncertainty ──────────────────────────────────────────────────

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
