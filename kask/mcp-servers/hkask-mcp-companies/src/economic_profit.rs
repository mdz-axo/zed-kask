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
pub enum FadeHorizon {
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

    /// Map from MAIA moat classification string.
    pub fn from_moat(moat: &str) -> Self {
        match moat {
            "wide" => FadeHorizon::Wide,
            "narrow" => FadeHorizon::Narrow,
            "none" => FadeHorizon::None,
            _ => FadeHorizon::Default,
        }
    }

    /// Override from user input: "wide", "narrow", "none", "default".
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "wide" => Some(FadeHorizon::Wide),
            "narrow" => Some(FadeHorizon::Narrow),
            "none" => Some(FadeHorizon::None),
            "default" => Some(FadeHorizon::Default),
            _ => None,
        }
    }
}

// ── Economic profit computation ───────────────────────────────────────────────

/// One period of projected economic profit.
#[derive(Debug, Clone, Serialize)]
pub struct EpPeriod {
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
pub struct EpValuation {
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
    /// Interpretation signal.
    pub signal: EpSignal,
}

/// Interpretation of the economic profit valuation.
#[derive(Debug, Clone, Serialize)]
pub struct EpSignal {
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
#[allow(clippy::too_many_arguments)]
pub fn value_economic_profit(
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
        signal,
    }
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
pub fn compute_roic(ebit: f64, tax_rate: f64, invested_capital: f64) -> Option<f64> {
    if invested_capital <= 0.0 {
        return None;
    }
    let nopat = ebit * (1.0 - tax_rate);
    Some(nopat / invested_capital)
}

/// Extract EBIT from income statement data.
/// Prefer explicit EBIT field, fall back to: grossProfit - depreciationAndAmortization.
pub fn extract_ebit(income_entry: &serde_json::Value) -> Option<f64> {
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
pub fn extract_invested_capital(balance_entry: &serde_json::Value) -> Option<f64> {
    balance_entry.get("totalAssets").and_then(|v| v.as_f64())
}

/// Extract book value of equity from balance sheet.
pub fn extract_book_value(balance_entry: &serde_json::Value) -> Option<f64> {
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
pub fn extract_treasury_stock(balance_entry: &serde_json::Value) -> f64 {
    balance_entry
        .get("treasuryStock")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
        .abs()
}

/// Adjusted Book Value: raw BV + 2 × |treasury stock|.
pub fn adj_book_value(balance_entry: &serde_json::Value) -> Option<f64> {
    let raw_bv = extract_book_value(balance_entry)?;
    let ts = extract_treasury_stock(balance_entry);
    Some(raw_bv + 2.0 * ts)
}

/// Adjusted Invested Capital: raw IC + 2 × |treasury stock|.
pub fn adj_invested_capital(balance_entry: &serde_json::Value) -> Option<f64> {
    let raw_ic = extract_invested_capital(balance_entry)?;
    let ts = extract_treasury_stock(balance_entry);
    Some(raw_ic + 2.0 * ts)
}

/// Adjusted Owner's Equity: raw equity + 2 × |treasury stock|.
pub fn adj_equity(balance_entry: &serde_json::Value) -> Option<f64> {
    let raw_eq = balance_entry
        .get("totalStockholdersEquity")
        .or_else(|| balance_entry.get("totalEquity"))
        .and_then(|v| v.as_f64())?;
    let ts = extract_treasury_stock(balance_entry);
    Some(raw_eq + 2.0 * ts)
}

/// Adjusted Total Assets: raw total assets + 2 × |treasury stock|.
pub fn adj_total_assets(balance_entry: &serde_json::Value) -> Option<f64> {
    let raw_ta = extract_invested_capital(balance_entry)?;
    let ts = extract_treasury_stock(balance_entry);
    Some(raw_ta + 2.0 * ts)
}

// ── ROIC from key_metrics (pre-computed) ──────────────────────────────────────

/// Extract ROIC from key_metrics data (pre-computed by FMP/EODHD).
pub fn extract_roic_from_metrics(metrics_entry: &serde_json::Value) -> Option<f64> {
    metrics_entry.get("roic").and_then(|v| v.as_f64())
}

/// Extract invested capital from key_metrics data.
pub fn extract_invested_capital_from_metrics(metrics_entry: &serde_json::Value) -> Option<f64> {
    metrics_entry
        .get("investedCapital")
        .or_else(|| metrics_entry.get("totalAssets"))
        .and_then(|v| v.as_f64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_horizon_years() {
        assert_eq!(FadeHorizon::Wide.years(), 20);
        assert_eq!(FadeHorizon::Narrow.years(), 10);
        assert_eq!(FadeHorizon::None.years(), 5);
        assert_eq!(FadeHorizon::Default.years(), 10);
    }

    #[test]
    fn fade_horizon_from_moat() {
        assert_eq!(FadeHorizon::from_moat("wide"), FadeHorizon::Wide);
        assert_eq!(FadeHorizon::from_moat("narrow"), FadeHorizon::Narrow);
        assert_eq!(FadeHorizon::from_moat("none"), FadeHorizon::None);
        assert_eq!(
            FadeHorizon::from_moat("insufficient_data"),
            FadeHorizon::Default
        );
        assert_eq!(FadeHorizon::from_moat("unknown"), FadeHorizon::Default);
    }

    #[test]
    fn roic_computation() {
        // EBIT = 100, tax_rate = 0.21, IC = 1000
        // NOPAT = 100 × 0.79 = 79, ROIC = 79/1000 = 0.079
        let roic = compute_roic(100.0, 0.21, 1000.0);
        assert!(roic.is_some());
        assert!((roic.unwrap() - 0.079).abs() < 0.001);
    }

    #[test]
    fn roic_zero_ic_returns_none() {
        assert!(compute_roic(100.0, 0.21, 0.0).is_none());
    }

    #[test]
    fn ep_valuation_value_creator() {
        // Company with ROIC > WACC: creates value
        let result = value_economic_profit(
            10_000_000_000.0, // BV ($10B)
            0.15,             // ROIC 15%
            10_000_000_000.0, // IC ($10B, neutral size — no decay adjustment)
            0.10,             // WACC 10%
            1_000_000_000.0,  // shares (1B)
            12.0,             // price
            FadeHorizon::Narrow,
            3,   // stage1_years
            0.0, // ic_growth
            0.0, // roic_trend
            0.1, // roic_variability (low)
        );

        // EP = (0.15 - 0.10) × 50000 = 2500
        assert!((result.roic_wacc_spread - 0.05).abs() < 0.001);
        assert!(result.current_roic > result.wacc);
        assert_eq!(result.fade_years, 10);
        assert_eq!(result.stage1_years, 3);
        assert_eq!(result.periods.len(), 13);

        // IV should be > BV since EP is positive
        assert!(result.intrinsic_value > result.book_value);
        assert!(result.pv_economic_profits > 0.0);
        assert!(result.pct_from_economic_profits > 0.0);

        // IV per share should be reasonable
        assert!(result.intrinsic_per_share > 0.0);

        // Margin of safety
        assert!((result.margin_of_safety + 1.0).abs() > 0.0);

        // IVM ratio
        assert!(result.ivm_ratio > 0.0);

        // Signal
        assert_eq!(result.signal.profitability, "value_creator");
    }

    #[test]
    fn ep_valuation_value_destroyer() {
        // Company with ROIC < WACC: destroys value
        let result = value_economic_profit(
            10_000_000_000.0, // BV
            0.05,             // ROIC 5%
            10_000_000_000.0, // IC
            0.10,             // WACC
            1_000_000_000.0,
            12.0,
            FadeHorizon::None,
            2,
            0.0,
            0.0,
            0.1,
        );

        // EP is negative, so PV(EP) is negative
        assert!(result.pv_economic_profits < 0.0);
        // IV < BV because future EP is negative
        assert!(result.intrinsic_value < result.book_value);
        // pct_from_book_value > 1.0 because EP subtracts from BV
        assert!(result.pct_from_book_value > 1.0);
        assert_eq!(result.signal.profitability, "value_destroyer");
    }

    #[test]
    fn ep_valuation_no_moat_fades_quickly() {
        let wide = value_economic_profit(
            10_000_000_000.0,
            0.15,
            10_000_000_000.0,
            0.10,
            1_000_000_000.0,
            12.0,
            FadeHorizon::Wide,
            3,
            0.0,
            0.0,
            0.1,
        );
        let none = value_economic_profit(
            10_000_000_000.0,
            0.15,
            10_000_000_000.0,
            0.10,
            1_000_000_000.0,
            12.0,
            FadeHorizon::None,
            3,
            0.0,
            0.0,
            0.1,
        );

        // Wide moat should produce higher IV (more years of EP)
        assert!(wide.intrinsic_value > none.intrinsic_value);
        assert!(wide.periods.len() > none.periods.len());
    }

    #[test]
    fn ep_decays_to_zero() {
        let result = value_economic_profit(
            10_000_000_000.0,
            0.15,
            10_000_000_000.0,
            0.10,
            1_000_000_000.0,
            12.0,
            FadeHorizon::Narrow,
            3,
            0.0,
            0.0,
            0.1,
        );

        // Last period's EP should be near zero
        let last = result.periods.last().unwrap();
        assert!(
            last.economic_profit.abs() < 1.0,
            "last EP should be near zero: {}",
            last.economic_profit
        );
    }

    #[test]
    fn stage1_holds_ep_constant() {
        let result = value_economic_profit(
            10_000_000_000.0,
            0.15,
            10_000_000_000.0,
            0.10,
            1_000_000_000.0,
            12.0,
            FadeHorizon::Narrow,
            3,
            0.0,
            0.0,
            0.1,
        );

        // First 3 periods should have same EP (stage 1, no decay)
        let ep0 = result.periods[0].economic_profit;
        let ep1 = result.periods[1].economic_profit;
        let ep2 = result.periods[2].economic_profit;
        assert!((ep0 - ep1).abs() < 1e-10);
        assert!((ep1 - ep2).abs() < 1e-10);

        // Period 4 (first fade year) starts at 100% with corrected formula.
        // When ic_growth=0, the fade boundary is smooth — the first fade period
        // equals the last stage 1 period (no artificial discontinuity).
        let ep3 = result.periods[3].economic_profit;
        assert!(
            (ep3 - ep0).abs() < 1e-10,
            "fade starts at 100% (smooth boundary)"
        );

        // Period 5 should be measurably less than period 4 (decay in progress)
        let ep4 = result.periods[4].economic_profit;
        assert!(ep4 < ep3, "fade reduces EP after first step");
    }

    #[test]
    fn signal_classification_undervalued() {
        let sig = classify_signal(1.5, 0.08, 0.7);
        assert_eq!(sig.valuation, "undervalued");
        assert_eq!(sig.profitability, "value_creator");
        assert_eq!(sig.composition, "growth_dependent");
    }

    #[test]
    fn signal_classification_overvalued_destroyer() {
        let sig = classify_signal(0.5, -0.10, 0.1);
        assert_eq!(sig.valuation, "overvalued");
        assert_eq!(sig.profitability, "value_destroyer");
        assert_eq!(sig.composition, "asset_heavy");
    }

    #[test]
    fn extract_ebit_from_json() {
        let income = serde_json::json!({
            "ebit": 5000.0,
            "grossProfit": 8000.0,
            "depreciationAndAmortization": 3000.0,
        });
        assert_eq!(extract_ebit(&income), Some(5000.0));
    }

    #[test]
    fn extract_ebit_fallback() {
        let income = serde_json::json!({
            "grossProfit": 8000.0,
            "depreciationAndAmortization": 3000.0,
        });
        assert_eq!(extract_ebit(&income), Some(5000.0));
    }

    #[test]
    fn extract_ebit_missing() {
        let income = serde_json::json!({});
        assert_eq!(extract_ebit(&income), None);
    }

    #[test]
    fn extract_ic_from_balance() {
        let balance = serde_json::json!({"totalAssets": 200_000.0});
        assert_eq!(extract_invested_capital(&balance), Some(200_000.0));
    }

    #[test]
    fn extract_book_value_from_balance() {
        let balance = serde_json::json!({"totalStockholdersEquity": 80_000.0});
        assert_eq!(extract_book_value(&balance), Some(80_000.0));
    }

    #[test]
    fn extract_book_value_fallback() {
        let balance = serde_json::json!({"totalEquity": 75_000.0});
        assert_eq!(extract_book_value(&balance), Some(75_000.0));
    }

    #[test]
    fn treasury_stock_adjustment() {
        let balance = serde_json::json!({
            "totalStockholdersEquity": 100_000.0,
            "totalAssets": 500_000.0,
            "treasuryStock": -5_000.0,
        });

        // Raw values
        assert_eq!(extract_book_value(&balance), Some(100_000.0));
        assert_eq!(extract_treasury_stock(&balance), 5_000.0);
        assert_eq!(extract_invested_capital(&balance), Some(500_000.0));

        // Adjusted values: raw + 2 × |TS|
        assert_eq!(adj_book_value(&balance), Some(100_000.0 + 10_000.0));
        assert_eq!(adj_invested_capital(&balance), Some(500_000.0 + 10_000.0));
        assert_eq!(adj_equity(&balance), Some(100_000.0 + 10_000.0));
        assert_eq!(adj_total_assets(&balance), Some(500_000.0 + 10_000.0));
    }

    #[test]
    fn treasury_stock_zero_when_missing() {
        let balance = serde_json::json!({
            "totalStockholdersEquity": 100_000.0,
            "totalAssets": 500_000.0,
        });

        assert_eq!(extract_treasury_stock(&balance), 0.0);
        // Adjusted = raw when no treasury stock
        assert_eq!(adj_book_value(&balance), Some(100_000.0));
        assert_eq!(adj_invested_capital(&balance), Some(500_000.0));
    }
}
