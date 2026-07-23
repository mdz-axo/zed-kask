//! Predictive regulation via a moving-average digital twin.
//!
//! Fermi's `dynamics` crate runs ODE-based simulations of coupled systems.
//! `MovingAverageExtrapolator` predicts future metric values so the regulation
//! loop can act *before* deviation, not after.
//!
//! ## Future (Fermi-style ODE models)
//!
//! - Energy budget model: `dE/dt = -consumption_rate + replenishment_rate`
//! - Queue depth model: `dQ/dt = arrival_rate - processing_rate`

use crate::types::loops::SignalMetric;

/// A predicted future state for a single metric.
#[derive(Debug, Clone)]
pub struct MetricPrediction {
    /// Trend direction: +1 = rising, -1 = falling, 0 = flat.
    pub trend: f64,
    /// Whether the prediction is reliable (enough data for the model).
    pub reliable: bool,
    /// Ticks until the metric crosses its set-point (None if not approaching).
    pub ticks_to_threshold: Option<u64>,
}

/// Simple moving-average extrapolator.
///
/// Fits a linear trend to the last N observations and projects forward.
/// No learning, no configuration — always available as a baseline.
pub struct MovingAverageExtrapolator {
    /// Per-metric observation history.
    history: std::sync::Mutex<std::collections::HashMap<SignalMetric, Vec<f64>>>,
    /// Number of observations to use for trend fitting.
    window: usize,
}

impl MovingAverageExtrapolator {
    pub fn new(window: usize) -> Self {
        Self {
            history: std::sync::Mutex::new(std::collections::HashMap::new()),
            window: window.max(3), // minimum 3 points for a meaningful trend
        }
    }

    /// Record a new observation for a metric.
    pub fn observe(&self, metric: SignalMetric, value: f64) {
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let entry = history.entry(metric).or_default();
        entry.push(value);
        if entry.len() > self.window {
            entry.remove(0);
        }
    }

    /// Predict the state of a metric relative to its set-point.
    pub fn predict(&self, metric: SignalMetric, current: f64, set_point: f64) -> MetricPrediction {
        let history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let obs = history.get(&metric);
        let n = obs.map(|v| v.len()).unwrap_or(0);

        if n < 3 {
            return MetricPrediction {
                trend: 0.0,
                reliable: false,
                ticks_to_threshold: None,
            };
        }

        let values = obs.unwrap();
        let last_n = if values.len() > self.window {
            &values[values.len() - self.window..]
        } else {
            values.as_slice()
        };

        // Simple linear regression: y = a + b*x
        let n_f = last_n.len() as f64;
        let sum_x: f64 = (0..last_n.len()).map(|i| i as f64).sum();
        let sum_y: f64 = last_n.iter().sum();
        let sum_xy: f64 = last_n.iter().enumerate().map(|(i, y)| i as f64 * y).sum();
        let sum_xx: f64 = (0..last_n.len()).map(|i| (i as f64).powi(2)).sum();

        let denominator = n_f * sum_xx - sum_x * sum_x;
        let trend = if denominator.abs() > f64::EPSILON {
            (n_f * sum_xy - sum_x * sum_y) / denominator
        } else {
            0.0
        };

        let ticks_to_threshold = if trend.abs() > f64::EPSILON {
            let gap = set_point - current;
            let ticks = gap / trend;
            if ticks > 0.0 && ticks.is_finite() {
                Some(ticks.ceil() as u64)
            } else {
                None
            }
        } else {
            None
        };

        MetricPrediction {
            trend,
            reliable: true,
            ticks_to_threshold,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_make_predictions_reliable() {
        let simulator = MovingAverageExtrapolator::new(3);
        simulator.observe(SignalMetric::ErrorRate, 0.1);
        simulator.observe(SignalMetric::ErrorRate, 0.2);
        simulator.observe(SignalMetric::ErrorRate, 0.3);

        let prediction = simulator.predict(SignalMetric::ErrorRate, 0.3, 0.5);

        assert!(prediction.reliable);
        assert!(prediction.trend > 0.0);
        assert_eq!(prediction.ticks_to_threshold, Some(2));
    }
}
