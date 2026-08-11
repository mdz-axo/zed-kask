//! DCF sensitivity (tornado) analysis — extracted from `financial_model.rs`
//! (deep-module split: varying each assumption +/- `range_pct` and ranking by
//! intrinsic-value delta re-runs `project_model` per driver; the fibo concept
//! labels come from `crate::fibo`).

use serde::Serialize;

use super::{HistoricalSnapshot, ProjectionAssumptions, project_model};
use crate::fibo::{
    CAPITAL_EXPENDITURE, DEPRECIATION_AND_AMORTIZATION, DISCOUNT_RATE, GROSS_PROFIT_MARGIN,
    NET_WORKING_CAPITAL, REVENUE_GROWTH_RATE,
};

/// Result of varying one assumption and measuring intrinsic value delta.
#[derive(Debug, Clone, Serialize)]
pub struct SensitivityResult {
    pub driver: String,
    pub label: String,
    pub base_value: f64,
    pub low_value: f64,
    pub high_value: f64,
    pub intrinsic_low: f64,
    pub intrinsic_high: f64,
    pub delta_pct: f64,
    pub fibo_concept: &'static str,
}

/// Run sensitivity analysis on all key DCF drivers.
/// Varies each assumption by +/- range_pct and records intrinsic value impact.
/// Returns results sorted by absolute delta (most impactful first).
pub fn sensitivity_analysis(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    range_pct: f64,
) -> Vec<SensitivityResult> {
    let base = project_model(hist, base_assumptions, 0.0);
    let base_intrinsic = base.intrinsic_per_share;

    let drivers: [(
        &str,
        &str,
        &dyn Fn(&ProjectionAssumptions) -> f64,
        &dyn Fn(&mut ProjectionAssumptions, f64),
        &str,
    ); 6] = [
        (
            "revenue_growth",
            "Revenue Growth",
            &|a| a.revenue_growth,
            &|a, v| a.revenue_growth = v.clamp(-0.50, 1.00),
            REVENUE_GROWTH_RATE,
        ),
        (
            "gross_margin",
            "Gross Margin",
            &|a| a.gross_margin,
            &|a, v| a.gross_margin = v.clamp(0.05, 0.95),
            GROSS_PROFIT_MARGIN,
        ),
        (
            "da_to_revenue",
            "D&A / Revenue",
            &|a| a.da_to_revenue,
            &|a, v| a.da_to_revenue = v.clamp(0.0, 0.20),
            DEPRECIATION_AND_AMORTIZATION,
        ),
        (
            "capex_to_revenue",
            "Capex / Revenue",
            &|a| a.capex_to_revenue,
            &|a, v| a.capex_to_revenue = v.clamp(0.0, 0.30),
            CAPITAL_EXPENDITURE,
        ),
        (
            "nwc_to_revenue",
            "NWC / Revenue",
            &|a| a.nwc_to_revenue,
            &|a, v| a.nwc_to_revenue = v.clamp(-0.20, 0.50),
            NET_WORKING_CAPITAL,
        ),
        (
            "discount_rate",
            "Discount Rate",
            &|a| a.discount_rate,
            &|a, v| a.discount_rate = v.clamp((a.terminal_growth + 0.0001).max(0.05), 0.30),
            DISCOUNT_RATE,
        ),
    ];

    let mut results = Vec::new();
    for (key, label, getter, setter, fibo) in &drivers {
        let base_val = getter(base_assumptions);
        let low_val = base_val * (1.0 - range_pct);
        let high_val = base_val * (1.0 + range_pct);

        let mut low_a = base_assumptions.clone();
        setter(&mut low_a, low_val);
        let intrinsic_low = project_model(hist, &low_a, 0.0).intrinsic_per_share;

        let mut high_a = base_assumptions.clone();
        setter(&mut high_a, high_val);
        let intrinsic_high = project_model(hist, &high_a, 0.0).intrinsic_per_share;

        let delta_pct = if base_intrinsic > 0.0 {
            (intrinsic_high - intrinsic_low) / base_intrinsic
        } else {
            0.0
        };

        results.push(SensitivityResult {
            driver: key.to_string(),
            label: label.to_string(),
            base_value: base_val,
            low_value: low_val,
            high_value: high_val,
            intrinsic_low,
            intrinsic_high,
            delta_pct,
            fibo_concept: fibo,
        });
    }

    results.sort_by(|a, b| {
        b.delta_pct
            .partial_cmp(&a.delta_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}
