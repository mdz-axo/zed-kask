//! WalletEnergyEstimator — Composite estimator with rJoule conversion awareness.
//!
//! Wraps the existing `CompositeEnergyEstimator` and adds `gas_per_rjoule`
//! for use by `WalletBackedBudget`. The estimator itself produces gas costs;
//! conversion to rJoules happens at reserve/settle time in `WalletBackedBudget`.
//!
//! # Calibration
//! The conversion rate can be adjusted at runtime via `calibrate()` based on
//! observed actual_gas / estimated_gas ratios from `GovernedTool` settlements.
//! This closes the Good Regulator feedback loop (P9).

use crate::composite_energy_estimator::CompositeEnergyEstimator;
use crate::energy_estimator::EnergyEstimator;
use serde_json::Value;

/// Energy estimator that wraps `CompositeEnergyEstimator` and carries
/// the gas→rJoule conversion rate for wallet-backed budgets.
///
/// The estimator produces dimensionless gas costs (same as the standard
/// estimator). The `gas_per_rjoule` field is consumed by `WalletBackedBudget`
/// at reserve/settle time to convert gas to rJoules for wallet debiting.
pub struct WalletEnergyEstimator {
    /// The underlying composite estimator (inference → token-based, others → table).
    inner: CompositeEnergyEstimator,
    /// Conversion rate: how many gas units equal 1 rJoule.
    /// Default: 250_000 gas = 1 rJ.
    pub gas_per_rjoule: u64,
    /// Exponential moving average (EMA) alpha for calibration smoothing.
    /// Default: 0.1 — each observation contributes 10% to the moving average.
    ema_alpha: f64,
    /// Current EMA of the observed actual/estimated gas ratio.
    /// None until first calibration observation.
    ema_ratio: Option<f64>,
}

impl WalletEnergyEstimator {
    /// Create a new WalletEnergyEstimator with the given conversion rate.
    pub fn new(gas_per_rjoule: u64) -> Self {
        Self::with_estimator(gas_per_rjoule, CompositeEnergyEstimator::new())
    }

    /// Create a WalletEnergyEstimator with a custom inner estimator.
    ///
    /// This allows wrapping a `CalibratedEnergyEstimator` or any pre-configured
    /// `CompositeEnergyEstimator` so per-server cost calibration and gas→rJoule
    /// calibration share the same gas-cost base.
    ///
    /// expect: "I can compose a wallet energy estimator from a pre-calibrated composite estimator so gas→rJoule conversion uses live costs"
    /// pre:  gas_per_rjoule > 0
    /// post: returns WalletEnergyEstimator with the supplied inner estimator
    pub fn with_estimator(gas_per_rjoule: u64, inner: CompositeEnergyEstimator) -> Self {
        Self {
            inner,
            gas_per_rjoule,
            ema_alpha: 0.1,
            ema_ratio: None,
        }
    }

    /// Calibrate the gas→rJoule conversion rate based on an observed
    /// actual_gas / estimated_gas ratio from a tool settlement.
    ///
    /// expect: "The wallet estimator self-calibrates from observed actual-vs-estimated gas ratios"
    /// \[P9\] Motivating: Homeostatic Self-Regulation — Good Regulator feedback loop closure
    /// \[P4\] Constraining: Clear Boundaries — threshold tolerance enforces boundary
    /// \[P7\] Constraining: Evolutionary Architecture — EMA parameters emerged from real usage
    /// pre:  observed_ratio > 0.0 (actual_gas / estimated_gas)
    /// post: ema_ratio updated via exponential moving average
    /// post: if ema_ratio deviates significantly from 1.0, gas_per_rjoule adjusted
    ///
    /// Uses an exponential moving average (EMA) to smooth observations.
    /// When the EMA ratio consistently exceeds 1.0 (systematic underestimation)
    /// or falls below 1.0 (systematic overestimation), `gas_per_rjoule` is
    /// adjusted to bring the ratio toward 1.0.
    ///
    /// # Returns
    /// `true` if `gas_per_rjoule` was adjusted, `false` if within tolerance.
    pub fn calibrate(&mut self, observed_ratio: f64) -> bool {
        // Clamp ratio to reasonable bounds (0.1 to 10.0)
        let ratio = observed_ratio.clamp(0.1, 10.0);

        // Update EMA
        let new_ema = match self.ema_ratio {
            Some(current) => self.ema_alpha * ratio + (1.0 - self.ema_alpha) * current,
            None => ratio, // first observation — initialize EMA
        };
        self.ema_ratio = Some(new_ema);

        // Adjust gas_per_rjoule if EMA deviates beyond tolerance (±20%)
        let tolerance: f64 = 0.2;
        if (new_ema - 1.0).abs() > tolerance {
            // Scale gas_per_rjoule by the EMA ratio to bring it toward 1.0
            // If EMA = 1.5 (actual 50% higher than estimated), increase rate by 50%
            // If EMA = 0.5 (actual 50% lower than estimated), decrease rate by 50%
            let adjusted = (self.gas_per_rjoule as f64 * new_ema) as u64;
            // Floor at 1 — gas_per_rjoule must be positive
            self.gas_per_rjoule = adjusted.max(1);
            true
        } else {
            false
        }
    }

    /// Current EMA ratio, or 1.0 if no observations yet.
    pub fn current_ratio(&self) -> f64 {
        self.ema_ratio.unwrap_or(1.0)
    }
}

impl EnergyEstimator for WalletEnergyEstimator {
    fn estimate_cost(&self, server: &str, tool: &str, args: &Value) -> u64 {
        self.inner.estimate_cost(server, tool, args)
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_wallet::GAS_PER_RJOULE;

    #[test]
    fn calibrate_first_observation_initializes_ema() {
        let mut estimator = WalletEnergyEstimator::new(GAS_PER_RJOULE);
        assert_eq!(estimator.current_ratio(), 1.0);

        // First observation: ratio = 1.5
        let adjusted = estimator.calibrate(1.5);
        assert!(adjusted, "ratio 1.5 exceeds 20% tolerance");
        assert_eq!(estimator.current_ratio(), 1.5);
        // gas_per_rjoule should be scaled: GAS_PER_RJOULE * 1.5
        assert_eq!(estimator.gas_per_rjoule, GAS_PER_RJOULE * 3 / 2);
    }

    #[test]
    fn calibrate_within_tolerance_no_adjustment() {
        let mut estimator = WalletEnergyEstimator::new(GAS_PER_RJOULE);
        // Ratio 1.1 is within ±20% tolerance
        let adjusted = estimator.calibrate(1.1);
        assert!(!adjusted, "ratio 1.1 is within 20% tolerance");
        assert_eq!(estimator.gas_per_rjoule, GAS_PER_RJOULE, "rate unchanged");
    }

    #[test]
    fn calibrate_ema_smooths_observations() {
        let mut estimator = WalletEnergyEstimator::new(GAS_PER_RJOULE);
        // First: ratio 2.0 → EMA = 2.0, rate = GAS_PER_RJOULE * 2
        estimator.calibrate(2.0);
        assert_eq!(estimator.gas_per_rjoule, GAS_PER_RJOULE * 2);

        // Second: ratio 1.0 → EMA = 0.1*1.0 + 0.9*2.0 = 1.9
        estimator.calibrate(1.0);
        let expected_ema = 0.1 * 1.0 + 0.9 * 2.0; // 1.9
        assert!((estimator.current_ratio() - expected_ema).abs() < 0.001);
    }

    #[test]
    fn calibrate_clamps_extreme_ratios() {
        let mut estimator = WalletEnergyEstimator::new(GAS_PER_RJOULE);
        // Ratio 0.001 → clamped to 0.1
        estimator.calibrate(0.001);
        assert_eq!(estimator.current_ratio(), 0.1);

        // Ratio 100.0 → clamped to 10.0
        let mut estimator2 = WalletEnergyEstimator::new(GAS_PER_RJOULE);
        estimator2.calibrate(100.0);
        assert_eq!(estimator2.current_ratio(), 10.0);
    }

    #[test]
    fn calibrate_floors_gas_per_rjoule_at_one() {
        let mut estimator = WalletEnergyEstimator::new(10);
        // Ratio 0.1 → EMA = 0.1, rate = 10 * 0.1 = 1 (floored)
        estimator.calibrate(0.1);
        assert_eq!(estimator.gas_per_rjoule, 1, "rate floored at 1");
    }
}
