//! Structural volatility model for prediction-market contracts (DR-AS, arXiv:2607.08199).
//!
//! Decomposes one-step conditional variance into two economic channels:
//! - **Deadline-Resolution (DR):** Wright-Fisher diffusion `p(1 − p)/τ · Δ`.
//! - **Adverse-Selection (AS):** Glosten-Milgrom order-flow variance `K · ν(V) · s²/4 · Δ`.

use serde::{Deserialize, Serialize};

/// Activity proxy ν(V). Xi et al. (arXiv:2607.08199) found √V statistically best.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ActivityProxy {
    /// ν(V) = 1. Isolates the spread channel.
    Constant,
    /// ν(V) = log(1+V). Concave; diminishing marginal effect of activity.
    LogOnePlusVolume,
    /// ν(V) = √V. The paper's best-performing activity proxy.
    #[default]
    SqrtVolume,
    /// ν(V) = V. Linear; volume proportional to information-event arrival.
    LinearVolume,
}

impl ActivityProxy {
    /// Evaluate ν(V) for a given volume.
    #[must_use]
    pub fn evaluate(self, volume: f64) -> f64 {
        let v = volume.max(0.0);
        match self {
            Self::Constant => 1.0,
            Self::LogOnePlusVolume => (1.0 + v).ln(),
            Self::SqrtVolume => v.sqrt(),
            Self::LinearVolume => v,
        }
    }
}

/// Inputs to the DR-AS volatility model observed at the forecast origin.
#[derive(Debug, Clone, Copy)]
pub struct VolatilityInputs {
    /// Prediction-market price (YES probability), in `[0, 1]`.
    pub price: f64,
    /// Time to resolution in hours (`> 0`).
    pub hours_to_resolution: f64,
    /// Bid-ask spread in price units `[0, 1]`. `None` contributes 0 to AS.
    pub spread: Option<f64>,
    /// Trading volume during the observation window (`0` when no trades).
    pub volume: f64,
}

/// DR-AS model configuration (`K` scale and activity proxy).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DrasConfig {
    /// Nonnegative adverse-selection scale parameter `K ≥ 0`.
    pub k: f64,
    /// Activity proxy ν(V).
    pub activity_proxy: ActivityProxy,
}

impl Default for DrasConfig {
    fn default() -> Self {
        Self {
            k: 0.12,
            activity_proxy: ActivityProxy::SqrtVolume,
        }
    }
}

/// Closed-form DR-AS volatility forecast result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VolatilityForecast {
    /// Conditional standard deviation `h = √(h²)`.
    pub conditional_volatility: f64,
    /// Conditional variance `h²`.
    pub conditional_variance: f64,
    /// Deadline-resolution variance component `p(1 − p)/τ · Δ`.
    pub dr_variance: f64,
    /// Adverse-selection variance component `K · ν(V) · s²/4 · Δ`.
    pub as_variance: f64,
    /// Evaluated activity proxy `ν(V)`.
    pub activity_value: f64,
    /// 95% prediction interval `[lower, upper]` clipped to `[0, 1]`.
    pub interval_95: (f64, f64),
    /// Configuration applied.
    pub config: DrasConfig,
}

/// Compute the DR-AS structural volatility forecast over horizon `delta_hours`.
///
/// Returns `None` when any input is degenerate (price outside `[0, 1]`, non-positive
/// or non-finite `hours_to_resolution` or `delta_hours`, negative or non-finite `K`
/// or `spread`, or non-finite `volume`).
#[must_use]
pub fn forecast(
    inputs: VolatilityInputs,
    config: DrasConfig,
    delta_hours: f64,
) -> Option<VolatilityForecast> {
    let p = inputs.price;
    let positive = |x: f64| x.is_finite() && x > 0.0;
    let nonnegative = |x: f64| x.is_finite() && x >= 0.0;
    if !(0.0..=1.0).contains(&p)
        || !positive(inputs.hours_to_resolution)
        || !positive(delta_hours)
        || !nonnegative(config.k)
        || !inputs.spread.is_none_or(nonnegative)
        || !inputs.volume.is_finite()
    {
        return None;
    }

    let tau = inputs.hours_to_resolution;
    let delta = delta_hours;

    let dr_variance = p * (1.0 - p) / tau * delta;
    let activity_value = config.activity_proxy.evaluate(inputs.volume);
    let spread = inputs.spread.unwrap_or(0.0);
    let as_variance = config.k * activity_value * (spread * spread / 4.0) * delta;

    let conditional_variance = dr_variance + as_variance;
    let conditional_volatility = conditional_variance.sqrt();

    let z = 1.959963984540054;
    let lower = (p - z * conditional_volatility).max(0.0);
    let upper = (p + z * conditional_volatility).min(1.0);

    Some(VolatilityForecast {
        conditional_volatility,
        conditional_variance,
        dr_variance,
        as_variance,
        activity_value,
        interval_95: (lower, upper),
        config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dras_as_channel_adds_to_dr() {
        let inputs = VolatilityInputs {
            price: 0.5,
            hours_to_resolution: 168.0,
            spread: Some(0.04),
            volume: 1000.0,
        };
        let fc = forecast(inputs, DrasConfig::default(), 1.0).expect("valid inputs");
        assert!(fc.as_variance > 0.0);
        assert!(fc.conditional_variance > fc.dr_variance);
        assert!((fc.conditional_volatility - fc.conditional_variance.sqrt()).abs() < 1e-12);
    }

    #[test]
    fn dras_no_spread_means_no_as_channel() {
        let inputs = VolatilityInputs {
            price: 0.5,
            hours_to_resolution: 168.0,
            spread: None,
            volume: 1000.0,
        };
        let fc = forecast(inputs, DrasConfig::default(), 1.0).expect("valid inputs");
        assert!(fc.as_variance.abs() < 1e-15);
        assert!((fc.conditional_variance - fc.dr_variance).abs() < 1e-15);
    }

    #[test]
    fn dras_interval_clipped_to_unit_range() {
        let inputs = VolatilityInputs {
            price: 0.99,
            hours_to_resolution: 1.0,
            spread: Some(0.10),
            volume: 5000.0,
        };
        let fc = forecast(inputs, DrasConfig::default(), 1.0).expect("valid inputs");
        let (lo, hi) = fc.interval_95;
        assert!(lo >= 0.0 && hi <= 1.0 && lo < hi);
    }

    #[test]
    fn dras_sqrt_volume_is_best_activity_proxy_per_paper() {
        let config = DrasConfig::default();
        assert_eq!(config.activity_proxy, ActivityProxy::SqrtVolume);
        assert!((config.activity_proxy.evaluate(1000.0) - 31.622776).abs() < 0.01);
    }

    #[test]
    fn activity_proxy_evaluates_correctly() {
        let v = 100.0;
        assert_eq!(ActivityProxy::Constant.evaluate(v), 1.0);
        assert!((ActivityProxy::LogOnePlusVolume.evaluate(v) - (101.0_f64).ln()).abs() < 1e-12);
        assert!((ActivityProxy::SqrtVolume.evaluate(v) - 10.0).abs() < 1e-12);
        assert_eq!(ActivityProxy::LinearVolume.evaluate(v), 100.0);
    }

    #[test]
    fn dras_rejects_degenerate_and_negative_scale_spread_and_volume() {
        let inputs = VolatilityInputs {
            price: 0.5,
            hours_to_resolution: 24.0,
            spread: Some(0.04),
            volume: 1000.0,
        };
        let with_k = |k: f64| DrasConfig {
            k,
            ..DrasConfig::default()
        };
        assert!(
            forecast(
                VolatilityInputs {
                    price: 1.5,
                    ..inputs
                },
                DrasConfig::default(),
                1.0
            )
            .is_none()
        );
        assert!(
            forecast(
                VolatilityInputs {
                    hours_to_resolution: 0.0,
                    ..inputs
                },
                DrasConfig::default(),
                1.0
            )
            .is_none()
        );
        assert!(forecast(inputs, DrasConfig::default(), 0.0).is_none());
        assert!(forecast(inputs, with_k(-100.0), 1.0).is_none());
        assert!(forecast(inputs, with_k(f64::NAN), 1.0).is_none());
        assert!(forecast(inputs, with_k(0.0), 1.0).is_some());
        assert!(
            forecast(
                VolatilityInputs {
                    spread: Some(-0.04),
                    ..inputs
                },
                DrasConfig::default(),
                1.0
            )
            .is_none()
        );
        assert!(
            forecast(
                VolatilityInputs {
                    volume: f64::INFINITY,
                    ..inputs
                },
                DrasConfig::default(),
                1.0
            )
            .is_none()
        );
        assert!(
            forecast(
                VolatilityInputs {
                    hours_to_resolution: f64::NAN,
                    ..inputs
                },
                DrasConfig::default(),
                1.0
            )
            .is_none()
        );
    }

    #[test]
    fn dras_scales_linearly_with_horizon_and_monotonically_with_k() {
        let inputs = VolatilityInputs {
            price: 0.5,
            hours_to_resolution: 168.0,
            spread: Some(0.04),
            volume: 1000.0,
        };
        let config = DrasConfig::default();
        let h1 = forecast(inputs, config, 1.0).expect("1h");
        let h4 = forecast(inputs, config, 4.0).expect("4h");
        assert!((h4.conditional_variance / h1.conditional_variance - 4.0).abs() < 0.01);
        assert!(h1.dr_variance > 0.001 && h1.as_variance > 0.0005);
        assert!(h1.conditional_volatility > 0.03 && h1.conditional_volatility < 0.10);

        let zero_vol = forecast(
            VolatilityInputs {
                volume: 0.0,
                ..inputs
            },
            config,
            1.0,
        )
        .expect("0 vol");
        assert!(zero_vol.as_variance.abs() < 1e-15);

        let low_k = forecast(inputs, DrasConfig { k: 0.01, ..config }, 1.0).expect("low k");
        let high_k = forecast(inputs, DrasConfig { k: 1.0, ..config }, 1.0).expect("high k");
        assert!(high_k.as_variance > low_k.as_variance);
        assert!(high_k.conditional_volatility > low_k.conditional_volatility);
    }
}
