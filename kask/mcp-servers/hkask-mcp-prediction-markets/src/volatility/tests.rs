//! Tests for the DR-AS structural volatility model (arXiv:2607.08199).

use super::*;

#[test]
fn dras_as_channel_adds_to_dr() {
    // The AS channel is additive: h² = DR + AS. With a spread and volume,
    // the total variance should exceed the DR-only variance.
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 1000.0,
    };
    let config = DrasConfig::default();
    let fc = forecast(inputs, config, 1.0).unwrap();
    assert!(fc.as_variance > 0.0, "AS channel contributes");
    assert!(fc.conditional_variance > fc.dr_variance, "total > DR alone");
    // h = √(h²)
    assert!((fc.conditional_volatility - fc.conditional_variance.sqrt()).abs() < 1e-12);
}

#[test]
fn dras_no_spread_means_no_as_channel() {
    // When spread is None (unobserved), the AS channel contributes 0.
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: None,
        volume: 1000.0,
    };
    let fc = forecast(inputs, DrasConfig::default(), 1.0).unwrap();
    assert!(fc.as_variance.abs() < 1e-15, "AS = 0 without spread");
    assert!((fc.conditional_variance - fc.dr_variance).abs() < 1e-15);
}

#[test]
fn dras_interval_clipped_to_unit_range() {
    // The 95% interval must be within [0, 1].
    let inputs = VolatilityInputs {
        price: 0.99,
        hours_to_resolution: 1.0, // very near deadline → high vol
        spread: Some(0.10),
        volume: 5000.0,
    };
    let fc = forecast(inputs, DrasConfig::default(), 1.0).unwrap();
    let (lo, hi) = fc.interval_95;
    assert!(lo >= 0.0, "lower >= 0");
    assert!(hi <= 1.0, "upper <= 1");
    assert!(lo < hi, "non-empty interval");
}

#[test]
fn dras_sqrt_volume_is_best_activity_proxy_per_paper() {
    // The paper found √V statistically best. Verify the default is √V.
    let config = DrasConfig::default();
    assert_eq!(config.activity_proxy, ActivityProxy::SqrtVolume);
    // ν(1000) = √1000 ≈ 31.6
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
fn dras_rejects_degenerate_inputs() {
    let bad_price = VolatilityInputs {
        price: 1.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 100.0,
    };
    assert!(forecast(bad_price, DrasConfig::default(), 1.0).is_none());

    let bad_tau = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 0.0,
        spread: Some(0.04),
        volume: 100.0,
    };
    assert!(forecast(bad_tau, DrasConfig::default(), 1.0).is_none());

    let bad_delta = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 100.0,
    };
    assert!(forecast(bad_delta, DrasConfig::default(), 0.0).is_none());
}

#[test]
fn dras_scales_linearly_with_horizon() {
    // The conditional variance scales linearly with Δ (the forecast horizon).
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 1000.0,
    };
    let config = DrasConfig::default();
    let h1 = forecast(inputs, config, 1.0).unwrap();
    let h4 = forecast(inputs, config, 4.0).unwrap();
    // 4-hour variance = 4 × 1-hour variance.
    assert!((h4.conditional_variance / h1.conditional_variance - 4.0).abs() < 0.01);
}

#[test]
fn dras_default_k_gives_reasonable_magnitudes() {
    // For a typical active contract (p=0.5, τ=168h, s=0.04, V=1000),
    // the DR and AS channels should be the same order of magnitude.
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 1000.0,
    };
    let fc = forecast(inputs, DrasConfig::default(), 1.0).unwrap();
    // DR ≈ 0.0015, AS ≈ 0.0015 → total ≈ 0.003, h ≈ 0.055
    assert!(fc.dr_variance > 0.001, "DR = {}", fc.dr_variance);
    assert!(fc.as_variance > 0.0005, "AS = {}", fc.as_variance);
    assert!(
        fc.conditional_volatility > 0.03,
        "h = {}",
        fc.conditional_volatility
    );
    assert!(
        fc.conditional_volatility < 0.10,
        "h = {}",
        fc.conditional_volatility
    );
}

#[test]
fn dras_volume_zero_means_no_as_channel() {
    // ν(0) = 0 for all non-constant proxies → AS channel = 0.
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 0.0,
    };
    let fc = forecast(inputs, DrasConfig::default(), 1.0).unwrap();
    assert!(fc.as_variance.abs() < 1e-15, "AS = 0 with zero volume");
}

#[test]
fn dras_config_k_override() {
    // The operator can override K. A larger K → larger AS contribution.
    let inputs = VolatilityInputs {
        price: 0.5,
        hours_to_resolution: 168.0,
        spread: Some(0.04),
        volume: 1000.0,
    };
    let low_k = DrasConfig {
        k: 0.01,
        ..DrasConfig::default()
    };
    let high_k = DrasConfig {
        k: 1.0,
        ..DrasConfig::default()
    };
    let fc_low = forecast(inputs, low_k, 1.0).unwrap();
    let fc_high = forecast(inputs, high_k, 1.0).unwrap();
    assert!(
        fc_high.as_variance > fc_low.as_variance,
        "higher K → more AS"
    );
    assert!(fc_high.conditional_volatility > fc_low.conditional_volatility);
}
