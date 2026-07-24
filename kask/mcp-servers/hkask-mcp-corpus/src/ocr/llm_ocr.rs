//! Vision LLM Backend — OCR via hkask-inference vision models.
//!
//! Sends page images as base64-encoded PNG to vision-capable LLMs
//! through the inference router. Supports provider-prefixed model names
//! (DI/, FW/, OM/) for backend routing.
//!
//! Includes a circuit breaker for rate-limit resilience: after N consecutive
//! 429 responses, all LLM requests pause for a cooldown period.
use async_trait::async_trait;

use crate::ocr::{OcrBackend, OcrResult};
use base64::Engine;
use hkask_inference::InferenceRouter;
use hkask_types::template::LLMParameters;
use image::DynamicImage;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Instant;

use crate::ocr::pipeline::{OcrError, OcrExecutor};

/// Fallback prompt used when the `docproc/ocr-extract.j2` template is missing.
/// Kept minimal — vision OCR models (OLMOCR-2 et al.) are trained to extract
/// structured markdown faithfully; restating what they already do is
/// over-engineering (hkask P5: simplicity).
const OCR_FALLBACK_PROMPT: &str = "Extract all text from this page image as Markdown. Preserve headings, tables, equations, and reading order. If the page is blank or contains no text, output BLANK.";

/// Build the OCR prompt, preferring the `docproc/ocr-extract.j2` template
/// (tunable without recompile) and falling back to `OCR_FALLBACK_PROMPT` when
/// the template is absent or fails to render.
///
/// `anchored_text` optionally supplies native text blocks from the page's text
/// layer (document anchoring — improves reading order on complex layouts).
pub(crate) fn build_ocr_prompt(anchored_text: Option<&str>) -> String {
    let mut vars = std::collections::HashMap::new();
    if let Some(t) = anchored_text {
        vars.insert("anchored_text", t.to_string());
    }
    let rendered = crate::render_docproc_template("ocr-extract", &vars);
    if rendered.is_empty() {
        OCR_FALLBACK_PROMPT.to_string()
    } else {
        rendered
    }
}

/// Circuit breaker for rate-limit resilience.
///
/// After `threshold` consecutive failures, pauses all requests until
/// `cooldown_secs` after the last failure. Embedded in `LlmOcrExecutor`.
struct CircuitBreaker {
    /// Consecutive failure count (429 or connection errors).
    failures: AtomicU64,
    /// Unix timestamp (seconds) until which the breaker is open.
    cooldown_until: AtomicI64,
    /// Consecutive failures before opening.
    threshold: u64,
    /// Cooldown duration in seconds.
    cooldown_secs: u64,
}

impl CircuitBreaker {
    const fn new(threshold: u64, cooldown_secs: u64) -> Self {
        Self {
            failures: AtomicU64::new(0),
            cooldown_until: AtomicI64::new(0),
            threshold,
            cooldown_secs,
        }
    }

    /// Check whether requests are allowed. Returns `true` if the circuit is closed.
    fn is_closed(&self) -> bool {
        let now = now_unix();
        let until = self.cooldown_until.load(Ordering::Relaxed);
        now >= until
    }

    /// Record a successful request — resets the failure counter.
    fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        self.cooldown_until.store(0, Ordering::Relaxed);
    }

    /// Record a failure. If the threshold is reached, open the circuit.
    fn record_failure(&self) {
        let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= self.threshold {
            let until = now_unix() + self.cooldown_secs as i64;
            self.cooldown_until.store(until, Ordering::Relaxed);
            tracing::warn!(
                target: "reg.pipeline.ocr.circuit_breaker",
                failures = count,
                cooldown_secs = self.cooldown_secs,
                "Circuit breaker opened — pausing LLM OCR requests"
            );
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Vision LLM OCR executor using the hkask-inference router.
///
/// Encodes page images as base64 PNG and dispatches to vision-capable
/// models via `generate_vision`. Supports all inference backends
/// (DeepInfra, Together AI) through provider-prefixed model names.
///
/// The router is constructed once and shared across all concurrent
/// OCR tasks via `Arc<InferenceRouter>`.
pub struct LlmOcrExecutor {
    /// Shared inference router (constructed once, used by all concurrent tasks).
    router: Arc<InferenceRouter>,
    /// Maximum output tokens per page.
    max_tokens: u32,
    /// Circuit breaker for rate-limit resilience.
    breaker: CircuitBreaker,
}

impl LlmOcrExecutor {
    /// Create a new LLM OCR executor with a shared router.
    pub fn new(router: Arc<InferenceRouter>) -> Self {
        Self {
            router,
            max_tokens: 4096,
            breaker: CircuitBreaker::new(5, 30), // 5 consecutive failures → 30s cooldown
        }
    }

    /// Set maximum output tokens per page.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    #[cfg(test)]
    fn force_open_circuit(&self) {
        for _ in 0..self.breaker.threshold {
            self.breaker.record_failure();
        }
    }
}
#[async_trait]
impl OcrExecutor for LlmOcrExecutor {
    fn is_available(&self, backend: &OcrBackend) -> bool {
        if !matches!(backend, OcrBackend::LlmOcr(_)) {
            return false;
        }
        // Circuit breaker: if open, report as unavailable so the pipeline
        // falls back to Tesseract gracefully without explicit circuit checks.
        if !self.breaker.is_closed() {
            tracing::debug!(
                target: "reg.pipeline.ocr.circuit_breaker",
                "LLM OCR reported unavailable — circuit breaker open"
            );
            return false;
        }
        true
    }

    async fn execute(
        &self,
        page_index: usize,
        backend: &OcrBackend,
        image: &DynamicImage,
        is_fallback: bool,
    ) -> Result<OcrResult, OcrError> {
        let model = match backend {
            OcrBackend::LlmOcr(model) => model.clone(),
            other => {
                return Err(OcrError::BackendFailed {
                    backend: format!("{:?}", other),
                    message: "LlmOcrExecutor cannot handle this backend".into(),
                });
            }
        };

        let start = Instant::now();

        // Encode image as base64 JPEG (smaller than PNG, fits 128K token limit)
        let mut img_bytes: Vec<u8> = Vec::new();
        image
            .write_to(
                &mut std::io::Cursor::new(&mut img_bytes),
                image::ImageFormat::Jpeg,
            )
            .map_err(|e| OcrError::BackendFailed {
                backend: "llm_ocr".into(),
                message: format!("Failed to encode page image as JPEG: {e}"),
            })?;

        let b64_data = base64::engine::general_purpose::STANDARD.encode(&img_bytes);

        let params = LLMParameters {
            temperature: 0.1, // Low temperature for faithful extraction
            max_tokens: self.max_tokens,
            ..Default::default()
        };

        let result = self
            .router
            .generate_vision(&build_ocr_prompt(None), &[b64_data], &params, Some(&model))
            .await
            .map_err(|e| {
                let err_str = e.to_string();
                // GAP-4: Regulation variety — detect rate-limit backpressure
                if err_str.contains("429")
                    || err_str.contains("rate limit")
                    || err_str.contains("Rate limit")
                {
                    tracing::warn!(
                        target: "reg.pipeline.ocr.rate_limit",
                        model = %model,
                        page_index = page_index,
                        "OCR inference rate-limited — circuit breaker tracking"
                    );
                }
                OcrError::InferenceFailed(err_str)
            });

        match &result {
            Ok(_) => self.breaker.record_success(),
            Err(OcrError::InferenceFailed(err_str)) => {
                if err_str.contains("429")
                    || err_str.contains("rate limit")
                    || err_str.contains("Rate limit")
                    || err_str.contains("timed out")
                    || err_str.contains("connection")
                {
                    self.breaker.record_failure();
                }
            }
            Err(_) => {}
        }

        let result = result?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // Vision OCR models don't expose real per-token confidence via chat
        // completions, so any number here is a placeholder. Use a fixed nominal
        // rather than an invented 3-factor heuristic (hkask P5: simplicity).
        let confidence = 0.8;
        let word_count = result.text.split_whitespace().count();

        // Direct plausibility check for the Regulation low-confidence alert: non-empty
        // but near-empty output is likely a hallucination or garbage. Replaces
        // the former `ocr_quality_heuristic < 0.3` trigger.
        if !result.text.trim().is_empty() && word_count < 5 {
            tracing::warn!(
                target: "reg.pipeline.ocr.low_confidence",
                page_index = page_index,
                word_count,
                model = %model,
                "LLM OCR produced near-empty non-blank output — possible hallucination or poor image quality"
            );
        }

        Ok(OcrResult {
            page_index,
            backend: backend.clone(),
            text: result.text,
            confidence,
            duration_ms,
            was_fallback: is_fallback,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hkask_inference::InferenceConfig;
    use image::{ImageBuffer, RgbImage};

    /// Create a simple test image.
    fn test_image() -> DynamicImage {
        let img: RgbImage = ImageBuffer::new(100, 100);
        DynamicImage::ImageRgb8(img)
    }

    fn test_executor() -> LlmOcrExecutor {
        LlmOcrExecutor::new(Arc::new(InferenceRouter::new(InferenceConfig::from_env())))
    }

    #[test]
    fn is_available_for_llm_ocr() {
        let executor = test_executor();
        // Circuit breaker starts closed, so should be available
        assert!(executor.is_available(&OcrBackend::LlmOcr("any-model".into())));
    }

    #[test]
    fn is_available_false_for_other_backends() {
        let executor = test_executor();
        assert!(!executor.is_available(&OcrBackend::Tesseract));
    }

    #[test]
    fn is_available_false_when_circuit_open() {
        let executor = test_executor();
        executor.force_open_circuit();
        assert!(
            !executor.is_available(&OcrBackend::LlmOcr("any-model".into())),
            "LLM OCR should be unavailable when circuit breaker is open"
        );
    }

    #[tokio::test]
    async fn execute_rejects_wrong_backend() {
        let executor = test_executor();
        let image = test_image();
        let result = executor
            .execute(0, &OcrBackend::Tesseract, &image, false)
            .await;
        assert!(result.is_err());
    }
}
