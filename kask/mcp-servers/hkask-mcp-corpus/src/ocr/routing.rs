//! Routing Strategy — Complexity-driven backend selection with overlap sampling.
//!
//! Deterministic routing (no randomness) guarantees statistical properties
//! without non-determinism. SamplingState is a transparent accumulator.

use crate::ocr::{ComplexityScore, ComplexityTier, DEFAULT_LLM_OCR_MODEL, OcrBackend};

/// Transparent accumulator for deterministic round-robin sampling.
///
/// Counters only — no side effects, no hidden state.
#[derive(Debug, Clone, Default)]
pub struct SamplingState {
    /// Total Moderate pages seen (accumulates across a pipeline run).
    pub moderate_pages_seen: usize,
    /// Moderate pages that were dual-routed.
    pub moderate_pages_dual_routed: usize,
    /// Round-robin counter for every_nth sampling.
    counter: usize,
    /// Sampling interval: dual-route every Nth Moderate page.
    sample_every_nth: usize,
}

impl SamplingState {
    /// Create a new sampling state.
    ///
    /// `sample_rate` is in [0.0, 1.0]. Internally converted to `every_nth`.
    pub fn new(sample_rate: f32) -> Self {
        let rate = sample_rate.clamp(0.0, 1.0);
        let every_nth = if rate <= 0.0 {
            usize::MAX // never sample
        } else if rate >= 1.0 {
            1 // always sample
        } else {
            (1.0 / rate).round() as usize
        };
        Self {
            sample_every_nth: every_nth,
            ..Default::default()
        }
    }

    /// Determine whether the current Moderate page should be dual-routed.
    fn should_dual_route(&mut self) -> bool {
        self.counter += 1;
        self.counter.is_multiple_of(self.sample_every_nth)
    }
}

/// Route a page to one or more OCR backends based on its complexity score.
///
/// # Strategy
/// - `Simple` → `[Tesseract]` (single backend, fast path)
/// - `Complex` → `[LightOn]` or `[LlmOcr(model)]` per config
/// - `Moderate` → `[Tesseract]` normally, dual-route `[Tesseract, LightOn]`
///   at a configurable rate (default 10%) using deterministic round-robin.
///
/// # Force fallback
/// When `state.force_fallback` is set, the primary backend candidate
/// is excluded. This is the unified fallback path — not a separate code fork.
pub fn route_page(
    score: ComplexityScore,
    state: &mut SamplingState,
    exclude_backend: Option<&OcrBackend>,
    llm_model: Option<&str>,
) -> Vec<OcrBackend> {
    match score.tier {
        ComplexityTier::Simple => {
            let backends = vec![OcrBackend::Tesseract];
            filter_excluded(backends, exclude_backend)
        }
        ComplexityTier::Complex => {
            let model = llm_model.unwrap_or(DEFAULT_LLM_OCR_MODEL);
            let backends = vec![OcrBackend::LlmOcr(model.to_string())];
            filter_excluded(backends, exclude_backend)
        }
        ComplexityTier::Moderate => {
            state.moderate_pages_seen += 1;
            let should_sample = state.should_dual_route();
            if should_sample {
                state.moderate_pages_dual_routed += 1;
                let model = llm_model.unwrap_or(DEFAULT_LLM_OCR_MODEL);
                let backends = vec![OcrBackend::Tesseract, OcrBackend::LlmOcr(model.to_string())];
                filter_excluded(backends, exclude_backend)
            } else {
                let backends = vec![OcrBackend::Tesseract];
                filter_excluded(backends, exclude_backend)
            }
        }
    }
}

/// Remove excluded backend from candidate list.
fn filter_excluded(backends: Vec<OcrBackend>, exclude: Option<&OcrBackend>) -> Vec<OcrBackend> {
    if let Some(excluded) = exclude {
        backends.into_iter().filter(|b| b != excluded).collect()
    } else {
        backends
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_routes_to_tesseract() {
        let score = ComplexityScore {
            value: 0.01,
            tier: ComplexityTier::Simple,
        };
        let mut state = SamplingState::new(0.10);
        let backends = route_page(score, &mut state, None, None);
        assert_eq!(backends, vec![OcrBackend::Tesseract]);
    }

    #[test]
    fn complex_routes_to_llm_ocr() {
        let score = ComplexityScore {
            value: 0.20,
            tier: ComplexityTier::Complex,
        };
        // Default model (olmOCR-2 on DeepInfra)
        let mut state = SamplingState::new(0.10);
        let backends = route_page(score, &mut state, None, None);
        assert_eq!(
            backends,
            vec![OcrBackend::LlmOcr(DEFAULT_LLM_OCR_MODEL.into())]
        );

        // Custom model override
        let mut state = SamplingState::new(0.10);
        let backends = route_page(score, &mut state, None, Some("minicpm"));
        assert_eq!(backends, vec![OcrBackend::LlmOcr("minicpm".into())]);
    }

    // count is within ±5% of configured 10% rate
    #[test]
    fn moderate_sampling_rate_within_tolerance() {
        let mut state = SamplingState::new(0.10); // 10% rate
        let score = ComplexityScore {
            value: 0.08,
            tier: ComplexityTier::Moderate,
        };

        let total = 1000;
        let mut dual_count = 0;
        for _ in 0..total {
            let backends = route_page(score, &mut state, None, None);
            if backends.len() == 2 {
                dual_count += 1;
            }
        }

        assert_eq!(state.moderate_pages_seen, total);
        assert_eq!(state.moderate_pages_dual_routed, dual_count);

        let expected = (total as f32 * 0.10) as usize;
        let tolerance = (total as f32 * 0.05) as usize;
        let min_expected = expected.saturating_sub(tolerance);
        let max_expected = expected + tolerance;

        assert!(
            dual_count >= min_expected && dual_count <= max_expected,
            "dual-routed count {} not within ±5% of expected {} (range: {}-{})",
            dual_count,
            expected,
            min_expected,
            max_expected
        );
    }

    #[test]
    fn exclude_failed_backend() {
        let score = ComplexityScore {
            value: 0.01,
            tier: ComplexityTier::Simple,
        };
        let mut state = SamplingState::new(0.10);
        let backends = route_page(score, &mut state, Some(&OcrBackend::Tesseract), None);
        assert!(
            backends.is_empty(),
            "excluding only backend should yield empty list"
        );
    }

    #[test]
    fn full_sample_rate_dual_routes_all() {
        let mut state = SamplingState::new(1.0);
        let score = ComplexityScore {
            value: 0.08,
            tier: ComplexityTier::Moderate,
        };
        for _ in 0..100 {
            let backends = route_page(score, &mut state, None, None);
            assert_eq!(backends.len(), 2);
        }
        assert_eq!(state.moderate_pages_dual_routed, 100);
    }

    #[test]
    fn zero_sample_rate_never_dual_routes() {
        let mut state = SamplingState::new(0.0);
        let score = ComplexityScore {
            value: 0.08,
            tier: ComplexityTier::Moderate,
        };
        for _ in 0..100 {
            let backends = route_page(score, &mut state, None, None);
            assert_eq!(backends.len(), 1);
        }
        assert_eq!(state.moderate_pages_dual_routed, 0);
    }
}
