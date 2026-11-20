//! Pure vector-math helpers shared across the kask families (no store
//! access — the numeric mirror of [`crate::text_chunking`]'s discipline
//! for text). `hkask-mcp-corpus` re-exports from here for its in-crate
//! paths.
//!
//! [`cosine_distance`] is the one implementation of f32-embedding cosine
//! distance. It was previously duplicated between this crate's federated
//! recall and hkask-mcp-corpus's helpers with drifting degenerate-input
//! semantics — 1.0 vs 2.0 for zero-norm vectors, silent zip-truncation vs
//! explicit refusal on length mismatch. Unified by the 2026-11-16 kask
//! audit on the corpus semantics: the safer retrieval defaults, where
//! degenerate and mismatched inputs are maximally distant, never silently
//! truncated.

/// Cosine distance between two f32 embeddings, computed in f64.
///
/// Returns 0.0 for identical, 1.0 for orthogonal, 2.0 for opposite or
/// degenerate (zero-norm) vectors. A length mismatch or empty input is
/// `2.0` (maximally distant), never a silent truncation. The result is
/// clamped to `[0.0, 2.0]` so float error cannot push it outside the
/// documented range.
#[must_use]
pub fn cosine_distance(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 2.0;
    }
    let dot: f64 = a
        .iter()
        .zip(b.iter())
        .map(|(x, y)| (*x as f64) * (*y as f64))
        .sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64).powi(2)).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 2.0;
    }
    let similarity = dot / (norm_a * norm_b);
    let distance = 1.0 - similarity;
    distance.clamp(0.0, 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_is_zero_and_opposite_is_two() {
        let v = [1.0_f32, 2.0, 3.0];
        assert!((cosine_distance(&v, &v) - 0.0).abs() < 1e-12);
        let w = [-1.0_f32, -2.0, -3.0];
        assert!((cosine_distance(&v, &w) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn orthogonal_is_one() {
        let v = [1.0_f32, 0.0];
        let w = [0.0_f32, 1.0];
        assert!((cosine_distance(&v, &w) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn zero_norm_is_maximally_distant_not_orthogonal() {
        // The pre-unification memory copy returned 1.0 here (tying
        // degenerate vectors with orthogonal ones); the unified semantics
        // rank them below every real vector instead.
        let v = [1.0_f32, 2.0];
        let zero = [0.0_f32, 0.0];
        assert!((cosine_distance(&v, &zero) - 2.0).abs() < 1e-12);
        assert!((cosine_distance(&zero, &zero) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn length_mismatch_is_maximally_distant_never_truncated() {
        // The pre-unification memory copy zip-truncated here, comparing
        // only the shared prefix; the unified semantics refuse the pair.
        let v = [1.0_f32, 2.0, 3.0];
        let w = [1.0_f32, 2.0];
        assert!((cosine_distance(&v, &w) - 2.0).abs() < 1e-12);
        assert!((cosine_distance(&[], &[]) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn float_error_stays_clamped() {
        // An exactly-opposite pair through the 1 - dot/(na*nb) path can
        // land a hair outside [0, 2]; the clamp pins the documented range.
        let v = [f32::MIN_POSITIVE, 0.0];
        let w = [-f32::MIN_POSITIVE, 0.0];
        let d = cosine_distance(&v, &w);
        assert!((0.0..=2.0).contains(&d));
    }
}
