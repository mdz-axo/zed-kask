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

/// Resolve the current price for valuation tools. FMP profiles carry a
/// `price` field; EODHD-routed profiles (every exchange-qualified symbol,
/// e.g. `DNB.OL`) do not — the stock quote's `close` is the fallback, in the
/// listing currency, which is consistent with the local-currency financials
/// the valuation models consume (live-observed 2026-09-10: `expectations_gap`
/// and `reverse_dcf` returned no market-implied growth for every
/// exchange-qualified symbol because the profile price was the only source).
///
/// Returns the price and its source ("profile" or "stock_quote"), or `None`
/// when neither surface has a positive price.
pub(crate) fn resolve_current_price(
    profile: &serde_json::Value,
    quote: Option<&serde_json::Value>,
) -> Option<(f64, &'static str)> {
    let profile_price = profile
        .as_array()
        .and_then(|entries| entries.first())
        .and_then(|entry| entry.get("price"))
        .and_then(|value| value.as_f64())
        .filter(|price| *price > 0.0);
    if let Some(price) = profile_price {
        return Some((price, "profile"));
    }
    quote_close(quote).map(|price| (price, "stock_quote"))
}

/// The positive current price of a raw stock-quote payload (object or
/// single-entry array shape). EODHD real-time quotes carry `close`; FMP
/// quotes carry `price` — both keys are accepted.
fn quote_close(quote: Option<&serde_json::Value>) -> Option<f64> {
    let entry = match quote {
        Some(serde_json::Value::Object(_)) => quote,
        Some(serde_json::Value::Array(entries)) => entries.first(),
        _ => None,
    };
    entry
        .and_then(|quote| {
            quote
                .get("close")
                .or_else(|| quote.get("price"))
                .and_then(|value| value.as_f64())
        })
        .filter(|price| *price > 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolve_current_price_prefers_profile_price() {
        let profile = json!([{"price": 30.0, "marketCap": 3_000_000_000.0}]);
        let quote = json!({"close": 31.5});
        assert_eq!(
            resolve_current_price(&profile, Some(&quote)),
            Some((30.0, "profile"))
        );
    }

    /// expect: EODHD-routed profiles (every exchange-qualified symbol) carry
    /// no `price` field — the stock quote's close is the fallback.
    #[test]
    fn resolve_current_price_falls_back_to_quote_close() {
        let profile = json!([{"marketCap": 3_000_000_000.0, "sharesOutstanding": 100_000_000.0}]);
        let quote = json!({"code": "DNB.OL", "close": 318.9});
        assert_eq!(
            resolve_current_price(&profile, Some(&quote)),
            Some((318.9, "stock_quote"))
        );
    }

    /// expect: FMP quotes carry `price` instead of `close` — both keys are
    /// accepted on the quote surface.
    #[test]
    fn resolve_current_price_accepts_fmp_quote_price_key() {
        let profile = json!([{"marketCap": 3_000_000_000.0}]);
        let quote = json!({"symbol": "ACME", "price": 42.0});
        assert_eq!(
            resolve_current_price(&profile, Some(&quote)),
            Some((42.0, "stock_quote"))
        );
    }

    /// expect: a non-positive or absent price on both surfaces resolves to
    /// None — never a fabricated zero.
    #[test]
    fn resolve_current_price_rejects_nonpositive_prices() {
        let profile = json!([{"price": 0.0}]);
        assert_eq!(resolve_current_price(&profile, None), None);
        let profile = json!([{"marketCap": 1.0}]);
        let quote = json!({"close": -5.0});
        assert_eq!(resolve_current_price(&profile, Some(&quote)), None);
    }
}
