//! hKask MCP FMP — Value-added financial analysis (MAIA framework)
//!
//! Pure functions for computing investment analysis from FMP data.
//! No API calls, no async — these operate on already-fetched JSON values.

use serde_json::Value;

/// Gross margin stability score (0.0–1.0). Higher = more stable.
/// Uses coefficient of variation: lower CV → more stable → higher score.
/// Returns 1.0 for perfect stability, near 0.0 for high volatility.
pub(crate) fn gross_margin_stability(margins: &[f64]) -> f64 {
    if margins.len() < 2 {
        return 1.0;
    }
    let mean = margins.iter().sum::<f64>() / margins.len() as f64;
    if mean == 0.0 {
        return 0.0;
    }
    let variance = margins.iter().map(|m| (m - mean).powi(2)).sum::<f64>() / margins.len() as f64;
    let cv = variance.sqrt() / mean.abs();
    // Score: 1.0 / (1.0 + CV). CV of 0 → 1.0, CV of 1.0 → 0.5, CV of 10 → ~0.09
    (1.0 / (1.0 + cv)).clamp(0.0, 1.0)
}

/// Working capital moat signal: DPO − DSO in days.
/// Positive = customers pay faster than you pay suppliers (market power).
pub(crate) fn working_capital_spread(dpo_days: f64, dso_days: f64) -> f64 {
    dpo_days - dso_days
}

/// Classify the working capital signal.
pub(crate) fn wc_signal_label(spread: f64) -> &'static str {
    if spread > 30.0 {
        "strong_market_power"
    } else if spread > 0.0 {
        "moderate_market_power"
    } else if spread > -15.0 {
        "neutral"
    } else {
        "supplier_dominated"
    }
}

/// Overall moat classification from margin stability and working capital signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MoatRating {
    Wide,
    Narrow,
    None,
    InsufficientData,
}

pub(crate) fn classify_moat(
    margin_stability: f64,
    wc_spread: f64,
    data_periods: usize,
) -> MoatRating {
    if data_periods < 3 {
        return MoatRating::InsufficientData;
    }
    let has_stable_margins = margin_stability > 0.7;
    let has_market_power = wc_spread > 0.0;

    if has_stable_margins && has_market_power {
        MoatRating::Wide
    } else if has_stable_margins || has_market_power {
        MoatRating::Narrow
    } else {
        MoatRating::None
    }
}

/// Extract gross margin values from FMP income-statement JSON array.
/// Computes grossProfit / revenue per period since the stable key-metrics
/// endpoint does not include grossProfitMargin.
/// Returns Vec of (year, margin) sorted by year ascending.
pub(crate) fn extract_gross_margins(income_json: &Value) -> Vec<(String, f64)> {
    let arr = match income_json.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut margins: Vec<(String, f64)> = arr
        .iter()
        .filter_map(|entry| {
            let year = extract_year(entry)?;
            let gross_profit = entry.get("grossProfit")?.as_f64()?;
            let revenue = entry.get("revenue")?.as_f64()?;
            if revenue == 0.0 {
                return None;
            }
            Some((year, gross_profit / revenue))
        })
        .collect();
    margins.sort_by(|a, b| a.0.cmp(&b.0));
    margins
}

/// Extract a year label from a JSON entry, trying `calendarYear`,
/// `fiscalYear`, then `date` (first 4 chars).
pub(crate) fn extract_year(entry: &Value) -> Option<String> {
    if let Some(y) = entry.get("calendarYear").and_then(|v| v.as_str()) {
        return Some(y.to_string());
    }
    if let Some(y) = entry.get("fiscalYear").and_then(|v| v.as_str()) {
        return Some(y.to_string());
    }
    if let Some(y) = entry.get("fiscalYear").and_then(|v| v.as_i64()) {
        return Some(y.to_string());
    }
    entry
        .get("date")
        .and_then(|v| v.as_str())
        .and_then(|s| s.get(..4).map(String::from))
}

/// Compute working capital days (DPO, DSO, DIO) from a set of balance sheet / income
/// statement pairs. Returns (dpo, dso) or None if data is insufficient.
///
/// FMP key-metrics provides daysOfPayablesOutstanding and daysOfSalesOutstanding.
pub(crate) fn extract_wc_days(metrics_json: &Value) -> Option<(f64, f64)> {
    let arr = metrics_json.as_array()?;
    let latest = arr.first()?;
    let dpo = latest.get("daysOfPayablesOutstanding")?.as_f64()?;
    let dso = latest.get("daysOfSalesOutstanding")?.as_f64()?;
    Some((dpo, dso))
}

// ── Tool 2: Management Scorecard ──

/// CEO capital allocation rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CeoRating {
    Excellent,
    Good,
    Neutral,
    Poor,
    InsufficientData,
}

/// Classify CEO capital allocation quality from returns on capital (ROIC/ROE) and
/// invested capital changes over time.
///
/// MAIA framework: Good = decreasing capital with steady/improving returns, OR
/// increasing capital with improving returns. Bad = increasing capital with
/// decreasing returns.
pub(crate) fn ceo_capital_allocation_score(returns: &[f64], invested_capital: &[f64]) -> CeoRating {
    if returns.len() < 3 || invested_capital.len() < 3 {
        return CeoRating::InsufficientData;
    }

    // Compute direction: first half vs second half averages
    let mid = returns.len() / 2;
    let early_return = returns[..mid].iter().sum::<f64>() / mid as f64;
    let late_return = returns[mid..].iter().sum::<f64>() / (returns.len() - mid) as f64;
    let early_capital = invested_capital[..mid].iter().sum::<f64>() / mid as f64;
    let late_capital =
        invested_capital[mid..].iter().sum::<f64>() / (invested_capital.len() - mid) as f64;

    let return_improving = late_return > early_return;
    let capital_decreasing = late_capital < early_capital;
    let capital_increasing = late_capital > early_capital;

    // MAIA: Good = decreasing capital + steady/improving returns,
    //       OR increasing capital + improving returns
    if (capital_increasing || capital_decreasing) && return_improving {
        // Distinguish Excellent (returns significantly improved)
        if late_return > early_return * 1.1 {
            CeoRating::Excellent
        } else {
            CeoRating::Good
        }
    } else if capital_increasing && !return_improving {
        // MAIA: Bad = increasing capital + decreasing returns
        CeoRating::Poor
    } else {
        CeoRating::Neutral
    }
}

/// Extract ROIC values from FMP key-metrics JSON array.
/// Tries `roic` (v3 field) then `returnOnInvestedCapital` (stable field).
/// Returns Vec of (year, roic) sorted by year ascending.
pub(crate) fn extract_roic(metrics_json: &Value) -> Vec<(String, f64)> {
    let arr = match metrics_json.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut values: Vec<(String, f64)> = arr
        .iter()
        .filter_map(|entry| {
            let year = extract_year(entry)?;
            let roic = entry
                .get("roic")
                .and_then(|v| v.as_f64())
                .or_else(|| entry.get("returnOnInvestedCapital").and_then(|v| v.as_f64()))?;
            Some((year, roic))
        })
        .collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    values
}

/// Extract invested capital from balance sheet JSON by computing total assets.
/// Returns Vec of (year, total_assets) sorted by year ascending.
pub(crate) fn extract_invested_capital(balance_sheets: &Value) -> Vec<(String, f64)> {
    let arr = match balance_sheets.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut values: Vec<(String, f64)> = arr
        .iter()
        .filter_map(|entry| {
            let year = extract_year(entry)?;
            let assets = entry.get("totalAssets")?.as_f64()?;
            Some((year, assets))
        })
        .collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    values
}
