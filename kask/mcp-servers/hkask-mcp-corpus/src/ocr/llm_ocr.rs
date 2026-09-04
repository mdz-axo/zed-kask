//! Vision LLM Backend — OCR via hkask-inference vision models.
//!
//! Sends page images as base64-encoded PNG to vision-capable LLMs
//! through the inference router. Supports provider-prefixed model names
//! (RunPod/, FW/, ollama/) for backend routing.
//!
//! Includes a circuit breaker for rate-limit resilience: after N consecutive
//! 429 responses, all LLM requests pause for a cooldown period.
use async_trait::async_trait;

use crate::ocr::{OcrBackend, OcrResult};
use base64::Engine;
use hkask_types::{InferencePort, template::LLMParameters};
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
pub(crate) fn build_ocr_prompt() -> String {
    let vars = std::collections::HashMap::new();
    let rendered = crate::render_docproc_template("ocr-extract", &vars);
    if rendered.is_empty() {
        OCR_FALLBACK_PROMPT.to_string()
    } else {
        rendered
    }
}

/// Encode `bytes` as base64 and dispatch a single OCR vision call via `router`,
/// returning the extracted text.
///
/// This is the single OCR vision primitive — the one enforcement point for
/// the empty-is-failure contract: an HTTP 200 with empty content is a typed
/// `EmptyOcrOutput` error, never an `Ok("")`. Callers own their own
/// post-processing (circuit-breaker integration, `OcrResult` assembly).
pub(crate) async fn vision_ocr_bytes(
    router: &dyn InferencePort,
    bytes: &[u8],
    model: &str,
) -> Result<String, OcrError> {
    let b64_data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let params = LLMParameters {
        temperature: 0.1,
        ..Default::default()
    };
    let result = router
        .generate_vision(&build_ocr_prompt(), &[b64_data], &params, Some(model))
        .await
        .map_err(|e| OcrError::InferenceFailed(e.to_string()))?;
    if result.text.trim().is_empty() {
        return Err(OcrError::EmptyOcrOutput {
            model: model.to_string(),
            input_bytes: bytes.len(),
        });
    }
    Ok(result.text)
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
    /// Consecutive openings without an intervening success — drives the
    /// exponential backoff below.
    consecutive_openings: AtomicU64,
    /// Consecutive failures before opening.
    threshold: u64,
    /// Base cooldown duration in seconds; escalated per consecutive
    /// opening up to [`CircuitBreaker::MAX_COOLDOWN_SECS`].
    cooldown_secs: u64,
}

impl CircuitBreaker {
    /// Hard cap for the escalated cooldown.
    const MAX_COOLDOWN_SECS: u64 = 300;

    const fn new(threshold: u64, cooldown_secs: u64) -> Self {
        Self {
            failures: AtomicU64::new(0),
            cooldown_until: AtomicI64::new(0),
            consecutive_openings: AtomicU64::new(0),
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

    /// Record a successful request — resets the failure counter and the
    /// backoff escalation.
    fn record_success(&self) {
        self.failures.store(0, Ordering::Relaxed);
        self.cooldown_until.store(0, Ordering::Relaxed);
        self.consecutive_openings.store(0, Ordering::Relaxed);
    }

    /// Record a failure. If the threshold is reached, open the circuit.
    ///
    /// The cooldown escalates exponentially per consecutive opening
    /// (base × 2^(openings-1), capped): a repeatedly-tripping breaker on a
    /// long book run otherwise re-burns one doomed vision call every fixed
    /// cooldown window — a dead endpoint taxed a 412-page run for its
    /// full duration at 30s intervals.
    fn record_failure(&self) {
        let count = self.failures.fetch_add(1, Ordering::Relaxed) + 1;
        if count >= self.threshold {
            let openings = self.consecutive_openings.fetch_add(1, Ordering::Relaxed) + 1;
            let shift = (openings - 1).min(4);
            let cooldown_secs = self
                .cooldown_secs
                .saturating_mul(1_u64 << shift)
                .min(Self::MAX_COOLDOWN_SECS);
            let until = now_unix() + cooldown_secs as i64;
            self.cooldown_until.store(until, Ordering::Relaxed);
            tracing::warn!(
                target: "reg.pipeline.ocr.circuit_breaker",
                failures = count,
                consecutive_openings = openings,
                cooldown_secs,
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

/// Records OCR health events to the cross-process health file read by the
/// cybernetics loop's `BridgeOcrHealthSource`.
///
/// The corpus server is a subprocess — its `reg.pipeline.ocr.silent_failure`
/// tracing warns never reach the zed main process's regulation sensors. This
/// recorder is the write side of the `hkask_types::ocr_health` file contract:
/// every silent failure (empty LLM output) and every circuit-breaker state
/// change is appended atomically (tmp+rename) so the loop's `OcrHealthSensor`
/// can sense OCR degradation storms instead of reporting `signal_count=0`.
pub(crate) struct OcrHealthRecorder {
    path: std::path::PathBuf,
    state: std::sync::Mutex<hkask_types::ocr_health::OcrHealthSnapshot>,
}

impl OcrHealthRecorder {
    /// Create a recorder writing to `path`. Callers in the server wiring use
    /// `hkask_types::ocr_health::ocr_health_path()`; tests pass an explicit path.
    pub fn new(path: std::path::PathBuf) -> Self {
        Self {
            path,
            state: std::sync::Mutex::new(hkask_types::ocr_health::OcrHealthSnapshot::default()),
        }
    }

    /// Record one silent failure (empty LLM output on a page) at the current
    /// time and persist the snapshot.
    pub fn record_silent_failure(&self) {
        let mut state = self.state.lock().expect("OCR health state mutex poisoned");
        state.record_silent_failure(now_unix());
        self.persist(&mut state);
    }

    /// Record a circuit-breaker state transition. A no-op when the state is
    /// unchanged — the success path calls this after every page, and only a
    /// genuine transition (opened or closed) should touch the file.
    pub fn record_breaker_state(&self, open: bool) {
        let mut state = self.state.lock().expect("OCR health state mutex poisoned");
        if state.circuit_breaker_open == open {
            return;
        }
        state.circuit_breaker_open = open;
        self.persist(&mut state);
    }

    /// Serialize + atomically publish the snapshot (tmp+rename). A write
    /// failure is warned, never silently dropped — an unwritable health file
    /// means the regulation loop is blind to OCR degradation, which the
    /// operator must be able to distinguish from "no events".
    fn persist(&self, state: &mut hkask_types::ocr_health::OcrHealthSnapshot) {
        state.updated_unix = now_unix();
        let json = match serde_json::to_string(&*state) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(
                    target: "reg.pipeline.ocr.health",
                    path = %self.path.display(),
                    error = %error,
                    "Failed to serialize OCR health snapshot — regulation loop is blind to OCR degradation"
                );
                return;
            }
        };
        // Self-healing posture (D28): every write path creates its parent.
        if let Some(parent) = self.path.parent()
            && let Err(error) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(
                target: "reg.pipeline.ocr.health",
                path = %self.path.display(),
                error = %error,
                "Failed to create OCR health file parent — regulation loop is blind to OCR degradation"
            );
            return;
        }
        let temp_path = self.path.with_extension("json.tmp");
        if let Err(error) = std::fs::write(&temp_path, json) {
            tracing::warn!(
                target: "reg.pipeline.ocr.health",
                path = %temp_path.display(),
                error = %error,
                "Failed to write OCR health snapshot — regulation loop is blind to OCR degradation"
            );
            return;
        }
        if let Err(error) = std::fs::rename(&temp_path, &self.path) {
            tracing::warn!(
                target: "reg.pipeline.ocr.health",
                path = %self.path.display(),
                error = %error,
                "Failed to publish OCR health snapshot — regulation loop is blind to OCR degradation"
            );
        }
    }
}

/// Vision LLM OCR executor using the hkask-inference router.
///
/// Encodes page images as base64 PNG and dispatches to vision-capable
/// models via `generate_vision`. Supports all inference backends
/// (RunPod, OpenRouter) through provider-prefixed model names.
///
/// The router is constructed once and shared across all concurrent
/// OCR tasks via `Arc<dyn InferencePort>`.
pub(crate) struct LlmOcrExecutor {
    /// Shared inference port (constructed once, used by all concurrent tasks).
    router: Arc<dyn InferencePort>,
    /// Circuit breaker for rate-limit resilience.
    breaker: CircuitBreaker,
    /// Write side of the cross-process OCR health file. `None` in tests —
    /// the recorder only exists in the server wiring.
    recorder: Option<Arc<OcrHealthRecorder>>,
    /// Adaptive ramp-up gate for the remote LLM service (AIMD: floor 2,
    /// +1 per success, halve per failure, ceiling = `HKASK_MAX_CONCURRENCY`).
    /// Process-lifetime: learns the endpoint's real capacity across runs
    /// instead of re-probing per book.
    limiter: crate::batch::AdaptiveLimiter,
}

impl LlmOcrExecutor {
    /// Create a new LLM OCR executor with a shared inference port.
    pub fn new(router: Arc<dyn InferencePort>) -> Self {
        Self {
            router,
            breaker: CircuitBreaker::new(5, 30), // 5 consecutive failures → 30s cooldown
            recorder: None,
            limiter: crate::batch::AdaptiveLimiter::new(
                crate::max_concurrency(),
                crate::batch::ADAPTIVE_CONCURRENCY_FLOOR,
            ),
        }
    }

    /// Attach the cross-process health recorder (the server wiring path —
    /// tests construct without it).
    #[must_use = "builder methods must be chained or assigned"]
    pub fn with_health_recorder(mut self, recorder: Arc<OcrHealthRecorder>) -> Self {
        self.recorder = Some(recorder);
        self
    }

    /// Whether the LLM OCR circuit breaker is currently open (LLM attempts
    /// paused after consecutive failures). Stamped into pipeline outcomes so
    /// a run that skipped every LLM page because of an open breaker is
    /// distinguishable in the tool result from one that never routed to the
    /// LLM at all.
    pub fn breaker_open(&self) -> bool {
        !self.breaker.is_closed()
    }

    /// Current adaptive LLM concurrency allowance — observability for tests
    /// and for the `reg.batch.concurrency` ramp events.
    pub fn adaptive_concurrency(&self) -> usize {
        self.limiter.current()
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

        // Remote-service gate: the adaptive limiter ramps LLM concurrency
        // (floor → ceiling on success, halved on failure) instead of
        // launching every in-flight page at the ceiling. The slot reports
        // the call's outcome and releases its in-flight count on drop.
        let slot = self.limiter.acquire().await;
        let result = vision_ocr_bytes(&*self.router, &img_bytes, &model).await;
        match &result {
            Ok(_) => slot.report_success(),
            Err(_) => slot.report_failure(),
        }

        // Circuit-breaker + rate-limit tracking on the vision-call outcome. The
        // breaker reacts to rate-limit, timeout, connection errors, AND empty
        // output — a dead-but-responsive endpoint (HTTP 200 with empty content)
        // must be quarantined like a transport failure, not reset the breaker
        // as a success. The rate-limit warn fires only for backpressure
        // (GAP-4 Regulation variety).
        match &result {
            Ok(_) => {
                self.breaker.record_success();
                // No-op unless the breaker just closed after a quarantine —
                // the recorder skips unchanged state.
                if let Some(ref recorder) = self.recorder {
                    recorder.record_breaker_state(false);
                }
            }
            // Empty output is classified as a typed failure by
            // `vision_ocr_bytes` (the single enforcement point). Count it
            // against the breaker so a dead-but-responsive endpoint is
            // quarantined like a transport failure, and record the silent
            // failure for the regulation loop's health file.
            Err(OcrError::EmptyOcrOutput { model, input_bytes }) => {
                self.breaker.record_failure();
                tracing::warn!(
                    target: "reg.pipeline.ocr.silent_failure",
                    page_index = page_index,
                    llm_model = %model,
                    input_bytes = input_bytes,
                    "OCR model returned empty output — treating as failure, degrading to fallback backend"
                );
                if let Some(ref recorder) = self.recorder {
                    recorder.record_silent_failure();
                    recorder.record_breaker_state(!self.breaker.is_closed());
                }
                return Err(OcrError::EmptyOcrOutput {
                    model: model.clone(),
                    input_bytes: *input_bytes,
                });
            }
            Err(OcrError::InferenceFailed(err_str)) => {
                let is_rate_limit = err_str.contains("429")
                    || err_str.contains("rate limit")
                    || err_str.contains("Rate limit");
                if is_rate_limit {
                    tracing::warn!(
                        target: "reg.pipeline.ocr.rate_limit",
                        model = %model,
                        page_index = page_index,
                        "OCR inference rate-limited — circuit breaker tracking"
                    );
                }
                if is_rate_limit || err_str.contains("timed out") || err_str.contains("connection")
                {
                    self.breaker.record_failure();
                }
                if let Some(ref recorder) = self.recorder {
                    recorder.record_breaker_state(!self.breaker.is_closed());
                }
            }
            Err(_) => {}
        }

        let text = result?;
        let duration_ms = start.elapsed().as_millis() as u64;

        // Vision OCR models don't expose real per-token confidence via chat
        // completions, so any number here is a placeholder. Use a fixed nominal
        // rather than an invented 3-factor heuristic (hkask P5: simplicity).
        let confidence = 0.8;
        let word_count = text.split_whitespace().count();

        // Direct plausibility check for the Regulation low-confidence alert: non-empty
        // but near-empty output is likely a hallucination or garbage. Replaces
        // the former `ocr_quality_heuristic < 0.3` trigger.
        if !text.trim().is_empty() && word_count < 5 {
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
            text,
            confidence,
            duration_ms,
            was_fallback: is_fallback,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_health_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("ocr-recorder-test-{name}-{}", std::process::id()))
    }

    fn read_snapshot(path: &std::path::Path) -> hkask_types::ocr_health::OcrHealthSnapshot {
        let contents = std::fs::read_to_string(path).expect("health file must exist");
        serde_json::from_str(&contents).expect("health file must parse")
    }

    #[test]
    fn silent_failures_are_persisted_with_timestamps() {
        let path = temp_health_path("failures");
        let recorder = OcrHealthRecorder::new(path.clone());
        recorder.record_silent_failure();
        recorder.record_silent_failure();
        recorder.record_silent_failure();

        let snapshot = read_snapshot(&path);
        assert_eq!(snapshot.silent_failure_timestamps.len(), 3);
        // All entries are recent (within a minute of now).
        let now = now_unix();
        assert!(
            snapshot
                .silent_failure_timestamps
                .iter()
                .all(|&ts| now - ts < 60)
        );
        assert!(!snapshot.circuit_breaker_open);
    }

    #[test]
    fn breaker_state_transitions_persist_and_no_ops_skip_the_write() {
        let path = temp_health_path("breaker");
        let recorder = OcrHealthRecorder::new(path.clone());

        recorder.record_breaker_state(true);
        let after_open = std::fs::read_to_string(&path).expect("open transition writes");
        assert!(read_snapshot(&path).circuit_breaker_open);

        // Same state again — no write, file content unchanged.
        recorder.record_breaker_state(true);
        assert_eq!(
            std::fs::read_to_string(&path).expect("file still readable"),
            after_open,
            "an unchanged breaker state must not touch the file"
        );

        recorder.record_breaker_state(false);
        assert!(!read_snapshot(&path).circuit_breaker_open);
    }
}
