//! hKask MCP FMP — Value-added financial analysis (MAIA framework)
//!
//! Pure functions for computing investment analysis from FMP data.
//! No API calls, no async — these operate on already-fetched JSON values.

use serde_json::Value;

/// Gross margin stability score (0.0–1.0). Higher = more stable.
/// Uses coefficient of variation: lower CV → more stable → higher score.
/// Returns 1.0 for perfect stability, near 0.0 for high volatility.
pub fn gross_margin_stability(margins: &[f64]) -> f64 {
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
pub fn working_capital_spread(dpo_days: f64, dso_days: f64) -> f64 {
    dpo_days - dso_days
}

/// Classify the working capital signal.
pub fn wc_signal_label(spread: f64) -> &'static str {
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
pub enum MoatRating {
    Wide,
    Narrow,
    None,
    InsufficientData,
}

pub fn classify_moat(margin_stability: f64, wc_spread: f64, data_periods: usize) -> MoatRating {
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

/// Extract gross margin values from FMP key-metrics JSON array.
/// Returns Vec of (year, grossProfitMargin) sorted by year ascending.
pub fn extract_gross_margins(metrics_json: &Value) -> Vec<(String, f64)> {
    let arr = match metrics_json.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut margins: Vec<(String, f64)> = arr
        .iter()
        .filter_map(|entry| {
            let year = entry.get("calendarYear")?.as_str().unwrap_or("");
            let margin = entry.get("grossProfitMargin")?.as_f64()?;
            Some((year.to_string(), margin))
        })
        .collect();
    margins.sort_by(|a, b| a.0.cmp(&b.0));
    margins
}

/// Compute working capital days (DPO, DSO, DIO) from a set of balance sheet / income
/// statement pairs. Returns (dpo, dso) or None if data is insufficient.
///
/// FMP key-metrics provides daysOfPayablesOutstanding and daysOfSalesOutstanding.
pub fn extract_wc_days(metrics_json: &Value) -> Option<(f64, f64)> {
    let arr = metrics_json.as_array()?;
    let latest = arr.first()?;
    let dpo = latest.get("daysOfPayablesOutstanding")?.as_f64()?;
    let dso = latest.get("daysOfSalesOutstanding")?.as_f64()?;
    Some((dpo, dso))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gross_margin_stability_perfect() {
        let score = gross_margin_stability(&[0.60, 0.60, 0.60, 0.60]);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn gross_margin_stability_volatile() {
        let score = gross_margin_stability(&[0.60, 0.20, 0.80, 0.10]);
        assert!(
            score < 0.75,
            "volatile margins should score lower than stable: {score}"
        );
        assert!(
            score > 0.0,
            "volatile margins should not score zero: {score}"
        );
    }

    #[test]
    fn gross_margin_stability_single_point() {
        assert!((gross_margin_stability(&[0.60]) - 1.0).abs() < 0.001);
        assert!((gross_margin_stability(&[]) - 1.0).abs() < 0.001);
    }

    #[test]
    fn gross_margin_stability_zero_mean() {
        assert!((gross_margin_stability(&[0.0, 0.0, 0.0]) - 0.0).abs() < 0.001);
    }

    #[test]
    fn working_capital_spread_computation() {
        assert!((working_capital_spread(90.0, 30.0) - 60.0).abs() < 0.001);
        assert!((working_capital_spread(20.0, 40.0) - (-20.0)).abs() < 0.001);
    }

    #[test]
    fn wc_signal_label_classification() {
        assert_eq!(wc_signal_label(60.0), "strong_market_power");
        assert_eq!(wc_signal_label(15.0), "moderate_market_power");
        assert_eq!(wc_signal_label(-5.0), "neutral");
        assert_eq!(wc_signal_label(-30.0), "supplier_dominated");
    }

    #[test]
    fn classify_moat_wide() {
        assert_eq!(classify_moat(0.9, 40.0, 5), MoatRating::Wide);
    }

    #[test]
    fn classify_moat_narrow() {
        assert_eq!(classify_moat(0.9, -10.0, 5), MoatRating::Narrow);
        assert_eq!(classify_moat(0.3, 40.0, 5), MoatRating::Narrow);
    }

    #[test]
    fn classify_moat_none() {
        assert_eq!(classify_moat(0.3, -10.0, 5), MoatRating::None);
    }

    #[test]
    fn classify_moat_insufficient_data() {
        assert_eq!(classify_moat(0.9, 40.0, 2), MoatRating::InsufficientData);
        assert_eq!(classify_moat(0.3, -10.0, 1), MoatRating::InsufficientData);
    }

    #[test]
    fn extract_gross_margins_from_json() {
        let json = serde_json::json!([
            {"calendarYear": "2024", "grossProfitMargin": 0.45},
            {"calendarYear": "2023", "grossProfitMargin": 0.43},
            {"calendarYear": "2022", "grossProfitMargin": 0.44},
        ]);
        let margins = extract_gross_margins(&json);
        assert_eq!(margins.len(), 3);
        assert_eq!(margins[0].0, "2022");
        assert_eq!(margins[2].0, "2024");
        // Index 1 = 2023 with margin 0.43
        assert!((margins[1].1 - 0.43).abs() < 0.001);
    }

    #[test]
    fn extract_gross_margins_empty() {
        assert!(extract_gross_margins(&serde_json::json!([])).is_empty());
        assert!(extract_gross_margins(&serde_json::json!({})).is_empty());
    }

    #[test]
    fn extract_wc_days_from_json() {
        let json = serde_json::json!([
            {"calendarYear": "2024", "daysOfPayablesOutstanding": 90.0, "daysOfSalesOutstanding": 30.0},
        ]);
        let (dpo, dso) = extract_wc_days(&json).unwrap();
        assert!((dpo - 90.0).abs() < 0.001);
        assert!((dso - 30.0).abs() < 0.001);
    }

    #[test]
    fn extract_wc_days_missing_fields() {
        assert!(extract_wc_days(&serde_json::json!([])).is_none());
        assert!(extract_wc_days(&serde_json::json!([{"year": "2024"}])).is_none());
    }

    // ── Tracer-bullet contract: moat_check analysis pipeline ────────
    //
    // Verifies the full MAIA moat analysis pipeline end-to-end:
    // extract → stability → WC spread → classify.
    // This is the narrowest path that exercises all analysis functions
    // with known data, serving as the behavioral contract for the moat_check tool.

    #[test]
    fn moat_check_pipeline_contract_wide_moat() {
        // Simulated key_metrics JSON: 5 years of stable gross margins + strong WC
        let metrics = serde_json::json!([
            {"calendarYear": "2024", "grossProfitMargin": 0.45, "daysOfPayablesOutstanding": 90.0, "daysOfSalesOutstanding": 30.0},
            {"calendarYear": "2023", "grossProfitMargin": 0.44, "daysOfPayablesOutstanding": 88.0, "daysOfSalesOutstanding": 32.0},
            {"calendarYear": "2022", "grossProfitMargin": 0.43, "daysOfPayablesOutstanding": 85.0, "daysOfSalesOutstanding": 31.0},
            {"calendarYear": "2021", "grossProfitMargin": 0.44, "daysOfPayablesOutstanding": 87.0, "daysOfSalesOutstanding": 29.0},
            {"calendarYear": "2020", "grossProfitMargin": 0.42, "daysOfPayablesOutstanding": 82.0, "daysOfSalesOutstanding": 33.0},
        ]);

        // Step 1: Extract gross margins
        let margins = extract_gross_margins(&metrics);
        assert_eq!(margins.len(), 5, "5 years of margin data");

        // Step 2: Compute margin stability
        let margin_values: Vec<f64> = margins.iter().map(|(_, m)| *m).collect();
        let stability = gross_margin_stability(&margin_values);
        assert!(stability > 0.9, "highly stable margins (>0.9)");

        // Step 3: Extract working capital days
        let (dpo, dso) = extract_wc_days(&metrics).unwrap();
        let wc_spread = working_capital_spread(dpo, dso);
        assert!(wc_spread > 30.0, "strong market power — DPO >> DSO");
        assert_eq!(wc_signal_label(wc_spread), "strong_market_power");

        // Step 4: Classify moat
        let rating = classify_moat(stability, wc_spread, margins.len());
        assert_eq!(
            rating,
            MoatRating::Wide,
            "stable margins + market power = Wide moat"
        );
    }

    #[test]
    fn moat_check_pipeline_contract_insufficient_data() {
        // Only 2 years of data → insufficient
        let metrics = serde_json::json!([
            {"calendarYear": "2024", "grossProfitMargin": 0.45},
            {"calendarYear": "2023", "grossProfitMargin": 0.44},
        ]);
        let margins = extract_gross_margins(&metrics);
        let margin_values: Vec<f64> = margins.iter().map(|(_, m)| *m).collect();
        let stability = gross_margin_stability(&margin_values);
        let wc_data = extract_wc_days(&metrics);
        let wc_spread = wc_data.map_or(0.0, |(d, s)| working_capital_spread(d, s));
        let rating = classify_moat(stability, wc_spread, margins.len());
        assert_eq!(
            rating,
            MoatRating::InsufficientData,
            "<3 periods → insufficient"
        );
    }
}

// ── Tool 2: Management Scorecard ──

/// CEO capital allocation rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CeoRating {
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
pub fn ceo_capital_allocation_score(returns: &[f64], invested_capital: &[f64]) -> CeoRating {
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
/// Returns Vec of (year, roic) sorted by year ascending.
pub fn extract_roic(metrics_json: &Value) -> Vec<(String, f64)> {
    let arr = match metrics_json.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut values: Vec<(String, f64)> = arr
        .iter()
        .filter_map(|entry| {
            let year = entry.get("calendarYear")?.as_str().unwrap_or("");
            let roic = entry.get("roic")?.as_f64()?;
            Some((year.to_string(), roic))
        })
        .collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    values
}

/// Extract invested capital from balance sheet JSON by computing total assets.
/// Returns Vec of (year, total_assets) sorted by year ascending.
pub fn extract_invested_capital(balance_sheets: &Value) -> Vec<(String, f64)> {
    let arr = match balance_sheets.as_array() {
        Some(a) => a,
        None => return vec![],
    };
    let mut values: Vec<(String, f64)> = arr
        .iter()
        .filter_map(|entry| {
            let year = entry.get("calendarYear")?.as_str().unwrap_or("");
            let assets = entry.get("totalAssets")?.as_f64()?;
            Some((year.to_string(), assets))
        })
        .collect();
    values.sort_by(|a, b| a.0.cmp(&b.0));
    values
}

#[cfg(test)]
mod management_tests {
    use super::*;

    #[test]
    fn ceo_score_insufficient_data() {
        assert_eq!(
            ceo_capital_allocation_score(&[0.15, 0.16], &[100.0, 105.0]),
            CeoRating::InsufficientData
        );
    }

    #[test]
    fn ceo_score_excellent_decreasing_capital_improving_returns() {
        let returns = [0.10, 0.10, 0.12, 0.12, 0.15, 0.20];
        let capital = [500.0, 490.0, 480.0, 460.0, 440.0, 420.0];
        assert_eq!(
            ceo_capital_allocation_score(&returns, &capital),
            CeoRating::Excellent
        );
    }

    #[test]
    fn ceo_score_good_increasing_capital_improving_returns() {
        // Returns improve but by less than 10% → Good, not Excellent
        let returns = [0.10, 0.10, 0.11, 0.108, 0.108, 0.109];
        let capital = [100.0, 110.0, 120.0, 130.0, 140.0, 150.0];
        assert_eq!(
            ceo_capital_allocation_score(&returns, &capital),
            CeoRating::Good
        );
    }

    #[test]
    fn ceo_score_poor_increasing_capital_decreasing_returns() {
        let returns = [0.20, 0.18, 0.18, 0.15, 0.12, 0.10];
        let capital = [100.0, 120.0, 140.0, 180.0, 220.0, 300.0];
        assert_eq!(
            ceo_capital_allocation_score(&returns, &capital),
            CeoRating::Poor
        );
    }

    #[test]
    fn extract_roic_from_json() {
        let json = serde_json::json!([
            {"calendarYear": "2024", "roic": 0.18},
            {"calendarYear": "2023", "roic": 0.16},
        ]);
        let roic = extract_roic(&json);
        assert_eq!(roic.len(), 2);
        assert!((roic[0].1 - 0.16).abs() < 0.001);
        assert!((roic[1].1 - 0.18).abs() < 0.001);
    }

    #[test]
    fn extract_invested_capital_from_json() {
        let json = serde_json::json!([
            {"calendarYear": "2024", "totalAssets": 1000.0},
            {"calendarYear": "2023", "totalAssets": 900.0},
        ]);
        let cap = extract_invested_capital(&json);
        assert_eq!(cap.len(), 2);
        assert!((cap[0].1 - 900.0).abs() < 0.001);
    }

    // ── Tracer-bullet contract: year-aligned CEO scoring ───────────
    //
    // Verifies the fix from B2: when ROIC and invested capital come from
    // different API endpoints with different year ranges, only the
    // overlapping years are used in the CEO rating computation.
    //
    // This is the behavioral contract for management_scorecard alignment.

    #[test]
    fn ceo_score_with_partially_overlapping_years() {
        // ROIC: 5 years (2020-2024), Capital: 3 years (2022-2024)
        // Alignment produces 3 overlapping years → should score
        let roic_by_year: Vec<(String, f64)> = vec![
            ("2020".into(), 0.10),
            ("2021".into(), 0.11),
            ("2022".into(), 0.12),
            ("2023".into(), 0.13),
            ("2024".into(), 0.14),
        ];
        let capital_by_year: Vec<(String, f64)> = vec![
            ("2022".into(), 500.0),
            ("2023".into(), 480.0),
            ("2024".into(), 450.0),
        ];

        // Align by calendar year (simulates the B2 fix)
        use std::collections::HashMap;
        let roic_map: HashMap<&str, f64> =
            roic_by_year.iter().map(|(y, v)| (y.as_str(), *v)).collect();
        let mut aligned: Vec<(f64, f64)> = capital_by_year
            .iter()
            .filter_map(|(year, cap)| roic_map.get(year.as_str()).map(|r| (*r, *cap)))
            .collect();
        aligned.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

        let roic_nums: Vec<f64> = aligned.iter().map(|(r, _)| *r).collect();
        let capital_nums: Vec<f64> = aligned.iter().map(|(_, c)| *c).collect();

        assert_eq!(aligned.len(), 3, "3 overlapping years");
        assert_eq!(roic_nums.len(), 3);
        assert_eq!(capital_nums.len(), 3);

        // 3 aligned years with improving returns + decreasing capital → should rate
        let rating = ceo_capital_allocation_score(&roic_nums, &capital_nums);
        assert!(
            matches!(
                rating,
                CeoRating::Excellent | CeoRating::Good | CeoRating::Neutral | CeoRating::Poor
            ),
            "3 years should produce a real rating, not InsufficientData"
        );
    }

    #[test]
    fn ceo_score_with_insufficient_overlap() {
        // ROIC: 5 years, Capital: only 2 years → alignment yields 2 years
        let roic_by_year: Vec<(String, f64)> = vec![
            ("2020".into(), 0.10),
            ("2021".into(), 0.11),
            ("2022".into(), 0.12),
            ("2023".into(), 0.13),
            ("2024".into(), 0.14),
        ];
        let capital_by_year: Vec<(String, f64)> =
            vec![("2023".into(), 500.0), ("2024".into(), 480.0)];

        use std::collections::HashMap;
        let roic_map: HashMap<&str, f64> =
            roic_by_year.iter().map(|(y, v)| (y.as_str(), *v)).collect();
        let aligned: Vec<(f64, f64)> = capital_by_year
            .iter()
            .filter_map(|(year, cap)| roic_map.get(year.as_str()).map(|r| (*r, *cap)))
            .collect();
        let roic_nums: Vec<f64> = aligned.iter().map(|(r, _)| *r).collect();
        let capital_nums: Vec<f64> = aligned.iter().map(|(_, c)| *c).collect();

        assert_eq!(aligned.len(), 2, "only 2 overlapping years");

        // <3 aligned years → InsufficientData
        let rating = ceo_capital_allocation_score(&roic_nums, &capital_nums);
        assert_eq!(
            rating,
            CeoRating::InsufficientData,
            "<3 aligned years → insufficient"
        );
    }
}
