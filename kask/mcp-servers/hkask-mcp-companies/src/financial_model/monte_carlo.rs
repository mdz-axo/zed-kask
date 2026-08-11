//! Monte Carlo DCF simulation — runs N simulations of the projection model
//! with randomly varied assumptions to produce a distribution of intrinsic
//! values. Includes percentile calculation and sensitivity-range validation.
//!
//! Extracted from `financial_model.rs` (deep-module split: the stochastic
//! analysis is independent of the deterministic projection, equity-duration,
//! gap-decomposition, and sensitivity-analysis concerns).

use serde::Serialize;

use super::{HistoricalSnapshot, ProjectionAssumptionError, ProjectionAssumptions, project_model};
/// Distribution of intrinsic values from Monte Carlo simulation.
#[derive(Debug, Clone, Serialize)]
pub struct MonteCarloResult {
    pub simulations: usize,
    pub base_intrinsic: f64,
    pub mean_intrinsic: f64,
    pub std_dev: f64,
    pub min_intrinsic: f64,
    pub p10: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p90: f64,
    pub max_intrinsic: f64,
    /// Probability intrinsic exceeds current price (if price > 0)
    pub prob_undervalued: f64,
    /// Histogram buckets: [(label, count)]
    pub histogram: Vec<(String, usize)>,
}

/// Range specification for one assumption in Monte Carlo simulation.
pub struct McRange {
    pub revenue_growth: f64,
    pub gross_margin: f64,
    pub da_to_revenue: f64,
    pub capex_to_revenue: f64,
    pub nwc_to_revenue: f64,
    pub discount_rate: f64,
}

impl McRange {
    /// Validate Monte Carlo perturbation widths before sampling.
    pub fn validate(&self) -> Result<(), ProjectionAssumptionError> {
        for (field, value) in [
            ("range_revenue_growth", self.revenue_growth),
            ("range_gross_margin", self.gross_margin),
            ("range_da", self.da_to_revenue),
            ("range_capex", self.capex_to_revenue),
            ("range_nwc", self.nwc_to_revenue),
            ("range_discount_rate", self.discount_rate),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(ProjectionAssumptionError::NotFiniteOrOutOfRange {
                    field,
                    min: 0.0,
                    max: 1.0,
                });
            }
        }
        Ok(())
    }
}

/// Validate the relative sensitivity range before varying assumptions.
pub fn validate_sensitivity_range(range_pct: f64) -> Result<(), ProjectionAssumptionError> {
    if !range_pct.is_finite() || !(0.0..=1.0).contains(&range_pct) {
        return Err(ProjectionAssumptionError::NotFiniteOrOutOfRange {
            field: "range_pct",
            min: 0.0,
            max: 1.0,
        });
    }
    Ok(())
}

impl Default for McRange {
    fn default() -> Self {
        Self {
            revenue_growth: 0.03,
            gross_margin: 0.03,
            da_to_revenue: 0.01,
            capex_to_revenue: 0.01,
            nwc_to_revenue: 0.02,
            discount_rate: 0.01,
        }
    }
}

/// The simulation-count bounds enforced by [`monte_carlo_dcf`].
pub const MC_MIN_SIMULATIONS: usize = 100;
pub const MC_MAX_SIMULATIONS: usize = 10_000;

/// Run N Monte Carlo simulations with randomized assumptions within +/- range.
///
/// `simulations` is clamped to `[MC_MIN_SIMULATIONS, MC_MAX_SIMULATIONS]` here
/// rather than at the call site, so the non-empty invariant the histogram and
/// percentile computations rely on holds for every caller of this public
/// function. `MonteCarloResult::simulations` reports the count actually run.
pub fn monte_carlo_dcf(
    hist: &HistoricalSnapshot,
    base_assumptions: &ProjectionAssumptions,
    simulations: usize,
    ranges: &McRange,
    current_price: f64,
    rng: &mut impl rand::Rng,
) -> MonteCarloResult {
    let simulations = simulations.clamp(MC_MIN_SIMULATIONS, MC_MAX_SIMULATIONS);
    let base = project_model(hist, base_assumptions, current_price);
    let mut values: Vec<f64> = Vec::with_capacity(simulations);

    for _ in 0..simulations {
        let mut a = base_assumptions.clone();
        a.revenue_growth =
            sample_uniform(rng, a.revenue_growth, ranges.revenue_growth).clamp(-0.50, 1.00);
        a.gross_margin = sample_uniform(rng, a.gross_margin, ranges.gross_margin).clamp(0.05, 0.95);
        a.da_to_revenue =
            sample_uniform(rng, a.da_to_revenue, ranges.da_to_revenue).clamp(0.0, 0.20);
        a.capex_to_revenue =
            sample_uniform(rng, a.capex_to_revenue, ranges.capex_to_revenue).clamp(0.0, 0.30);
        a.nwc_to_revenue =
            sample_uniform(rng, a.nwc_to_revenue, ranges.nwc_to_revenue).clamp(-0.20, 0.50);
        a.discount_rate = sample_uniform(rng, a.discount_rate, ranges.discount_rate)
            .clamp((a.terminal_growth + 0.0001).max(0.05), 0.30);
        values.push(project_model(hist, &a, current_price).intrinsic_per_share);
    }

    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    let mean = values.iter().sum::<f64>() / n as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n as f64;
    let std_dev = variance.sqrt();

    // Histogram with 10 buckets
    let min_val = values[0];
    let max_val = values[n - 1];
    let bucket_width = (max_val - min_val) / 10.0;
    let mut histogram: Vec<(String, usize)> = Vec::new();
    if bucket_width > 0.0 {
        for i in 0..10 {
            let lo = min_val + i as f64 * bucket_width;
            let hi = lo + bucket_width;
            let count = values
                .iter()
                .filter(|&&v| v >= lo && (i == 9 || v < hi))
                .count();
            histogram.push((format!("{:.0}-{:.0}", lo, hi), count));
        }
    }

    let prob_undervalued = if current_price > 0.0 {
        values.iter().filter(|&&v| v > current_price).count() as f64 / n as f64
    } else {
        0.0
    };

    MonteCarloResult {
        simulations: n,
        base_intrinsic: base.intrinsic_per_share,
        mean_intrinsic: mean,
        std_dev,
        min_intrinsic: values[0],
        p10: percentile(&values, 0.10),
        p25: percentile(&values, 0.25),
        median: percentile(&values, 0.50),
        p75: percentile(&values, 0.75),
        p90: percentile(&values, 0.90),
        max_intrinsic: values[n - 1],
        prob_undervalued,
        histogram,
    }
}

fn sample_uniform(rng: &mut impl rand::Rng, center: f64, range: f64) -> f64 {
    // A zero perturbation width is valid input (McRange::validate accepts
    // 0.0..=1.0) and means "no perturbation." random_range on an empty
    // lo..hi panics, so return the center directly.
    if range == 0.0 {
        return center;
    }
    let lo = center - range;
    let hi = center + range;
    rng.random_range(lo..hi)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = (p * (sorted.len() - 1) as f64).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}
