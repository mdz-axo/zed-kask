//! Tests for residual risk decomposition (T15).

use hkask_mcp_prediction_markets::residual::{MIN_OBSERVATIONS, residual_analysis};

/// Build a synthetic series where niche = base moved by factor β plus a
/// known idiosyncratic residual on the last observation.
fn synthetic(base_moves: &[f64], beta: f64) -> Vec<(f64, f64)> {
    let mut observations = Vec::new();
    let mut base = 0.5f64;
    let mut niche = 0.5f64;
    observations.push((niche, base));
    for &m in base_moves {
        base = (base + m).clamp(0.05, 0.95);
        niche = (niche + m * beta).clamp(0.05, 0.95);
        observations.push((niche, base));
    }
    observations
}

#[test]
fn synthetic_beta_one_recovers_unit_exposure() {
    // Niche tracks base exactly (β=1): regression must recover β≈1, r²≈1.
    let moves: Vec<f64> = (0..15).map(|i| 0.02 * if i % 2 == 0 { 1.0 } else { -0.7 }).collect();
    let observations = synthetic(&moves, 1.0);
    let analysis = residual_analysis(&observations).expect("fits");
    assert!(
        (analysis.beta - 1.0).abs() < 0.15,
        "recovered beta {} should be ≈1",
        analysis.beta
    );
    assert!(analysis.r_squared > 0.95, "r² {}", analysis.r_squared);
    assert!(analysis.latest_residual.abs() < 0.05);
}

#[test]
fn synthetic_half_exposure_recovers_beta_half() {
    let moves: Vec<f64> = (0..15).map(|i| 0.03 * if i % 2 == 0 { 1.0 } else { -0.8 }).collect();
    let observations = synthetic(&moves, 0.5);
    let analysis = residual_analysis(&observations).expect("fits");
    assert!(
        (analysis.beta - 0.5).abs() < 0.15,
        "recovered beta {} should be ≈0.5",
        analysis.beta
    );
}

#[test]
fn thin_overlap_refuses() {
    let observations: Vec<(f64, f64)> = (0..5).map(|i| (0.5 + i as f64 * 0.01, 0.5)).collect();
    assert!(observations.len() < MIN_OBSERVATIONS);
    assert!(residual_analysis(&observations).is_none());
}

#[test]
fn immobile_base_refuses() {
    // Base never moves → no exposure estimable (sxx = 0).
    let observations: Vec<(f64, f64)> = (0..12)
        .map(|i| (0.5 + i as f64 * 0.01, 0.5))
        .collect();
    assert!(residual_analysis(&observations).is_none());
}

#[test]
fn output_carries_fit_quality() {
    let moves: Vec<f64> = (0..12).map(|i| 0.02 * if i % 3 == 0 { -1.0 } else { 1.0 }).collect();
    let observations = synthetic(&moves, 0.8);
    let analysis = residual_analysis(&observations).expect("fits");
    // No bare-number returns: consumers get the evidence to judge the fit.
    assert!(analysis.observations >= MIN_OBSERVATIONS - 1);
    assert!((0.0..=1.0).contains(&analysis.r_squared));
}
