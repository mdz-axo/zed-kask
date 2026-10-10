//! Forecast-vs-actual return-gap decomposition — extracted from
//! `financial_model.rs` (deep-module split: decomposing the intrinsic-value
//! gap into line-item drivers re-runs `project_financial_model` per driver and is
//! independent of the projection core).

use serde::Serialize;

use super::{
    HistoricalSnapshot, ProjectedFinancialModel, ProjectionAssumptions, ProjectionError,
    project_financial_model,
};

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
    projected: &ProjectedFinancialModel,
    projected_assumptions: &ProjectionAssumptions,
    actual_hist: &HistoricalSnapshot,
    actual_price: f64,
    actual_multiple: f64,
    _projected_intrinsic: f64,
    projected_price: f64,
) -> Result<GapDecomposition, ProjectionError> {
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
    let base_model = project_financial_model(actual_hist, projected_assumptions)?;
    let base_from_actual = base_model.intrinsic_per_share;
    let compute_delta = |assumptions: &ProjectionAssumptions| -> Result<f64, ProjectionError> {
        let alternative = project_financial_model(actual_hist, assumptions)?;
        Ok(alternative.intrinsic_per_share - base_from_actual)
    };

    // Revenue growth contribution: use actual CAGR vs projected CAGR
    let mut growth_assumptions = projected_assumptions.clone();
    growth_assumptions.revenue_growth = actual_hist.revenue_cagr();
    let revenue_growth_delta = compute_delta(&growth_assumptions)?;

    // Gross margin contribution
    let mut gm_assumptions = projected_assumptions.clone();
    gm_assumptions.gross_margin = actual_hist.gross_margin();
    let gross_margin_delta = compute_delta(&gm_assumptions)?;

    // D&A contribution
    let mut da_assumptions = projected_assumptions.clone();
    da_assumptions.da_to_revenue = actual_hist.da_to_revenue();
    let da_delta = compute_delta(&da_assumptions)?;

    // Capex contribution
    let mut capex_assumptions = projected_assumptions.clone();
    capex_assumptions.capex_to_revenue = actual_hist.capex_to_revenue();
    let capex_delta = compute_delta(&capex_assumptions)?;

    // NWC contribution
    let mut nwc_assumptions = projected_assumptions.clone();
    nwc_assumptions.nwc_to_revenue = actual_hist.nwc_to_revenue();
    let nwc_delta = compute_delta(&nwc_assumptions)?;

    // Multiple contribution: the per-share terminal-value delta from the
    // terminal multiple coming in as `actual_multiple`, holding the FCF base
    // fixed — (actual multiple − base implied multiple) × base last-year FCF
    // per share. The base is the same actual-hist re-projection the other
    // drivers use (H5), so the delta isolates the multiple change only. The
    // former `× 10.0` was an unscaled placeholder — right only when
    // per-share FCF happened to equal 10 (scoring audit 2026-10-09).
    let base_last_fcf = base_model
        .periods
        .last()
        .map(|period| period.free_cash_flow)
        .unwrap_or(0.0);
    let base_implied_multiple = if base_last_fcf > 0.0 {
        base_model.terminal_value / base_last_fcf
    } else {
        0.0
    };
    let multiple_delta =
        (actual_multiple - base_implied_multiple) * base_last_fcf / actual_hist.shares_outstanding;

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

    Ok(GapDecomposition {
        total_return_gap,
        revenue_growth_contribution: revenue_growth_delta,
        gross_margin_contribution: gross_margin_delta,
        da_contribution: da_delta,
        capex_contribution: capex_delta,
        nwc_contribution: nwc_delta,
        multiple_contribution: multiple_delta,
        net_debt_contribution: net_debt_delta,
        residual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn worked_history() -> HistoricalSnapshot {
        HistoricalSnapshot::from_api_json(
            &[
                json!({"calendarYear":"2025","revenue":1000.0,"costOfRevenue":600.0,"sellingGeneralAndAdministrativeExpenses":100.0,"depreciationAndAmortization":50.0,"operatingIncome":200.0,"interestExpense":0.0,"incomeTaxExpense":40.0,"incomeBeforeTax":200.0,"netIncome":160.0,"weightedAverageShsOut":100.0}),
                json!({"calendarYear":"2024","revenue":1000.0,"costOfRevenue":600.0,"sellingGeneralAndAdministrativeExpenses":100.0,"depreciationAndAmortization":50.0,"operatingIncome":200.0,"interestExpense":0.0,"incomeTaxExpense":40.0,"incomeBeforeTax":200.0,"netIncome":160.0,"weightedAverageShsOut":100.0}),
            ],
            &[
                json!({"calendarYear":"2025","totalCurrentAssets":0.0,"totalCurrentLiabilities":0.0,"cashAndCashEquivalents":0.0,"longTermDebt":0.0,"netReceivables":0.0,"inventory":0.0,"accountsPayable":0.0,"totalStockholdersEquity":1000.0,"totalAssets":1000.0,"netPPE":0.0}),
                json!({"calendarYear":"2024","totalCurrentAssets":0.0,"totalCurrentLiabilities":0.0,"cashAndCashEquivalents":0.0,"longTermDebt":0.0,"netReceivables":0.0,"inventory":0.0,"accountsPayable":0.0,"totalStockholdersEquity":1000.0,"totalAssets":1000.0,"netPPE":0.0}),
            ],
            &[
                json!({"calendarYear":"2025","capitalExpenditure":-80.0,"dividendsPaid":0.0}),
                json!({"calendarYear":"2024","capitalExpenditure":-80.0,"dividendsPaid":0.0}),
            ],
            &[],
            &json!({}),
        )
    }

    /// Audit site 35: the multiple contribution is the per-share
    /// terminal-value delta from the terminal multiple coming in as
    /// `actual_multiple` on the H5 actual-hist base — (actual − implied)
    /// × last-year FCF per share. With the actual history equal to the
    /// projected history the base model IS the projection, so the oracle is
    /// Δmultiple × last_fcf / shares, computed here from the projection's
    /// own published outputs — never from `decompose_gap`. The former
    /// `× 10.0` placeholder is right only when per-share FCF equals 10
    /// (this fixture's is 130/100 = 1.3, so the placeholder read 20.0 vs
    /// the faithful 2.6).
    #[test]
    fn multiple_contribution_is_delta_multiple_times_fcf_per_share() {
        let hist = worked_history();
        let assumptions =
            ProjectionAssumptions::from_history(&hist, 0.10).expect("assumptions from history");
        let projected = project_financial_model(&hist, &assumptions).expect("projection");
        let last_fcf = projected
            .periods
            .last()
            .expect("projected periods")
            .free_cash_flow;
        let implied_multiple = projected.terminal_value / last_fcf;
        let multiple_expansion = 2.0;
        let actual_multiple = implied_multiple + multiple_expansion;
        let price = 100.0;

        let gap = decompose_gap(
            &projected,
            &assumptions,
            &hist,
            price,
            actual_multiple,
            projected.intrinsic_per_share,
            price,
        )
        .expect("gap decomposition");

        let expected = multiple_expansion * last_fcf / hist.shares_outstanding;
        assert!(
            (gap.multiple_contribution - expected).abs() < 1e-9,
            "multiple contribution {}, expected {expected}",
            gap.multiple_contribution
        );

        // Only the multiple changed (actual history == projected history), so
        // every other driver's contribution is the H5 base re-projection
        // against itself: zero.
        for (name, contribution) in [
            ("revenue_growth", gap.revenue_growth_contribution),
            ("gross_margin", gap.gross_margin_contribution),
            ("da", gap.da_contribution),
            ("capex", gap.capex_contribution),
            ("nwc", gap.nwc_contribution),
            ("net_debt", gap.net_debt_contribution),
        ] {
            assert!(
                contribution.abs() < 1e-9,
                "{name} contribution {contribution} should be zero when only the multiple changed"
            );
        }
    }
}
