//! Economic Profit valuation module — Residual Income Model (RIM).
//!
//! Implements the intrinsic value framework from Bergen, Franzoni, Obrycki,
//! and Resendes (2025, Financial Analysts Journal): "Intrinsic Value: A Solution
//! to the Declining Performance of Value Strategies."
//!
//! Core insight: Intrinsic Value = Book Value + PV(Future Economic Profits).
//! This decomposition separates assets-in-place from competitive advantage,
//! addressing why traditional multiples (P/B, P/E) stopped working as discount
//! rates fell and profit dispersion widened.
//!
//! ## Model
//!
//! ```text
//! IV = BV + Σ_{t=1}^{T} EP_t / (1+r)^t
//!
//! where:  EP_t = (ROIC - WACC) × Invested Capital_t
//!         BV  = total stockholders' equity (latest fiscal year)
//!         r   = discount rate (WACC)
//!         T   = competitive fade horizon
//! ```
//!
//! Competitive fade (Bergen §2): economic profits are not perpetual — they
//! decay to zero as competitors enter. The fade horizon depends on the
//! company's competitive moat:
//!
//! | Moat       | Fade Horizon | Rationale                                |
//! |------------|-------------|------------------------------------------|
//! | Wide       | 20 years    | Strong, durable competitive advantage     |
//! | Narrow     | 10 years    | Defensible but erodable advantage         |
//! | None       | 5 years     | Commodity or highly competitive industry  |
//! | Unknown    | 10 years    | Conservative default                      |
//!
//! ## Decomposition
//!
//! Each valuation output includes:
//! - `pct_from_book_value`: % of IV from assets already on the balance sheet
//! - `pct_from_economic_profits`: % of IV from future competitive advantage
//! - `ivm_ratio`: IV / Market Cap (the paper's key screening metric)
//!
//! When `pct_from_economic_profits` is high (>60%), the valuation is sensitive
//! to assumptions about competitive advantage duration. When low, the company
//! is mostly valued on its existing assets.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ── Competitive fade ──────────────────────────────────────────────────────────

/// Competitive advantage duration, controlling how fast economic profits fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum FadeHorizon {
    /// 20-year fade — durable competitive advantage.
    Wide,
    /// 10-year fade — defensible but erodable advantage.
    Narrow,
    /// 5-year fade — no durable advantage, rapid erosion.
    None,
    /// 10-year fade — conservative default when moat is unknown.
    Default,
}

impl FadeHorizon {
    /// Years until economic profits reach ~zero.
    pub fn years(self) -> u8 {
        match self {
            FadeHorizon::Wide => 20,
            FadeHorizon::Narrow => 10,
            FadeHorizon::None => 5,
            FadeHorizon::Default => 10,
        }
    }
}

// ── Economic profit computation ───────────────────────────────────────────────

/// One period of projected economic profit.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct EpPeriod {
    /// Period number (1-based).
    pub period: usize,
    /// Invested capital at start of period.
    pub invested_capital: f64,
    /// Return on invested capital (ROIC).
    pub roic: f64,
    /// Weighted average cost of capital.
    pub wacc: f64,
    /// Economic profit: (ROIC - WACC) × Invested Capital.
    pub economic_profit: f64,
    /// Discount factor for this period.
    pub discount_factor: f64,
    /// Present value of this period's economic profit.
    pub present_value: f64,
}

/// Result of economic profit valuation.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct EpValuation {
    /// Book value of equity (latest fiscal year).
    pub book_value: f64,
    /// Discount rate (WACC).
    pub wacc: f64,
    /// Current ROIC from historical data.
    pub current_roic: f64,
    /// ROIC-WACC spread — positive = value creation, negative = value destruction.
    pub roic_wacc_spread: f64,
    /// Invested capital at latest fiscal year.
    pub invested_capital: f64,
    /// IC growth rate used in stage 1.
    pub ic_growth_rate: f64,
    /// Base fade horizon before decay adjustment.
    pub base_fade_years: u8,
    /// Fade horizon used.
    pub fade_horizon: FadeHorizon,
    /// Fade horizon in years.
    pub fade_years: u8,
    /// Stage 1 years (growth phase, EP held constant).
    pub stage1_years: u8,
    /// Projected economic profits by period.
    pub periods: Vec<EpPeriod>,
    /// Present value of all future economic profits.
    pub pv_economic_profits: f64,
    /// Total intrinsic value: BV + PV(EP).
    pub intrinsic_value: f64,
    /// Intrinsic value per share.
    pub intrinsic_per_share: f64,
    /// Current stock price.
    pub current_price: f64,
    /// Market capitalisation.
    pub market_cap: f64,
    /// IVM ratio: intrinsic value / market cap.
    pub ivm_ratio: f64,
    /// Margin of safety: (IV - price) / price.
    pub margin_of_safety: f64,
    /// % of intrinsic value from book value.
    pub pct_from_book_value: f64,
    /// % of intrinsic value from PV of future economic profits.
    pub pct_from_economic_profits: f64,
    /// Equity duration in years: PV-weighted average time of the economic-
    /// profit stream (Macaulay-style over the EP periods). None when PV(EP)
    /// is zero or negative — a duration over a non-positive stream is not
    /// meaningful, and None is never a fabricated number.
    pub equity_duration_years: Option<f64>,
    /// Interpretation signal.
    pub signal: EpSignal,
}

/// Interpretation of the economic profit valuation.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct EpSignal {
    /// "undervalued", "fairly_valued", "overvalued"
    pub valuation: &'static str,
    /// "value_creator", "value_neutral", "value_destroyer"
    pub profitability: &'static str,
    /// "asset_heavy", "balanced", "growth_dependent"
    pub composition: &'static str,
    /// Human-readable summary.
    pub summary: String,
}

// ── Valuation engine ──────────────────────────────────────────────────────────

/// Compute residual income valuation with competitive fade.
///
/// # Parameters
/// - `latest_book_value`: total stockholders' equity from latest fiscal year.
/// - `latest_roic`: return on invested capital (NOPAT / invested capital) from
///   latest fiscal year.
/// - `latest_invested_capital`: total capital employed (debt + equity, or total
///   assets for simplified computation).
/// - `wacc`: weighted average cost of capital (discount rate).
/// - `shares_outstanding`: shares used for per-share computation.
/// - `current_price`: current stock price for IVM ratio.
/// - `fade_horizon`: competitive advantage duration.
/// - `stage1_years`: years to hold EP constant before fade begins.
///
///   AFG four value drivers (Obrycki & Resendes, 2000): Profitability = (ROIC - WACC) × Invested Capital, Competition = decay rate → 0, Growth = invested capital growth, Cost of capital = WACC.
pub(crate) fn value_economic_profit(
    latest_book_value: f64,
    latest_roic: f64,
    latest_invested_capital: f64,
    wacc: f64,
    shares_outstanding: f64,
    current_price: f64,
    fade_horizon: FadeHorizon,
    stage1_years: u8,
    ic_growth_rate: f64,
    roic_trend: f64,
    roic_variability: f64,
) -> EpValuation {
    // Adjust fade for empirical decay factors
    let fade_years = adjust_fade_for_decay_factors(
        fade_horizon.years(),
        latest_roic,
        wacc,
        roic_variability,
        roic_trend,
        latest_invested_capital,
    );

    let total_years = stage1_years + fade_years;
    let roic_wacc_spread = latest_roic - wacc;
    let ic_growth = ic_growth_rate.clamp(-0.20, 0.30);
    let _current_ep = roic_wacc_spread * latest_invested_capital;

    let mut periods = Vec::with_capacity(total_years as usize);
    let mut ic = latest_invested_capital;

    // Stage 1: EP constant, IC grows
    for p in 0..stage1_years {
        let ep = roic_wacc_spread * ic;
        let df = 1.0 / (1.0 + wacc).powi((p + 1) as i32);
        periods.push(EpPeriod {
            period: (p + 1) as usize,
            invested_capital: ic,
            roic: latest_roic,
            wacc,
            economic_profit: ep,
            discount_factor: df,
            present_value: ep * df,
        });
        ic *= 1.0 + ic_growth;
    }

    // Stage 2: competitive fade, IC held constant at end-of-stage-1 level.
    // Uses the IC grown during Stage 1 (not the initial IC) so the fade
    // starts from the correct economic profit baseline.
    let fade_start_ic = ic;
    let fade_start_ep = roic_wacc_spread * fade_start_ic;
    for p in 0..fade_years {
        let decay_pct = if fade_years > 1 {
            (fade_years - p - 1) as f64 / (fade_years - 1) as f64
        } else {
            0.0
        };
        let ep = fade_start_ep * decay_pct;
        let year = (stage1_years + p + 1) as usize;
        let df = 1.0 / (1.0 + wacc).powi(year as i32);
        let faded_roic = if latest_roic.abs() > 1e-10 {
            latest_roic * decay_pct.max(0.0)
        } else {
            0.0
        };
        periods.push(EpPeriod {
            period: year,
            invested_capital: fade_start_ic,
            roic: faded_roic,
            wacc,
            economic_profit: ep,
            discount_factor: df,
            present_value: ep * df,
        });
    }

    let pv_economic_profits: f64 = periods.iter().map(|p| p.present_value).sum();
    let intrinsic_value = latest_book_value + pv_economic_profits;
    let intrinsic_per_share = if shares_outstanding > 0.0 {
        intrinsic_value / shares_outstanding
    } else {
        0.0
    };
    let market_cap = current_price * shares_outstanding;
    let ivm_ratio = if market_cap > 0.0 {
        intrinsic_value / market_cap
    } else {
        1.0
    };
    let margin_of_safety = if current_price > 0.0 {
        (intrinsic_per_share - current_price) / current_price
    } else {
        0.0
    };
    let pct_from_book_value = if intrinsic_value > 0.0 {
        latest_book_value / intrinsic_value
    } else {
        1.0
    };
    let pct_from_economic_profits = if intrinsic_value > 0.0 {
        pv_economic_profits / intrinsic_value
    } else {
        0.0
    };

    let signal = classify_signal(ivm_ratio, roic_wacc_spread, pct_from_economic_profits);
    let equity_duration_years = equity_duration_years(&periods);

    EpValuation {
        book_value: latest_book_value,
        wacc,
        current_roic: latest_roic,
        roic_wacc_spread,
        invested_capital: latest_invested_capital,
        ic_growth_rate,
        base_fade_years: fade_horizon.years(),
        fade_horizon,
        fade_years,
        stage1_years,
        periods,
        pv_economic_profits,
        intrinsic_value,
        intrinsic_per_share,
        current_price,
        market_cap,
        ivm_ratio,
        margin_of_safety,
        pct_from_book_value,
        pct_from_economic_profits,
        equity_duration_years,
        signal,
    }
}

/// Macaulay-style equity duration over the EP stream: the PV-weighted
/// average period in which economic-profit value is received.
///
/// D = Σ_t t·PV(EP_t) / Σ_t PV(EP_t)
///
/// Only the EP stream is timed (book value is a stock, not a flow, so it
/// has no time coordinate). Returns None when total PV(EP) ≤ 0 — for a
/// value-destroying company the weighting is not a duration.
pub fn equity_duration_years(periods: &[EpPeriod]) -> Option<f64> {
    let total_pv: f64 = periods.iter().map(|p| p.present_value).sum();
    if total_pv <= 0.0 {
        return None;
    }
    let weighted: f64 = periods
        .iter()
        .map(|p| p.period as f64 * p.present_value)
        .sum();
    Some(weighted / total_pv)
}

/// Adjust fade horizon for empirical decay factors (AFG, Obrycki & Resendes 2000).
///
/// Grounded in Greenwald & Kahn (2005) "Competition Demystified": competitive
/// advantages are barriers to entry. The decay rate models the erosion of those
/// barriers over the Competitive Advantage Period (CAP).
///
/// Decay increases (fade shortens) when:
/// 1. EM spread is extreme (|ROIC - WACC| > 8%) — high profits attract entrants
/// 2. ROIC variability is high (CV > 0.3) — unstable advantages are less durable
/// 3. ROIC trend is declining (< -1%) — eroding barriers
/// 4. Firm size is small (IC < $1B) — fewer scale-based barriers
fn adjust_fade_for_decay_factors(
    base_years: u8,
    roic: f64,
    wacc: f64,
    roic_variability: f64,
    roic_trend: f64,
    invested_capital: f64,
) -> u8 {
    let mut years = base_years as f64;
    let spread = (roic - wacc).abs();

    if spread > 0.15 {
        years *= 0.7;
    } else if spread > 0.08 {
        years *= 0.85;
    }
    if roic_variability > 0.5 {
        years *= 0.7;
    } else if roic_variability > 0.3 {
        years *= 0.85;
    }
    if roic_trend < -0.03 {
        years *= 0.75;
    } else if roic_trend < -0.01 {
        years *= 0.9;
    } else if roic_trend > 0.03 {
        years *= 1.1;
    }
    if invested_capital < 500_000_000.0 {
        years *= 0.7;
    } else if invested_capital < 1_000_000_000.0 {
        years *= 0.85;
    } else if invested_capital > 50_000_000_000.0 {
        years *= 1.1;
    }

    (years.round() as u8).clamp(3, 25)
}

// ── Signal classification ─────────────────────────────────────────────────────

fn classify_signal(ivm: f64, spread: f64, pct_ep: f64) -> EpSignal {
    let valuation = if ivm > 1.2 {
        "undervalued"
    } else if ivm > 0.9 {
        "fairly_valued"
    } else {
        "overvalued"
    };

    let profitability = if spread > 0.03 {
        "value_creator"
    } else if spread > -0.03 {
        "value_neutral"
    } else {
        "value_destroyer"
    };

    let composition = if pct_ep < 0.2 {
        "asset_heavy"
    } else if pct_ep < 0.6 {
        "balanced"
    } else {
        "growth_dependent"
    };

    let summary = format!(
        "{valuation} ({profitability}) — {:.0}% of value from future economic profits. {}",
        pct_ep * 100.0,
        match composition {
            "asset_heavy" =>
                "Valuation anchored to tangible assets; low sensitivity to growth assumptions.",
            "balanced" => "Mix of assets-in-place and competitive advantage.",
            "growth_dependent" =>
                "Most value depends on sustaining competitive advantage. Sensitive to moat durability assumptions.",
            _ => "",
        }
    );

    EpSignal {
        valuation,
        profitability,
        composition,
        summary,
    }
}

// ── ROIC computation helpers ──────────────────────────────────────────────────

/// Compute ROIC from income statement and balance sheet data.
/// ROIC = NOPAT / Invested Capital
///   NOPAT = EBIT × (1 - tax_rate)
///   Invested Capital = Total Assets - Non-Interest-Bearing Current Liabilities
///   (simplified: total assets, as we don't have detailed liability breakdown)
pub(crate) fn compute_roic(ebit: f64, tax_rate: f64, invested_capital: f64) -> Option<f64> {
    if invested_capital <= 0.0 {
        return None;
    }
    let nopat = ebit * (1.0 - tax_rate);
    Some(nopat / invested_capital)
}

/// Extract EBIT from income statement data.
/// Prefer explicit EBIT field, fall back to: grossProfit - depreciationAndAmortization.
pub(crate) fn extract_ebit(income_entry: &serde_json::Value) -> Option<f64> {
    // Prefer direct EBIT field
    if let Some(ebit) = income_entry
        .get("ebit")
        .or_else(|| income_entry.get("ebitda"))
        .and_then(|v| v.as_f64())
    {
        return Some(ebit);
    }

    // Fall back to: grossProfit - depreciationAndAmortization
    let gp = income_entry.get("grossProfit").and_then(|v| v.as_f64());
    let da = income_entry
        .get("depreciationAndAmortization")
        .and_then(|v| v.as_f64());
    match (gp, da) {
        (Some(g), Some(d)) => Some(g - d),
        _ => None,
    }
}

/// Extract invested capital from balance sheet data.
/// Invested Capital = Total Assets - Current Liabilities + Short-term Debt
/// Simplified: Total Assets (proxy when detailed breakdown unavailable).
pub(crate) fn extract_invested_capital(balance_entry: &serde_json::Value) -> Option<f64> {
    balance_entry.get("totalAssets").and_then(|v| v.as_f64())
}

/// Extract book value of equity from balance sheet.
pub(crate) fn extract_book_value(balance_entry: &serde_json::Value) -> Option<f64> {
    balance_entry
        .get("totalStockholdersEquity")
        .or_else(|| balance_entry.get("totalEquity"))
        .and_then(|v| v.as_f64())
}

// ── Treasury stock adjustment (hKask non-standard treatment) ──────────────────
//
// hKask treats Treasury Stock as committed capital rather than a reduction
// in equity. The adjustment adds 2× |treasury stock| to both Owner's Equity
// and Intangible Assets, preserving the balance sheet identity:
//
//   (Intangible Assets + 2×TS) + Other Assets = Liabilities + (Equity + 2×TS)
//
// Rationale: treasury stock represents capital returned to shareholders
// that remains available for redeployment — it should be treated as
// committed capital, not a reduction. The corresponding increase in
// intangible assets reflects that buybacks often build organizational
// capital (leaner operations, higher per-share metrics) that GAAP
// does not capitalise.

/// Extract raw treasury stock from balance sheet (typically negative in FMP/EODHD).
pub(crate) fn extract_treasury_stock(balance_entry: &serde_json::Value) -> f64 {
    balance_entry
        .get("treasuryStock")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .abs()
}

/// Adjusted Book Value: raw BV + 2 × |treasury stock|.
pub(crate) fn adj_book_value(balance_entry: &serde_json::Value) -> Option<f64> {
    let raw_bv = extract_book_value(balance_entry)?;
    let ts = extract_treasury_stock(balance_entry);
    Some(raw_bv + 2.0 * ts)
}

/// Adjusted Invested Capital: raw IC + 2 × |treasury stock|.
pub(crate) fn adj_invested_capital(balance_entry: &serde_json::Value) -> Option<f64> {
    let raw_ic = extract_invested_capital(balance_entry)?;
    let ts = extract_treasury_stock(balance_entry);
    Some(raw_ic + 2.0 * ts)
}

// ── ROIC from key_metrics (pre-computed) ──────────────────────────────────────

/// Extract ROIC from key_metrics data (pre-computed by FMP/EODHD).
/// Checks `roic` (legacy alias added by `enrich_key_metrics`) and
/// `returnOnInvestedCapital` (FMP stable field name).
pub(crate) fn extract_roic_from_metrics(metrics_entry: &serde_json::Value) -> Option<f64> {
    metrics_entry
        .get("roic")
        .or_else(|| metrics_entry.get("returnOnInvestedCapital"))
        .and_then(|v| v.as_f64())
}

/// Extract invested capital from key_metrics data.
pub(crate) fn extract_invested_capital_from_metrics(
    metrics_entry: &serde_json::Value,
) -> Option<f64> {
    metrics_entry
        .get("investedCapital")
        .or_else(|| metrics_entry.get("totalAssets"))
        .and_then(|v| v.as_f64())
}

// ── Equity-based extraction helpers (financial-sector firms) ───────────────────
//
// For financial-sector companies (banks, insurance, investment firms),
// the ROIC/WACC/invested-capital framework breaks down because debt is
// raw material, not a source of capital. Damodaran (Applied Corporate
// Finance, Ch. 4; Investment Valuation, Ch. 21) argues for valuing equity
// directly using:
//
//   Excess Equity Return = (ROE - Cost of Equity) x Book Value of Equity
//   Value of Equity = BV Equity + PV(Excess Equity Returns)
//
// Source: Damodaran, A. (2014). Applied Corporate Finance (4th ed.),
// Ch. 4: "Equity, Debt and Cost of Capital for Banks" -- "For banks,
// debt is raw material that is used to generate profits... when banks
// talk about capital, they mean equity capital."
//
// Source: Damodaran, A. (2002). Investment Valuation (2nd ed.),
// Ch. 21: "Valuing Financial Service Firms" -- "Given the difficulty
// associated with defining total capital in a financial service firm,
// it makes far more sense to focus on just equity when using an excess
// return model to value a financial service firm."

/// Extract ROE from key_metrics data (pre-computed by FMP).
/// FMP field: `returnOnEquity`.
pub(crate) fn extract_roe_from_metrics(metrics_entry: &serde_json::Value) -> Option<f64> {
    metrics_entry.get("returnOnEquity").and_then(|v| v.as_f64())
}

/// Compute ROE from net income and book value of equity.
/// ROE = Net Income / Book Value of Equity (beginning of year).
pub(crate) fn compute_roe(net_income: f64, book_value: f64) -> Option<f64> {
    if book_value <= 0.0 {
        return None;
    }
    Some(net_income / book_value)
}

/// Extract net income from income statement data.
pub(crate) fn extract_net_income(income_entry: &serde_json::Value) -> Option<f64> {
    income_entry
        .get("netIncome")
        .or_else(|| income_entry.get("netIncomeCommonStockholders"))
        .and_then(|v| v.as_f64())
}

/// Compute cost of equity using CAPM.
/// COE = risk_free_rate + beta x equity_risk_premium.
/// Defaults: risk_free_rate = 4.25% (10Y Treasury),
///           equity_risk_premium = 4.5% (Damodaran implied ERP for US).
/// Source: Damodaran, A. (2024). "Equity Risk Premiums: Determinants,
/// Estimation and Implications" -- implied ERP for US market.
pub(crate) fn cost_of_equity(
    beta: f64,
    risk_free_rate: Option<f64>,
    equity_risk_premium: Option<f64>,
) -> f64 {
    let rf = risk_free_rate.unwrap_or(0.0425);
    let erp = equity_risk_premium.unwrap_or(0.045);
    rf + beta * erp
}

/// Extract beta from company profile data.
pub(crate) fn extract_beta(profile_entry: &serde_json::Value) -> Option<f64> {
    profile_entry.get("beta").and_then(|v| v.as_f64())
}
