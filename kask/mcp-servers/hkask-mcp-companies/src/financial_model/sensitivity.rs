//! DCF sensitivity (tornado) analysis — extracted from `financial_model.rs`
//! (deep-module split: varying each assumption +/- `range_pct` and ranking by
//! intrinsic-value delta re-runs `project_financial_model` per driver; the fibo concept
//! labels come from `crate::fibo`).

use serde::Serialize;

use super::{HistoricalSnapshot, ProjectionAssumptions, ProjectionError, project_financial_model};
use crate::fibo::{
    METRIC_CAPITAL_EXPENDITURE, METRIC_DEPRECIATION_AND_AMORTIZATION, METRIC_DISCOUNT_RATE,
    METRIC_GROSS_PROFIT_MARGIN, METRIC_NET_WORKING_CAPITAL, METRIC_REVENUE_GROWTH_RATE,
};

/// Result of varying one assumption and measuring intrinsic value delta.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct SensitivityResult {
    pub driver: String,
    pub label: String,
    pub base_value: f64,
    pub low_value: f64,
    pub high_value: f64,
    pub intrinsic_low: f64,
    pub intrinsic_high: f64,
    pub delta_pct: f64,
    /// Internal metric identifier for the driver (hKask canonical metric
    /// name — not an ontology URI).
    pub metric: &'static str,
}

/// Run sensitivity analysis on all key DCF drivers.
/// Varies each assumption by +/- range_pct and records intrinsic value impact.
/// Returns results sorted by absolute delta (most impactful first).
pub(crate) fn sensitivity_analysis(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    range_pct: f64,
) -> Result<Vec<SensitivityResult>, ProjectionError> {
    let base = project_financial_model(hist, base_assumptions)?;
    let base_intrinsic = base.intrinsic_per_share;

    let drivers: [(
        &str,
        &str,
        &dyn Fn(&ProjectionAssumptions) -> f64,
        &dyn Fn(&mut ProjectionAssumptions, f64),
        &str,
    ); 8] = [
        (
            "revenue_growth",
            "Revenue Growth",
            &|a| a.revenue_growth,
            &|a, v| a.revenue_growth = v.clamp(-0.50, 1.00),
            METRIC_REVENUE_GROWTH_RATE,
        ),
        (
            "gross_margin",
            "Gross Margin",
            &|a| a.gross_margin,
            &|a, v| a.gross_margin = v.clamp(0.05, 0.95),
            METRIC_GROSS_PROFIT_MARGIN,
        ),
        (
            "other_operating_expense_to_revenue",
            "Other Operating Expense / Revenue",
            &|a| a.other_operating_expense_to_revenue,
            &|a, v| a.other_operating_expense_to_revenue = v.clamp(0.0, 1.0),
            "operating_expense_reconciliation",
        ),
        (
            "da_to_revenue",
            "D&A / Revenue",
            &|a| a.da_to_revenue,
            &|a, v| a.da_to_revenue = v.clamp(0.0, 0.20),
            METRIC_DEPRECIATION_AND_AMORTIZATION,
        ),
        (
            "capex_to_revenue",
            "Capex / Revenue",
            &|a| a.capex_to_revenue,
            &|a, v| a.capex_to_revenue = v.clamp(0.0, 0.30),
            METRIC_CAPITAL_EXPENDITURE,
        ),
        (
            "nwc_to_revenue",
            "NWC / Revenue",
            &|a| a.nwc_to_revenue,
            &|a, v| a.nwc_to_revenue = v.clamp(-0.20, 0.50),
            METRIC_NET_WORKING_CAPITAL,
        ),
        (
            "terminal_growth",
            "Terminal Growth",
            &|a| a.terminal_growth,
            &|a, v| a.terminal_growth = v.clamp(0.0, (a.discount_rate - 0.0001).max(0.0)),
            "terminal_growth_rate",
        ),
        (
            "discount_rate",
            "Discount Rate",
            &|a| a.discount_rate,
            &|a, v| a.discount_rate = v.clamp((a.terminal_growth + 0.0001).max(0.05), 0.30),
            METRIC_DISCOUNT_RATE,
        ),
    ];

    let mut results = Vec::new();
    for (key, label, getter, setter, metric) in &drivers {
        let base_val = getter(base_assumptions);
        let low_val = base_val * (1.0 - range_pct);
        let high_val = base_val * (1.0 + range_pct);

        let mut low_a = base_assumptions.clone();
        setter(&mut low_a, low_val);
        let intrinsic_low = project_financial_model(hist, &low_a)?.intrinsic_per_share;

        let mut high_a = base_assumptions.clone();
        setter(&mut high_a, high_val);
        let intrinsic_high = project_financial_model(hist, &high_a)?.intrinsic_per_share;

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
            metric,
        });
    }

    let base_horizon = base_assumptions.total_years;
    let low_horizon = base_horizon
        .saturating_sub(1)
        .max(base_assumptions.stage1_years + 1);
    let high_horizon = base_horizon.saturating_add(1);
    let mut low_assumptions = base_assumptions.clone();
    low_assumptions.total_years = low_horizon;
    let mut high_assumptions = base_assumptions.clone();
    high_assumptions.total_years = high_horizon;
    let intrinsic_low = project_financial_model(hist, &low_assumptions)?.intrinsic_per_share;
    let intrinsic_high = project_financial_model(hist, &high_assumptions)?.intrinsic_per_share;
    results.push(SensitivityResult {
        driver: "forecast_horizon_years".to_string(),
        label: "Forecast Horizon".to_string(),
        base_value: f64::from(base_horizon),
        low_value: f64::from(low_horizon),
        high_value: f64::from(high_horizon),
        intrinsic_low,
        intrinsic_high,
        delta_pct: if base_intrinsic > 0.0 {
            (intrinsic_high - intrinsic_low) / base_intrinsic
        } else {
            0.0
        },
        metric: "forecast_horizon",
    });

    results.sort_by(|a, b| {
        b.delta_pct
            .abs()
            .partial_cmp(&a.delta_pct.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(results)
}
