//! Portfolio aggregation methods — weighted average calculations for
//! portfolio characteristics.
//!
//! Implements four aggregation methods used by institutional portfolio
//! analytics platforms (FactSet, Bloomberg, Morningstar):
//!
//! - **Weighted arithmetic mean** (default): Σ(wᵢ × xᵢ). The standard
//!   method, but biased upward for ratios like P/E.
//! - **Weighted harmonic mean**: 1 / Σ(wᵢ / xᵢ). Correct for ratios —
//!   gives equal weight to each unit of the denominator. Morningstar
//!   switched to this for P/E, P/B, P/S, P/CF in 2005.
//! - **Weighted median**: the value where cumulative weight crosses 50%.
//!   Robust to outliers.
//! - **Winsorized weighted mean**: clamp at 5th/95th percentiles, then
//!   weighted arithmetic mean. Bloomberg uses this for index descriptors.
//!
//! References:
//! - Agrrawal, Borgman, Clark, Strong (2010). "Using the Price-to-Earnings
//!   Harmonic Mean to Improve Firm Valuation Estimates." JFED.
//! - CFA Level II, Reading 25: "Market-Based Valuation: Price and Enterprise
//!   Value Multiples" — LOS 25(q).
//! - Morningstar Methodology Note (2005): harmonic weighted average for
//!   trailing price ratios.
//! - Bloomberg Index Methodology: winsorization at 5th/95th percentiles
//!   for Quality and Value-Growth descriptors.

use crate::types::AggregationMethod;

/// A single holding's contribution to a characteristic: its weight and
/// the raw value of the metric being aggregated.
#[derive(Debug, Clone)]
pub struct WeightedValue {
    pub weight: f64,
    pub value: f64,
}

/// Aggregate a set of weighted values using the specified method.
///
/// Returns `None` if the aggregation is undefined (e.g., harmonic mean
/// with all-zero values, or an empty input set).
pub fn aggregate(values: &[WeightedValue], method: &AggregationMethod) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    match method {
        AggregationMethod::WeightedArithmetic => weighted_arithmetic(values),
        AggregationMethod::WeightedHarmonic => weighted_harmonic(values),
        AggregationMethod::WeightedMedian => weighted_median(values),
        AggregationMethod::Winsorized => winsorized_weighted_arithmetic(values),
    }
}

/// Weighted arithmetic mean: Σ(wᵢ × xᵢ) / Σ(wᵢ).
///
/// The standard method. Biased upward for ratios because it gives
/// greater weight to high values.
fn weighted_arithmetic(values: &[WeightedValue]) -> Option<f64> {
    let total_weight: f64 = values.iter().map(|v| v.weight).sum();
    if total_weight <= 0.0 {
        return None;
    }
    let sum: f64 = values.iter().map(|v| v.weight * v.value).sum();
    Some(sum / total_weight)
}

/// Weighted harmonic mean: 1 / Σ(wᵢ / xᵢ) × Σ(wᵢ).
///
/// Correct for averaging ratios (P/E, P/B, P/S). Gives equal weight
/// to each unit of the denominator. Cannot handle zero or negative
/// values — those are filtered out before aggregation.
fn weighted_harmonic(values: &[WeightedValue]) -> Option<f64> {
    let valid: Vec<&WeightedValue> = values.iter().filter(|v| v.value > 0.0).collect();
    if valid.is_empty() {
        return None;
    }
    let total_weight: f64 = valid.iter().map(|v| v.weight).sum();
    if total_weight <= 0.0 {
        return None;
    }
    let sum_reciprocals: f64 = valid.iter().map(|v| v.weight / v.value).sum();
    if sum_reciprocals <= 0.0 {
        return None;
    }
    Some(total_weight / sum_reciprocals)
}

/// Weighted median: the value where cumulative weight crosses 50%.
///
/// Sort by value, then walk the cumulative weight until it reaches
/// half the total. Robust to outliers — a single extreme P/E won't
/// move the median.
fn weighted_median(values: &[WeightedValue]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    let total_weight: f64 = values.iter().map(|v| v.weight).sum();
    if total_weight <= 0.0 {
        return None;
    }
    let mut sorted: Vec<&WeightedValue> = values.iter().collect();
    sorted.sort_by(|a, b| {
        a.value
            .partial_cmp(&b.value)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let half = total_weight / 2.0;
    let mut cumulative = 0.0;
    for v in &sorted {
        cumulative += v.weight;
        if cumulative >= half {
            return Some(v.value);
        }
    }
    // Fallback: return the last value (shouldn't happen unless floating-point
    // rounding prevented the cumulative from reaching exactly half).
    sorted.last().map(|v| v.value)
}

/// Winsorized weighted arithmetic mean: clamp values at the 5th and 95th
/// percentiles, then compute the weighted arithmetic mean.
///
/// Bloomberg winsorizes descriptors at the 5th and 95th percentiles for
/// Quality and Value-Growth indices. This reduces the influence of outliers
/// without excluding them entirely.
fn winsorized_weighted_arithmetic(values: &[WeightedValue]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    // Compute the 5th and 95th percentile values (unweighted — by count).
    let mut sorted_values: Vec<f64> = values.iter().map(|v| v.value).collect();
    sorted_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted_values.len();
    let p5_idx = ((n as f64) * 0.05).ceil() as usize;
    let p95_idx = ((n as f64) * 0.95).ceil() as usize;
    let p5_idx = p5_idx.min(n.saturating_sub(1));
    let p95_idx = p95_idx.min(n.saturating_sub(1));
    let p5 = sorted_values[p5_idx];
    let p95 = sorted_values[p95_idx];

    // Clamp each value to [p5, p95], then compute weighted arithmetic mean.
    let clamped: Vec<WeightedValue> = values
        .iter()
        .map(|v| WeightedValue {
            weight: v.weight,
            value: v.value.clamp(p5, p95),
        })
        .collect();
    weighted_arithmetic(&clamped)
}
