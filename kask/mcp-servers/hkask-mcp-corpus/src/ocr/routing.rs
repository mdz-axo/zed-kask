//! Routing Strategy — Complexity-driven backend selection with overlap sampling.
//!
//! Deterministic routing (no randomness) guarantees statistical properties
//! without non-determinism. SamplingState is a transparent accumulator.

use crate::ocr::{ComplexityScore, ComplexityTier, DEFAULT_LLM_OCR_MODEL, OcrBackend};

/// Transparent accumulator for deterministic round-robin sampling.
///
/// Counters only — no side effects, no hidden state.
#[derive(Debug, Clone, Default)]
pub(crate) struct SamplingState {
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
    ///
    /// When `sample_every_nth == usize::MAX` (the zero-rate sentinel),
    /// returns `false` without incrementing the counter — otherwise the
    /// counter would eventually overflow and wrap to 0, which is a multiple
    /// of any value and would falsely trigger dual-routing.
    fn should_dual_route(&mut self) -> bool {
        if self.sample_every_nth == usize::MAX {
            return false;
        }
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
pub(crate) fn route_page(
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
