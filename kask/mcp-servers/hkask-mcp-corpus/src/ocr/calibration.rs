//! OCR Threshold Calibration — Self-tuning analysis for Regulation → Curator → human flow.
//!
//! Analyzes accumulated cross-validation data to detect when routing thresholds
//! may be too conservative. Emits Regulation alerts (via `tracing::warn!`) suggesting
//! threshold adjustments. **Never auto-adjusts** — P4 affirmative consent requires
//! human approval via `kask settings set`.
//!
//! # Regulation Flow
//!
//! Threshold drift alerts are emitted via `tracing::warn!` for operational
//! observability. Pipeline-level Regulation observation (start/settle spans, gas
//! accounting, variety tracking) is handled externally by the GovernedTool
//! membrane — the docproc server is a pure instrument with no self-instrumentation.

use crate::ocr::{ComplexityTier, CrossValidation, PipelineOutcome, ThresholdConfig};

/// Evidence backing a threshold drift suggestion.
#[derive(Debug, Clone)]
pub(crate) struct DriftEvidence {
    /// Number of dual-routed pages analyzed.
    pub sample_count: usize,
    /// Mean similarity across all dual-routed pages in this tier.
    pub mean_similarity: f32,
}

/// A Regulation alert suggesting a threshold adjustment.
///
/// Observation only — does not autonomously change routing (P4).
#[derive(Debug, Clone)]
pub(crate) struct ThresholdDriftAlert {
    /// Which threshold parameter to adjust (e.g., "moderate_max").
    pub parameter: &'static str,
    /// Current configured value.
    pub current_value: f32,
    /// Suggested new value based on evidence.
    pub suggested_value: f32,
    /// Statistical evidence backing the suggestion.
    pub evidence: DriftEvidence,
}

/// Analyze accumulated pipeline outcomes for threshold drift.
///
/// Collects cross-validation data from Moderate-tier dual-routed pages.
/// If enough samples show consistently high agreement between Tesseract
/// and LlmOcr, suggests raising `moderate_max` (fewer pages need dual routing).
///
/// # Thresholds
/// - Minimum sample count: 100 dual-routed Moderate pages
/// - Mean similarity threshold: >95% to suggest raising `moderate_max`
/// - Suggested adjustment: raise `moderate_max` by 0.05 (capped at 0.50)
///
/// Returns `None` if insufficient data or similarity is too low.
pub(crate) fn analyze_threshold_drift(
    outcomes: &[PipelineOutcome],
    current_thresholds: &ThresholdConfig,
) -> Option<ThresholdDriftAlert> {
    // P4: Respect the tuneable guardrail — if threshold tuning is disabled,
    // don't even analyze. The field was previously defined but never enforced.
    if !current_thresholds.tuneable {
        return None;
    }

    // Collect all cross-validations from Moderate-tier pages
    let moderate_cvs: Vec<&CrossValidation> = outcomes
        .iter()
        .flat_map(|o| &o.cross_validations)
        .filter(|cv| cv.tier == ComplexityTier::Moderate)
        .collect();

    if moderate_cvs.len() < 100 {
        return None; // Insufficient data
    }

    let mean_similarity: f32 =
        moderate_cvs.iter().map(|cv| cv.similarity).sum::<f32>() / moderate_cvs.len() as f32;

    if mean_similarity <= 0.95 {
        return None; // Not enough agreement to justify threshold change
    }

    // Suggest raising moderate_max by 0.05, capped at 0.50
    let suggested = (current_thresholds.moderate_max + 0.05).min(0.50);

    // Don't suggest if already at or above the suggested value
    if current_thresholds.moderate_max >= suggested {
        return None;
    }

    Some(ThresholdDriftAlert {
        parameter: "moderate_max",
        current_value: current_thresholds.moderate_max,
        suggested_value: suggested,
        evidence: DriftEvidence {
            sample_count: moderate_cvs.len(),
            mean_similarity,
        },
    })
}

/// Emit a Regulation alert for a threshold drift suggestion.
///
/// Uses `tracing::warn!` under `reg.pipeline.calibration` target.
/// The GovernedTool membrane handles RegulationRecord persistence for Regulation learning.
pub(crate) fn emit_drift_alert(alert: &ThresholdDriftAlert) {
    tracing::warn!(
        target: "reg.pipeline.calibration",
        parameter = alert.parameter,
        current = alert.current_value,
        suggested = alert.suggested_value,
        sample_count = alert.evidence.sample_count,
        mean_similarity = alert.evidence.mean_similarity,
        "OCR threshold drift detected — human approval required. \
         Run: kask settings set ocr_{} {}",
        alert.parameter,
        alert.suggested_value
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ocr::{CrossValidation, OcrBackend, PipelineOutcome, VerificationReport};

    fn default_thresholds() -> ThresholdConfig {
        ThresholdConfig::default()
    }

    /// Build a PipelineOutcome with N cross-validations at a given similarity.
    fn outcome_with_cvs(count: usize, similarity: f32) -> PipelineOutcome {
        let cvs: Vec<CrossValidation> = (0..count)
            .map(|i| CrossValidation {
                page_index: i,
                similarity,
                tier: ComplexityTier::Moderate,
                backend_a: OcrBackend::Tesseract,
                backend_b: OcrBackend::LlmOcr("test-model".into()),
                semantic_similarity: None,
            })
            .collect();

        PipelineOutcome {
            results: vec![],
            report: VerificationReport::new(true, vec![], 0),
            cross_validations: cvs,
            errors: vec![],
        }
    }

    #[test]
    fn insufficient_samples_returns_none() {
        let outcomes = vec![outcome_with_cvs(50, 0.98)];
        let thresholds = default_thresholds();
        assert!(analyze_threshold_drift(&outcomes, &thresholds).is_none());
    }

    #[test]
    fn low_similarity_returns_none() {
        let outcomes = vec![outcome_with_cvs(100, 0.90)];
        let thresholds = default_thresholds();
        assert!(analyze_threshold_drift(&outcomes, &thresholds).is_none());
    }

    #[test]
    fn sufficient_high_similarity_returns_alert() {
        let outcomes = vec![outcome_with_cvs(150, 0.97)];
        let thresholds = default_thresholds();
        let alert = analyze_threshold_drift(&outcomes, &thresholds).unwrap();
        assert_eq!(alert.parameter, "moderate_max");
        assert_eq!(alert.current_value, 0.15);
        assert_eq!(alert.suggested_value, 0.20); // 0.15 + 0.05
        assert_eq!(alert.evidence.sample_count, 150);
        assert!((alert.evidence.mean_similarity - 0.97).abs() < 0.001);
    }

    #[test]
    fn suggested_value_capped_at_0_50() {
        let outcomes = vec![outcome_with_cvs(200, 0.99)];
        let mut thresholds = default_thresholds();
        thresholds.moderate_max = 0.48;
        let alert = analyze_threshold_drift(&outcomes, &thresholds).unwrap();
        assert_eq!(alert.suggested_value, 0.50); // 0.48 + 0.05 = 0.53, capped
    }

    #[test]
    fn already_at_suggested_returns_none() {
        let outcomes = vec![outcome_with_cvs(200, 0.99)];
        let mut thresholds = default_thresholds();
        thresholds.moderate_max = 0.50; // Already at cap
        assert!(analyze_threshold_drift(&outcomes, &thresholds).is_none());
    }

    #[test]
    fn only_moderate_cvs_counted() {
        // Mix of Simple and Moderate CVs — only Moderate should count
        let mut outcome = outcome_with_cvs(80, 0.98); // 80 Moderate
        // Add 30 Simple CVs (should be ignored)
        let simple_cvs: Vec<CrossValidation> = (0..30)
            .map(|i| CrossValidation {
                page_index: i + 1000,
                similarity: 0.99,
                tier: ComplexityTier::Simple,
                backend_a: OcrBackend::Tesseract,
                backend_b: OcrBackend::LlmOcr("test-model".into()),
                semantic_similarity: None,
            })
            .collect();
        outcome.cross_validations.extend(simple_cvs);
        // Total: 110 CVs, but only 80 Moderate → insufficient
        let thresholds = default_thresholds();
        assert!(analyze_threshold_drift(&[outcome], &thresholds).is_none());
    }
}
