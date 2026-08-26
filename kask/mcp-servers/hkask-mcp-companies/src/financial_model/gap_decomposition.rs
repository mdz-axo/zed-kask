//! Forecast-vs-actual return-gap decomposition — extracted from
//! `financial_model.rs` (deep-module split: decomposing the intrinsic-value
//! gap into line-item drivers re-runs `project_model` per driver and is
//! independent of the projection core).

use serde::Serialize;

use super::{HistoricalSnapshot, ProjectedModel, ProjectionAssumptions, project_model};

/// Result of decomposing a forecast-vs-actual return gap.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct GapDecomposition {
    pub total_return_gap: f64,
    pub revenue_growth_contribution: f64,
    pub gross_margin_contribution: f64,
    pub da_contribution: f64,
    pub capex_contribution: f64,
    pub nwc_contribution: f64,
    pub multiple_contribution: f64,
    pub net_debt_contribution: f64,
    pub residual: f64,
}

/// Decompose the gap between projected and actual intrinsic value into
/// 11-line-item drivers. Each contribution is computed by running the
/// projection model with only that one assumption changed to the actual,
/// and measuring the intrinsic value delta.
pub(crate) fn decompose_gap(
    projected: &ProjectedModel,
    projected_assumptions: &ProjectionAssumptions,
    actual_hist: &HistoricalSnapshot,
    actual_price: f64,
    actual_multiple: f64,
    _projected_intrinsic: f64,
    projected_price: f64,
) -> GapDecomposition {
    // Baseline: the original projection gives projected_intrinsic_per_share
    let base_intrinsic = projected.intrinsic_per_share;
    let base_price = projected_price;

    // Total return gap: actual price change - projected price change
    // (if we had projected price and actual price)
    let projected_return = if base_price > 0.0 {
        (base_intrinsic - base_price) / base_price
    } else {
        0.0
    };
    let actual_return = if actual_price > 0.0 && projected_price > 0.0 {
        (actual_price - projected_price) / projected_price
    } else {
        0.0
    };
    let total_return_gap = actual_return - projected_return;

    // Helper to compute what intrinsic would be with one parameter changed.
    // FIX (H5): Use the SAME historical base (actual_hist) for both the base
    // and the alternative, so the delta isolates the driver change only.
    // Previously, `base_intrinsic` was computed from `projected` (which used
    // the original historical data at forecast time), while `alt_model` used
    // `actual_hist` (updated data at outcome time) — contaminating driver
    // contributions with the changed historical base.
    // Now: recompute the base from actual_hist with the original assumptions,
    // so each delta is pure driver effect.
    let base_from_actual = project_model(actual_hist, projected_assumptions, 0.0).intrinsic_per_share;
    let compute_delta = |assumptions: &ProjectionAssumptions| -> f64 {
        let alt_model = project_model(actual_hist, assumptions, 0.0);
        alt_model.intrinsic_per_share - base_from_actual
    };

    // Revenue growth contribution: use actual CAGR vs projected CAGR
    let mut growth_assumptions = projected_assumptions.clone();
    growth_assumptions.revenue_growth = actual_hist.revenue_cagr();
    let revenue_growth_delta = compute_delta(&growth_assumptions);

    // Gross margin contribution
    let mut gm_assumptions = projected_assumptions.clone();
    gm_assumptions.gross_margin = actual_hist.gross_margin();
    let gross_margin_delta = compute_delta(&gm_assumptions);

    // D&A contribution
    let mut da_assumptions = projected_assumptions.clone();
    da_assumptions.da_to_revenue = actual_hist.da_to_revenue();
    let da_delta = compute_delta(&da_assumptions);

    // Capex contribution
    let mut capex_assumptions = projected_assumptions.clone();
    capex_assumptions.capex_to_revenue = actual_hist.capex_to_revenue();
    let capex_delta = compute_delta(&capex_assumptions);

    // NWC contribution
    let mut nwc_assumptions = projected_assumptions.clone();
    nwc_assumptions.nwc_to_revenue = actual_hist.nwc_to_revenue();
    let nwc_delta = compute_delta(&nwc_assumptions);

    // Multiple contribution: (actual multiple - projected multiple) * actual_fcf
    let projected_multiple = if let Some(last) = projected.periods.last() {
        if last.free_cash_flow > 0.0 {
            projected.terminal_value / last.free_cash_flow
        } else {
            0.0
        }
    } else {
        0.0
    };
    let multiple_delta = (actual_multiple - projected_multiple) * 10.0;

    // Net debt contribution: change in net debt directly affects equity value
    let projected_net_debt = projected.net_debt;
    let actual_net_debt = actual_hist.net_debt();
    let net_debt_delta =
        (projected_net_debt - actual_net_debt) / actual_hist.shares_outstanding.max(1.0);

    // Residual: total gap minus sum of contributions
    let sum_contributions = revenue_growth_delta
        + gross_margin_delta
        + da_delta
        + capex_delta
        + nwc_delta
        + multiple_delta
        + net_debt_delta;
    let residual =
        (actual_return * base_price) - (projected_return * base_price) - sum_contributions;

    GapDecomposition {
        total_return_gap,
        revenue_growth_contribution: revenue_growth_delta,
        gross_margin_contribution: gross_margin_delta,
        da_contribution: da_delta,
        capex_contribution: capex_delta,
        nwc_contribution: nwc_delta,
        multiple_contribution: multiple_delta,
        net_debt_contribution: net_debt_delta,
        residual,
    }
}
