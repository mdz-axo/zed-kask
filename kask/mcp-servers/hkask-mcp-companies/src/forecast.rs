//! Durable forecast snapshots and valuation-multiple helpers.
//!
//! `StoredForecast` is the persisted projection model that `forecast_record`
//! later decomposes against actual outcomes. The snapshot is a serde JSON
//! round-trip of the struct; `from_snapshot` reverses it. The
//! `projected_terminal_multiple` and `current_price_from_multiple` helpers
//! derive an implied terminal multiple and a price-from-multiple for the gap
//! decomposition in `forecast_record`.
//!
//! These items lived in the server-composition file (`hkask_mcp_companies.rs`)
//! but are pure domain functions with no MCP dependency. They live here so the
//! lib root is a true composition root (server struct + router + pin tests)
//! and the forecast-store logic concentrates next to its only test
//! (`stored_forecast_snapshot_reconstructs_decomposition_model`).

use crate::financial_model::{HistoricalSnapshot, ProjectedModel, ProjectionAssumptions};
use serde::{Deserialize, Serialize};

/// A stored forecast model for later decomposition during `forecast_record`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct StoredForecast {
    pub model: ProjectedModel,
    pub assumptions: ProjectionAssumptions,
    pub current_price: f64,
    pub intrinsic_per_share: f64,
}

impl StoredForecast {
    /// Serialize the forecast to a serde JSON snapshot for persistence.
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or(serde_json::Value::Null)
    }

    /// Reconstruct a forecast from a persisted snapshot. Fails (returns `Err`)
    /// for snapshots that aren't a full `StoredForecast` — e.g. the minimal
    /// pre-computed price-target snapshots `forecast_persist` writes, which
    /// carry no projected model. `forecast_record` treats that failure as
    /// "no decomposition" (Brier scoring still runs) and logs it rather than
    /// collapsing to `None` via `.ok()?` silently.
    pub fn from_snapshot(snapshot: &serde_json::Value) -> Result<Self, serde_json::Error> {
        serde_json::from_value(snapshot.clone())
    }
}

/// Extract the terminal multiple implied by the projected model: terminal
/// value divided by the last projected period's free cash flow. Returns 0.0
/// when there are no periods or the last FCF is non-positive (the division
/// would be undefined or misleading).
pub(crate) fn projected_terminal_multiple(model: &ProjectedModel) -> f64 {
    if let Some(last) = model.periods.last() {
        if last.free_cash_flow > 0.0 {
            model.terminal_value / last.free_cash_flow
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// Approximate a current price from a valuation multiple and the latest
/// historical free cash flow per share. Used by `forecast_record` to derive
/// the "actual" price the stored forecast's terminal multiple would imply
/// today, for the gap decomposition.
pub(crate) fn current_price_from_multiple(multiple: f64, hist: &HistoricalSnapshot) -> f64 {
    let latest_fcf =
        hist.latest_revenue() * hist.gross_margin() - hist.latest_da() - hist.latest_capex();
    if hist.shares_outstanding > 0.0 {
        (latest_fcf * multiple) / hist.shares_outstanding
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::financial_model::ProjectedLineItems;

    /// Round-trip a `StoredForecast` through `snapshot` → `from_snapshot` and
    /// confirm the projected model and its line items survive the serde
    /// round-trip. This is the decomposition contract: `forecast_record`
    /// reloads the stored snapshot to decompose actual outcomes against the
    /// original projections, so the model must reconstruct exactly.
    #[test]
    fn stored_forecast_snapshot_reconstructs_decomposition_model() {
        let stored = StoredForecast {
            model: ProjectedModel {
                periods: vec![ProjectedLineItems {
                    period: 1,
                    year: 2026.0,
                    revenue: 120.0,
                    cogs: 72.0,
                    gross_profit: 48.0,
                    da: 4.0,
                    ebit: 44.0,
                    tax: 9.2,
                    nopat: 34.8,
                    capex: 6.0,
                    change_in_nwc: 1.8,
                    free_cash_flow: 27.0,
                    discount_factor: 0.9,
                    present_value: 24.3,
                }],
                terminal_value: 270.0,
                terminal_pv: 135.0,
                enterprise_value: 159.3,
                net_debt: 10.0,
                equity_value: 149.3,
                intrinsic_per_share: 14.93,
            },
            assumptions: ProjectionAssumptions {
                stage1_years: 3,
                total_years: 10,
                discount_rate: 0.10,
                terminal_growth: 0.02,
                revenue_growth: 0.08,
                gross_margin: 0.40,
                da_to_revenue: 0.05,
                capex_to_revenue: 0.05,
                nwc_to_revenue: 0.02,
                tax_rate: 0.21,
            },
            current_price: 12.0,
            intrinsic_per_share: 14.93,
        };

        let reconstructed = StoredForecast::from_snapshot(&stored.snapshot()).unwrap();
        assert_eq!(reconstructed.model.periods.len(), 1);
        assert_eq!(reconstructed.model.periods[0].free_cash_flow, 27.0);
        assert_eq!(reconstructed.current_price, 12.0);
        assert_eq!(reconstructed.intrinsic_per_share, 14.93);
    }

    /// `from_snapshot` fails on a snapshot that isn't a `StoredForecast` —
    /// the pre-computed price-target snapshots `forecast_persist` writes
    /// carry no projected model. `forecast_record` treats this as "no
    /// decomposition" rather than silently collapsing to `None` via `.ok()?`.
    #[test]
    fn from_snapshot_rejects_non_forecast_snapshot() {
        let minimal = serde_json::json!({"kind": "precomputed_price_target"});
        assert!(StoredForecast::from_snapshot(&minimal).is_err());
    }

    /// `projected_terminal_multiple` returns 0.0 when the last FCF is
    /// non-positive (the division would be undefined or misleading).
    #[test]
    fn projected_terminal_multiple_non_positive_fcf_yields_zero() {
        let model = ProjectedModel {
            periods: vec![ProjectedLineItems {
                period: 1,
                year: 2026.0,
                revenue: 100.0,
                cogs: 60.0,
                gross_profit: 40.0,
                da: 5.0,
                ebit: 35.0,
                tax: 7.0,
                nopat: 28.0,
                capex: 10.0,
                change_in_nwc: 2.0,
                free_cash_flow: -5.0,
                discount_factor: 0.9,
                present_value: -4.5,
            }],
            terminal_value: 160.0,
            terminal_pv: 100.0,
            enterprise_value: 95.5,
            net_debt: 10.0,
            equity_value: 85.5,
            intrinsic_per_share: 8.55,
        };
        assert_eq!(projected_terminal_multiple(&model), 0.0);
    }

    /// `projected_terminal_multiple` returns terminal_value / last_fcf when
    /// the last FCF is positive.
    #[test]
    fn projected_terminal_multiple_positive_fcf() {
        let model = ProjectedModel {
            periods: vec![ProjectedLineItems {
                period: 1,
                year: 2026.0,
                revenue: 100.0,
                cogs: 60.0,
                gross_profit: 40.0,
                da: 5.0,
                ebit: 35.0,
                tax: 7.0,
                nopat: 28.0,
                capex: 10.0,
                change_in_nwc: 2.0,
                free_cash_flow: 20.0,
                discount_factor: 0.9,
                present_value: 18.0,
            }],
            terminal_value: 200.0,
            terminal_pv: 120.0,
            enterprise_value: 138.0,
            net_debt: 10.0,
            equity_value: 128.0,
            intrinsic_per_share: 12.8,
        };
        assert_eq!(projected_terminal_multiple(&model), 10.0);
    }

    /// `current_price_from_multiple` returns 0.0 when shares outstanding is
    /// zero (the division would be undefined).
    #[test]
    fn current_price_from_multiple_zero_shares_yields_zero() {
        let hist = HistoricalSnapshot {
            revenue: vec![("2024".to_string(), 100.0)],
            cogs: vec![("2024".to_string(), 60.0)],
            da: vec![("2024".to_string(), 5.0)],
            capex: vec![("2024".to_string(), 10.0)],
            current_assets: vec![],
            current_liabilities: vec![],
            cash: vec![],
            long_term_debt: vec![],
            shares_outstanding: 0.0,
            tax_rate: 0.21,
        };
        assert_eq!(current_price_from_multiple(15.0, &hist), 0.0);
    }
}
