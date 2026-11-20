//! Property layer for the calibration math
//! (`kask/docs/reference/testing-protocol.md`). Batch 2 of the propagation
//! plan (`kask/docs/reference/testing-protocol.md`): Phase 0 measured 53
//! value-assert tests and zero property sites in this crate — the math is
//! exactly property-shaped. Each property states a falsifiable hypothesis
//! with its declared input domain; a shrunk counterexample is a finding to
//! report, never a signal to weaken the property.

use super::*;
use proptest::prelude::*;

proptest! {
    /// Hypothesis: the Brier penalty is exactly quadratic and bounded — for
    /// every p ∈ [0,1] and binary outcome, score = (p − outcome)² ∈ [0,1].
    #[test]
    fn brier_is_quadratic_and_bounded(p in 0.0..=1.0, occurred in any::<bool>()) {
        let score = brier_score(p, occurred);
        let expected = (p - if occurred { 1.0 } else { 0.0 }).powi(2);
        prop_assert!((score - expected).abs() < 1e-15);
        prop_assert!((0.0..=1.0).contains(&score));
    }

    /// Hypothesis: the multi-forecast score is the exact mean of the
    /// per-forecast Brier scores, over matched nonempty generated vectors.
    #[test]
    fn brier_multi_mean_is_exact_over_generated_vectors(
        cases in proptest::collection::vec((0.0..=1.0, any::<bool>()), 1..24),
    ) {
        let (probabilities, outcomes): (Vec<f64>, Vec<bool>) = cases.into_iter().unzip();
        let multi =
            brier_score_multi(&probabilities, &outcomes).expect("matched nonempty vectors score");
        let expected = probabilities
            .iter()
            .zip(&outcomes)
            .map(|(&p, &o)| brier_score(p, o))
            .sum::<f64>()
            / probabilities.len() as f64;
        prop_assert!((multi - expected).abs() < 1e-12);
    }

    /// Hypothesis: the Wilson interval always brackets the observed rate and
    /// stays inside [0,1]. Domain: 1 ≤ trials, hits ≤ trials (a hit count
    /// above the trial count is out of domain by construction).
    #[test]
    fn wilson_bounds_bracket_the_observed_rate(
        sample in (1usize..=1000).prop_flat_map(|trials| (Just(trials), 0usize..=trials)),
    ) {
        let (trials, hits) = sample;
        let (lo, hi) = wilson_bounds(hits, trials).expect("trials >= 1 always yields bounds");
        let p_hat = hits as f64 / trials as f64;
        prop_assert!(lo <= p_hat + 1e-12, "lower bound {lo} above rate {p_hat}");
        prop_assert!(hi >= p_hat - 1e-12, "upper bound {hi} below rate {p_hat}");
        prop_assert!(lo <= hi);
        prop_assert!((0.0..=1.0).contains(&lo) && (0.0..=1.0).contains(&hi));
    }

    /// Hypothesis: uncertainty shrinks with sample size at a constant rate —
    /// doubling both hits and trials weakly narrows the interval.
    #[test]
    fn wilson_width_weakly_shrinks_when_both_sides_double(
        sample in (1usize..=500).prop_flat_map(|trials| (Just(trials), 0usize..=trials)),
    ) {
        let (base_trials, base_hits) = sample;
        let (lo1, hi1) = wilson_bounds(base_hits, base_trials).expect("bounds at n");
        let (lo2, hi2) =
            wilson_bounds(base_hits * 2, base_trials * 2).expect("bounds at 2n");
        let w1 = hi1 - lo1;
        let w2 = hi2 - lo2;
        prop_assert!(
            w2 <= w1 + 1e-12,
            "width grew from {w1} to {w2} at the constant rate {}",
            base_hits as f64 / base_trials as f64
        );
    }

    /// Hypothesis: with nonzero confidence weights the Fermi aggregate stays
    /// inside the convex hull of the sub-question estimates — a weighted mean
    /// can never amplify past its extremes.
    #[test]
    fn fermi_aggregate_stays_in_the_estimate_hull(
        questions in proptest::collection::vec((0.0..=1.0, 0.01..=1.0), 1..10),
    ) {
        let qs: Vec<FermiQuestion> = questions
            .into_iter()
            .enumerate()
            .map(|(i, (estimate, confidence))| {
                FermiQuestion::new(format!("sub-question {i}"), estimate, confidence)
            })
            .collect();
        let aggregate = calibrate_from_fermi(&qs).expect("in-range inputs aggregate");
        let lo = qs.iter().map(|q| q.estimate).fold(f64::INFINITY, f64::min);
        let hi = qs.iter().map(|q| q.estimate).fold(f64::NEG_INFINITY, f64::max);
        prop_assert!(
            aggregate >= lo - 1e-12 && aggregate <= hi + 1e-12,
            "aggregate {aggregate} escaped the estimate hull [{lo}, {hi}]"
        );
    }

    /// Hypothesis: zero total confidence always yields the neutral prior — no
    /// estimate can drag the aggregate when nothing is trusted.
    #[test]
    fn fermi_zero_total_confidence_is_always_neutral(
        estimates in proptest::collection::vec(0.0..=1.0, 1..10),
    ) {
        let qs: Vec<FermiQuestion> = estimates
            .into_iter()
            .enumerate()
            .map(|(i, estimate)| {
                FermiQuestion::new(format!("sub-question {i}"), estimate, 0.0)
            })
            .collect();
        let aggregate = calibrate_from_fermi(&qs).expect("zero weights are the neutral case");
        prop_assert!((aggregate - 0.5).abs() < 1e-12);
    }

    /// Hypothesis: the isotonic fit is non-decreasing and its application is
    /// monotone and bounded — calibrated probabilities never decrease as the
    /// raw probability rises, over any generated calibration set.
    #[test]
    fn isotonic_apply_is_monotone_and_bounded(
        pairs in proptest::collection::vec((0.0..=1.0, any::<bool>()), 2..16),
        raw_x in 0.0..=1.0,
        raw_y in 0.0..=1.0,
    ) {
        let fit = isotonic_fit(&pairs).expect("two or more pairs always fit");
        for window in fit.windows(2) {
            prop_assert!(
                window[0].1 <= window[1].1 + 1e-12,
                "fit values must be non-decreasing: {fit:?}"
            );
        }
        let (x, y) = if raw_x <= raw_y {
            (raw_x, raw_y)
        } else {
            (raw_y, raw_x)
        };
        let calibrated_x = isotonic_apply(&fit, x);
        let calibrated_y = isotonic_apply(&fit, y);
        prop_assert!(
            calibrated_x <= calibrated_y + 1e-12,
            "apply must be monotone: f({x}) = {calibrated_x} > f({y}) = {calibrated_y}"
        );
        prop_assert!((0.0..=1.0).contains(&calibrated_x));
        prop_assert!((0.0..=1.0).contains(&calibrated_y));
    }

    /// Hypothesis: an empty fit is the identity — uncalibrated input passes
    /// through unchanged for every generated probability.
    #[test]
    fn isotonic_apply_empty_fit_is_identity(p in 0.0..=1.0) {
        prop_assert!((isotonic_apply(&[], p) - p).abs() < 1e-15);
    }

    /// Hypothesis: the calibration is constant within each knot interval —
    /// two generated points in the same inter-knot region map to the same
    /// calibrated value (the ledger's piecewise-constancy follow-up).
    #[test]
    fn isotonic_apply_is_constant_within_knot_intervals(
        pairs in proptest::collection::vec((0.0f64..=1.0, any::<bool>()), 2..16),
        a in 0.0f64..=1.0,
    ) {
        let fit = isotonic_fit(&pairs).expect("two or more pairs always fit");
        let upper = fit
            .iter()
            .map(|(threshold, _)| *threshold)
            .find(|threshold| a < *threshold)
            .unwrap_or(1.0);
        let b = (a + upper) / 2.0;
        if b > a {
            prop_assert_eq!(
                isotonic_apply(&fit, a),
                isotonic_apply(&fit, b),
                "points {} and {} share a knot interval and must calibrate equal",
                a,
                b
            );
        }
    }
}
