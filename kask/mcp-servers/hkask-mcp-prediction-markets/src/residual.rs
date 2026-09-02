//! Residual risk decomposition (T15).
//!
//! A niche event's log-odds changes are regressed on its base event's
//! log-odds changes over overlapping windows: the slope (β) is the event's
//! exposure to the base, the residual series is what's idiosyncratic.
//! Linear co-movement in log-odds is a deliberate first-approximation model
//! choice (integration report §5.6), not a fact — the output carries
//! `r_squared` and `observations` so consumers can judge fit quality.
//!
//! Refusal gate: fewer than MIN_OBSERVATIONS overlapping points ⇒
//! `insufficient_overlap`, never a fabricated residual.

use hkask_forecast::{from_log_odds, log_odds};

/// Minimum overlapping observation pairs for a regression to be meaningful.
pub const MIN_OBSERVATIONS: usize = 10;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ResidualAnalysis {
    /// Slope: niche-event log-odds change per unit base-event log-odds change.
    pub beta: f64,
    /// Intercept in log-odds space.
    pub alpha: f64,
    /// Fraction of niche variance explained by the base event.
    pub r_squared: f64,
    /// Overlapping observation count — the consumer's fit-quality signal.
    pub observations: usize,
    /// Most recent residual (niche log-odds minus fitted value) — the
    /// event's current idiosyncratic deviation from its base.
    pub latest_residual: f64,
}

/// Aligned price series for niche + base events: (niche_price, base_price)
/// at each shared observation time.
pub fn residual_analysis(observations: &[(f64, f64)]) -> Option<ResidualAnalysis> {
    if observations.len() < MIN_OBSERVATIONS {
        return None;
    }
    // Log-odds changes.
    let niche: Vec<f64> = observations
        .windows(2)
        .map(|w| log_odds(w[1].0) - log_odds(w[0].0))
        .collect();
    let base: Vec<f64> = observations
        .windows(2)
        .map(|w| log_odds(w[1].1) - log_odds(w[0].1))
        .collect();
    let n = niche.len();
    if n < MIN_OBSERVATIONS - 1 {
        return None;
    }

    let mean_x: f64 = base.iter().sum::<f64>() / n as f64;
    let mean_y: f64 = niche.iter().sum::<f64>() / n as f64;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    let mut syy = 0.0;
    for i in 0..n {
        let dx = base[i] - mean_x;
        let dy = niche[i] - mean_y;
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    if sxx < 1e-12 {
        return None; // base never moved — no exposure estimable
    }
    let beta = sxy / sxx;
    let alpha = mean_y - beta * mean_x;
    let r_squared = if syy < 1e-12 {
        0.0
    } else {
        (sxy * sxy) / (sxx * syy)
    };

    // Latest residual from the final observation levels.
    let (last_niche, last_base) = observations[observations.len() - 1];
    let fitted = from_log_odds(log_odds(last_base) * beta);
    let latest_residual = last_niche - fitted;

    Some(ResidualAnalysis {
        beta,
        alpha,
        r_squared,
        observations: n,
        latest_residual,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prices whose log-odds follow the given levels exactly.
    fn prices_from_log_odds(levels: &[f64]) -> Vec<f64> {
        levels.iter().map(|l| from_log_odds(*l)).collect()
    }

    #[test]
    fn insufficient_overlap_is_none() {
        let observations: Vec<(f64, f64)> =
            (0..9).map(|i| (0.4 + f64::from(i) * 0.01, 0.5)).collect();
        assert!(
            residual_analysis(&observations).is_none(),
            "fewer than MIN_OBSERVATIONS must refuse, never fabricate a residual"
        );
    }

    #[test]
    fn perfectly_proportional_series_recovers_beta_and_fit() {
        // Niche log-odds = 2 × base log-odds at every step ⇒ β = 2, R² = 1,
        // and the latest residual is zero (niche sits exactly on its base).
        // The level steps vary — a uniform ramp would give constant log-odds
        // changes (zero variance in the change series), which the analysis
        // correctly refuses as "base never moved".
        let base_levels: Vec<f64> = vec![
            -0.50, -0.30, -0.45, -0.05, 0.10, -0.10, 0.25, 0.05, 0.40, 0.15, 0.55, 0.30,
        ];
        let niche_levels: Vec<f64> = base_levels.iter().map(|l| 2.0 * l).collect();
        let base = prices_from_log_odds(&base_levels);
        let niche = prices_from_log_odds(&niche_levels);
        let observations: Vec<(f64, f64)> =
            niche.iter().zip(&base).map(|(n, b)| (*n, *b)).collect();
        let analysis = residual_analysis(&observations).expect("sufficient overlap");
        assert!(
            (analysis.beta - 2.0).abs() < 1e-9,
            "beta is the niche-per-base log-odds slope"
        );
        assert!(
            (analysis.r_squared - 1.0).abs() < 1e-9,
            "exact proportionality is fully explained"
        );
        assert_eq!(
            analysis.observations, 11,
            "windows of 2 over 12 observations"
        );
        assert!(
            analysis.latest_residual.abs() < 1e-9,
            "proportional series has no idiosyncratic deviation"
        );
    }

    #[test]
    fn base_that_never_moved_is_none() {
        let observations: Vec<(f64, f64)> =
            (0..12).map(|i| (0.4 + f64::from(i) * 0.01, 0.5)).collect();
        assert!(
            residual_analysis(&observations).is_none(),
            "zero base variance ⇒ no exposure estimable"
        );
    }
}
