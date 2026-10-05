//! Vision LLM Backend — OCR via hkask-inference vision models.
//!
//! Sends page images as base64-encoded PNG to vision-capable LLMs
//! through the inference router. Supports provider-prefixed model names
//! (RunPod/, FW/, ollama/) for backend routing.
//!
//! Includes a circuit breaker for rate-limit resilience: after N consecutive
//! 429 responses, all LLM requests pause for a cooldown period.
use async_trait::async_trait;

use crate::ocr::OcrResult;
use base64::Engine;
use hkask_types::{InferenceError, InferencePort, template::LLMParameters};
use image::DynamicImage;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

use crate::ocr::pipeline::{OcrError, OcrExecutor};

/// Load the one deployed OCR prompt. Missing instructions are a deployment
/// failure, not permission to dispatch a different inline prompt.
pub(crate) fn build_ocr_prompt() -> Result<String, OcrError> {
    let vars = std::collections::HashMap::new();
    let rendered = crate::render_docproc_template("ocr-extract", &vars);
    if rendered.is_empty() {
        Err(OcrError::TemplateUnavailable)
    } else {
        Ok(rendered)
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
        // Batch page OCR queues at the provider past the chat-oriented global
        // deadline (2026-10-03 zk-ref build: pages completing <300s solo
        // exceeded the 600s global in full runs at every concurrency); the
        // workload class that knows its horizon sets it. Env-tunable per build.
        timeout_hint_secs: Some(ocr_timeout_hint_secs()),
        ..Default::default()
    };
    let prompt = build_ocr_prompt()?;
    let result = router
        .generate_vision(&prompt, &[b64_data], &params, Some(model))
        .await
        .map_err(|e| OcrError::Inference { source: e })?;
    crate::helpers::record_usage(&result);
    if result.text.trim().is_empty() {
        return Err(OcrError::EmptyOcrOutput {
            model: model.to_string(),
            input_bytes: bytes.len(),
        });
    }
    Ok(result.text)
}

/// The OCR workload's admission-to-completion budget, honored by the
/// server-side inference port up to its cap. Default 1200s (2x the typical
/// 600s global) absorbs the observed provider-queue band; a malformed
/// override warns naming the value rather than silently falling back.
fn ocr_timeout_hint_secs() -> u64 {
    const DEFAULT_HINT_SECS: u64 = 1200;
    match std::env::var("HKASK_OCR_INFERENCE_TIMEOUT_SECS") {
        Ok(value) => match value.parse::<u64>() {
            Ok(secs) => secs,
            Err(_) => {
                tracing::warn!(
                    target: "reg.pipeline.ocr",
                    value = %value,
                    "HKASK_OCR_INFERENCE_TIMEOUT_SECS malformed — using default {DEFAULT_HINT_SECS}"
                );
                DEFAULT_HINT_SECS
            }
        },
        Err(_) => DEFAULT_HINT_SECS,
    }
}

/// Encode a page image as PNG bytes — the input contract of
/// [`vision_ocr_bytes`] (`LanguageModelImage` requires base64 PNG; JPEG
/// bytes under a PNG MIME are dropped by strict decoders).
fn encode_page_png(image: &DynamicImage, model: &str) -> Result<Vec<u8>, OcrError> {
    let mut img_bytes: Vec<u8> = Vec::new();
    image
        .write_to(
            &mut std::io::Cursor::new(&mut img_bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| OcrError::BackendFailed {
            model: model.to_string(),
            message: format!("Failed to encode page image as PNG: {e}"),
        })?;
    Ok(img_bytes)
}

/// Apply the model's `rotation_correction` to a page image. The correction
/// is COUNTER-clockwise degrees (measured live: ieee-1012 pages 15/51
/// requested 270, a clockwise 270 application left them 180 off and the
/// retry requested 180 — wave-ocr-fix6, 2026-10-03); the image crate
/// rotates clockwise, so apply the complement.
fn apply_rotation_correction(image: DynamicImage, correction: i16) -> DynamicImage {
    match correction {
        90 => image.rotate270(),
        180 => image.rotate180(),
        270 => image.rotate90(),
        // The parser admits only 0/90/180/270 and 0 never reaches here;
        // unreachable in practice.
        _ => image,
    }
}

/// Circuit breaker for rate-limit resilience.
///
/// After `threshold` consecutive failures, opens: the executor reports the
/// cooldown to the adaptive limiter, which holds new vision calls until it
/// passes. A sensor/accumulator only — the limiter is the one actuation
/// point (the 2026-10-02 spiral was two actuators fighting: this breaker
/// fail-fasting pages while the limiter froze, unpausing, re-tripping).
/// Embedded in `LlmOcrExecutor`.
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
    /// Record a failure. Returns `Some(cooldown_secs)` when this failure
    /// opened (or re-opened) the breaker — the caller actuates the adaptive
    /// limiter's pause with it, so breaker state and concurrency regulation
    /// share one actuation path.
    fn record_failure(&self) -> Option<u64> {
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
            Some(cooldown_secs)
        } else {
            None
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
    /// +1 per success, halve per failure, pause while any breaker is open,
    /// ceiling = `HKASK_MAX_CONCURRENCY`). Process-lifetime: learns the
    /// endpoint's real capacity across runs instead of re-probing per book.
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

    /// Whether the LLM OCR circuit breaker is currently open. Stamped into
    /// pipeline outcomes for observability: while it is open the adaptive
    /// limiter is paused, so a run reporting `llm_breaker_open: true` WAITED
    /// OUT a cooldown rather than losing pages to it (the pre-fix
    /// fail-fast page loss was the 2026-10-02 spiral's other arm).
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
    async fn execute(
        &self,
        page_index: usize,
        model: &str,
        image: &DynamicImage,
    ) -> Result<OcrResult, OcrError> {
        let model = model.to_string();

        // Encode as PNG: `LanguageModelImage`'s contract is base64 PNG and
        // `to_base64_url` hardcodes the `data:image/png` MIME. JPEG bytes under
        // a PNG MIME are dropped by strict decoders (the RunPod OLMOCR-2 proxy
        // returned empty output for exactly this reason; ollama happens to
        // sniff the real format). Preserve color and normalize the longest
        // edge to the page protocol's input size for both PDFs and images.
        let normalized = DynamicImage::ImageRgb8(
            image
                .resize(
                    crate::ocr::OCR_IMAGE_LONG_EDGE,
                    crate::ocr::OCR_IMAGE_LONG_EDGE,
                    image::imageops::FilterType::Lanczos3,
                )
                .to_rgb8(),
        );
        // Pre-call blankness gate: a confidently blank page (no
        // recoverable ink — `ocr::blank`) is a fact about the source, not
        // a conversion failure. Skip the vision call entirely — no limiter
        // slot, no generation, no deadline risk (the 2026-10-03 zk-ref
        // class: empty and near-blank pages burned full deadline budgets
        // under load for ~0 words). The threshold is deliberately
        // conservative: stamps and sparse pages stay content-ambiguous
        // and go to the model.
        if crate::ocr::blank::is_blank_page(&normalized) {
            tracing::info!(
                target: "reg.pipeline.ocr.blank",
                page_index,
                "page pre-detected blank — skipping the vision call"
            );
            return Ok(OcrResult::blank_page(page_index));
        }
        // Remote-service gate: the adaptive limiter ramps LLM concurrency
        // (floor → ceiling on success, halved on failure, paused while any
        // breaker is open) instead of launching every in-flight page at the
        // ceiling. The ONE outcome classifier routes the call's result — a
        // CircuitOpen verdict pauses here — and the slot releases its
        // in-flight count on drop. A rotation request is the protocol
        // working, not endpoint distress: the re-render retry stays inside
        // the acquired slot (the endpoint already answered the first call)
        // and the classifier reports the FINAL outcome only — no spurious
        // halve for the rotated-page round trip.
        let slot = self.limiter.acquire().await;
        let mut current = normalized;
        let mut rotation_retried = false;
        let result = loop {
            let img_bytes = encode_page_png(&current, &model)?;
            let attempt = vision_ocr_bytes(&*self.router, &img_bytes, &model)
                .await
                .and_then(|raw| super::response::parse_page_response(&raw));
            match attempt {
                Err(OcrError::RotationRequested { correction }) if !rotation_retried => {
                    rotation_retried = true;
                    tracing::info!(
                        target: "reg.pipeline.ocr.rotation",
                        page_index,
                        correction,
                        "page requested rotation — re-rendering rotated and retrying once"
                    );
                    current = apply_rotation_correction(current, correction);
                    continue;
                }
                outcome => break outcome,
            }
        };
        slot.report_inference_outcome(&result);

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
                if let Some(cooldown_secs) = self.breaker.record_failure() {
                    self.limiter
                        .report_breaker_open(Duration::from_secs(cooldown_secs));
                }
                tracing::warn!(
                    target: "reg.pipeline.ocr.silent_failure",
                    page_index = page_index,
                    llm_model = %model,
                    input_bytes = input_bytes,
                    "OCR model returned empty output — treating as failure, no fallback backend exists"
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
            Err(OcrError::Inference { source }) => {
                // Preserve HTTP status, DNS, TLS, or timeout details so the
                // operator can distinguish a broken endpoint from empty text.
                tracing::warn!(
                    target: "reg.pipeline.ocr.inference_failure",
                    page_index = page_index,
                    llm_model = %model,
                    error = %source,
                    "OCR inference call failed — no fallback backend exists"
                );
                let is_rate_limit = source.to_string().contains("429")
                    || source.to_string().contains("rate limit")
                    || source.to_string().contains("Rate limit");
                if is_rate_limit {
                    tracing::warn!(
                        target: "reg.pipeline.ocr.rate_limit",
                        model = %model,
                        page_index = page_index,
                        "OCR inference rate-limited — circuit breaker tracking"
                    );
                }
                // The shared bridge breaker's own verdict (CircuitOpen) is
                // actuated by the outcome classifier's pause above — one
                // signal, one clock. Counting it here too would re-grow the
                // spiral's scoreboard (the 94 consecutive openings came from
                // exactly that double-counting). Only DIRECT endpoint
                // failures (HTTP, timeout, connection) accumulate here; a
                // dead-but-erroring endpoint (404/5xx) is the same class as
                // a dead-but-responding one (200-empty).
                if !matches!(source, InferenceError::CircuitOpen(_)) {
                    if let Some(cooldown_secs) = self.breaker.record_failure() {
                        self.limiter
                            .report_breaker_open(Duration::from_secs(cooldown_secs));
                    }
                }
                if let Some(ref recorder) = self.recorder {
                    recorder.record_breaker_state(!self.breaker.is_closed());
                }
            }
            Err(OcrError::InvalidResponse(reason)) => {
                tracing::warn!(target: "reg.pipeline.ocr.protocol", page_index, model = %model, reason = %reason, "OCR response rejected; no plain-text fallback");
            }
            // No silent swallows: pre-call failures (wrong backend, JPEG
            // encoding) surface with their cause too.
            Err(other) => {
                tracing::warn!(
                    target: "reg.pipeline.ocr.inference_failure",
                    page_index = page_index,
                    llm_model = %model,
                    error = %other,
                    "OCR LLM executor failed before the vision call — no fallback backend exists"
                );
            }
        }

        let response = result?;
        let word_count = response.text.split_whitespace().count();

        // Direct plausibility check for the Regulation low-confidence alert: non-empty
        // but near-empty output is likely a hallucination or garbage. Replaces
        // the former `ocr_quality_heuristic < 0.3` trigger.
        if !response.text.trim().is_empty() && word_count < 5 {
            tracing::warn!(
                target: "reg.pipeline.ocr.low_confidence",
                page_index = page_index,
                word_count,
                model = %model,
                "LLM OCR produced near-empty non-blank output — possible hallucination or poor image quality"
            );
        }

        Ok(OcrResult::from_response(page_index, model, response))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The breaker's cooldown escalates per consecutive opening (base ×
    /// 2^(openings-1), capped at 300s) and a success resets the escalation.
    /// A fixed 30s cooldown let a dead endpoint tax a 412-page run for its
    /// full duration — one doomed vision call every cooldown window.
    #[test]
    fn breaker_cooldown_escalates_on_consecutive_openings() {
        let breaker = CircuitBreaker::new(5, 30);

        let cooldown_after = |failures: usize| {
            for _ in 0..failures {
                breaker.record_failure();
            }
            let until = breaker
                .cooldown_until
                .load(std::sync::atomic::Ordering::Relaxed);
            (until - now_unix()).max(0) as u64
        };

        // First opening (5 failures): base cooldown.
        let first = cooldown_after(5);
        assert!(
            first <= 30,
            "first opening uses the base cooldown, got {first}"
        );
        // Each subsequent failure re-opens with escalation: 60, 120, 240,
        // then capped at 300.
        assert_eq!(cooldown_after(1), 60, "second opening doubles");
        assert_eq!(cooldown_after(1), 120, "third opening doubles again");
        assert_eq!(cooldown_after(1), 240, "fourth opening doubles again");
        assert_eq!(cooldown_after(1), 300, "fifth opening hits the cap");
        assert_eq!(cooldown_after(1), 300, "cap holds");

        // A success resets the failure counter and the escalation.
        breaker.record_success();
        let reset = cooldown_after(5);
        assert!(reset <= 30, "success resets the backoff, got {reset}");
    }

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

    /// A dead-but-erroring endpoint (HTTP 404) must open the circuit breaker
    /// like a dead-but-responding one (200-empty). The former substring
    /// filter ("timed out"/"connection") missed HTTP errors entirely — a
    /// 404ing endpoint was hammered on every page with a doomed call and
    /// logged nothing (observed 2026-09-04: the kask-ocr endpoint 404s).
    struct HttpErrorVisionPort;

    impl hkask_types::InferencePort for HttpErrorVisionPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "noop — only generate_vision is under test".into(),
                ))
            })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(hkask_types::InferenceError::Connection(
                    "HTTP 404: The requested path was not found.".into(),
                ))
            })
        }
    }

    /// expect: [P4] Missing deployed OCR instructions must fail before the provider, even inside a developer checkout.
    #[tokio::test]
    async fn ocr_requires_deployed_template() -> anyhow::Result<()> {
        const CHILD: &str = "HKASK_TEST_OCR_TEMPLATE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let error = vision_ocr_bytes(&HttpErrorVisionPort, &[1], "fixture/ocr")
                .await
                .expect_err("missing template");
            assert!(
                error.to_string().contains("Required OCR template"),
                "{error}"
            );
            return Ok(());
        }
        let dir = tempfile::tempdir()?;
        for configured in [false, true] {
            let mut child = tokio::process::Command::new(std::env::current_exe()?);
            child
                .args([
                    "--exact",
                    "ocr::llm_ocr::tests::ocr_requires_deployed_template",
                ])
                .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
                .env(CHILD, "1")
                .env_remove("HKASK_TEMPLATE_ROOT");
            if configured {
                child.env("HKASK_TEMPLATE_ROOT", dir.path().join("missing"));
            }
            let result = child.output().await?;
            assert!(
                result.status.success(),
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn http_error_failures_open_the_breaker() {
        crate::helpers::seed_registry_template_root();
        let executor = LlmOcrExecutor::new(Arc::new(HttpErrorVisionPort));
        let image = crate::helpers::inked_page();
        for _ in 0..5 {
            assert!(
                executor
                    .execute(0, "RunPod/kask-ocr", &image)
                    .await
                    .is_err(),
                "each vision call must fail"
            );
        }
        assert!(
            executor.breaker_open(),
            "5 consecutive HTTP-error failures must open the breaker"
        );
    }

    /// The shared bridge breaker's verdict arrives as a typed
    /// `InferenceError::CircuitOpen`. It must pause the adaptive limiter
    /// (the next page waits out the horizon) WITHOUT counting toward the
    /// executor's own breaker — one signal, one clock (the 2026-10-02
    /// spiral double-counted it into 94 consecutive openings).
    struct CircuitOpenVisionPort;

    impl hkask_types::InferencePort for CircuitOpenVisionPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::Connection(
                    "noop — only generate_vision is under test".into(),
                ))
            })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::CircuitOpen(
                    "transient inference failure threshold reached; retry after 30s".into(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn circuit_open_pauses_the_limiter_and_not_the_local_breaker() {
        crate::helpers::seed_registry_template_root();
        let executor = LlmOcrExecutor::new(Arc::new(CircuitOpenVisionPort));
        let image = crate::helpers::inked_page();
        let error = executor
            .execute(0, "RunPod/kask-ocr", &image)
            .await
            .expect_err("a CircuitOpen vision call must fail the page");
        assert!(
            matches!(&error, OcrError::Inference { source } if matches!(source, InferenceError::CircuitOpen(_))),
            "the typed verdict must survive to the caller, got: {error}"
        );
        assert!(
            !executor.breaker_open(),
            "the local breaker must not count the shared breaker's own verdict"
        );
        assert_eq!(
            executor.adaptive_concurrency(),
            crate::batch::ADAPTIVE_CONCURRENCY_FLOOR,
            "the pause must snap the allowance to the floor"
        );
        // The next page WAITS OUT the pause instead of failing fast into the
        // open breaker — no page is lost to a transient outage.
        let blocked = tokio::time::timeout(
            std::time::Duration::from_millis(300),
            executor.execute(1, "RunPod/kask-ocr", &image),
        )
        .await;
        assert!(blocked.is_err(), "execute must block on the limiter pause");
    }

    /// A vision port that counts calls and always refuses — the
    /// blank-gate test asserts the call count stays at zero for blanks.
    #[derive(Default)]
    struct CountingVisionPort {
        calls: std::sync::atomic::AtomicUsize,
    }

    impl hkask_types::InferencePort for CountingVisionPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {
                Err(hkask_types::InferenceError::NotConfigured(
                    "counting port: only generate_vision is wired".into(),
                ))
            })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, hkask_types::InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Box::pin(async {
                Err(hkask_types::InferenceError::NotConfigured(
                    "the blank gate must skip this call".into(),
                ))
            })
        }
    }

    /// A blank page never reaches the vision call: the executor answers it
    /// locally with a blank-flagged result — no limiter slot, no
    /// generation. A page with text-sized ink still goes to the model.
    #[tokio::test]
    async fn blank_pages_skip_the_vision_call() {
        crate::helpers::seed_registry_template_root();
        let port = Arc::new(CountingVisionPort::default());
        let executor = LlmOcrExecutor::new(Arc::clone(&port) as Arc<dyn InferencePort>);

        let blank = DynamicImage::new_rgb8(64, 64);
        let result = executor
            .execute(0, "RunPod/kask-ocr", &blank)
            .await
            .expect("a blank page is a successful no-content observation");
        assert!(result.blank, "the result must carry the blank flag");
        assert!(result.text.is_empty());
        assert_eq!(result.page_index, 0);
        assert_eq!(
            port.calls.load(Ordering::Relaxed),
            0,
            "a blank page must not reach the vision call"
        );

        // A page with text-sized ink is content-ambiguous: it goes to the
        // model (the counting port refuses — the call count is the
        // assertion, not the outcome).
        let text_page = crate::helpers::inked_page();
        let outcome = executor.execute(1, "RunPod/kask-ocr", &text_page).await;
        assert!(
            matches!(outcome, Err(OcrError::Inference { .. })),
            "the non-blank page reaches the (refusing) vision port, got: {outcome:?}"
        );
        assert_eq!(
            port.calls.load(Ordering::Relaxed),
            1,
            "exactly the non-blank page reaches the vision call"
        );
    }

    /// First call requests a 90° rotation; the retry on the re-rendered page
    /// returns valid protocol text. Pins the recovery path end-to-end.
    struct RotatingThenValidVisionPort {
        calls: AtomicU64,
    }

    impl hkask_types::InferencePort for RotatingThenValidVisionPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::Connection(
                    "noop — only generate_vision is under test".into(),
                ))
            })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let text = if n == 0 {
                    "---\nprimary_language: en\nis_rotation_valid: False\nrotation_correction: 90\nis_table: False\nis_diagram: False\n---\nrotated".to_string()
                } else {
                    "---\nprimary_language: en\nis_rotation_valid: True\nrotation_correction: 0\nis_table: False\nis_diagram: False\n---\n\nrecovered page text".to_string()
                };
                Ok(hkask_types::InferenceResult {
                    text,
                    model: "fixture/ocr".to_string(),
                    usage: hkask_types::InferenceUsage::default(),
                    finish_reason: "stop".to_string(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn rotation_request_re_renders_and_recovers_the_page() {
        crate::helpers::seed_registry_template_root();
        let port = Arc::new(RotatingThenValidVisionPort {
            calls: AtomicU64::new(0),
        });
        let executor = LlmOcrExecutor::new(port.clone());
        let image = crate::helpers::inked_page();
        let result = executor
            .execute(0, "RunPod/kask-ocr", &image)
            .await
            .expect("the rotated page must recover on the re-render retry");
        assert_eq!(result.text, "recovered page text");
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            2,
            "exactly one re-render retry"
        );
    }

    /// The OCR vision call must budget its admission-to-completion horizon
    /// past the chat-oriented global deadline — the request carries a timeout
    /// hint the server-side port honors (batch OCR queues at the provider;
    /// 2026-10-03 zk-ref build: pages completing <300s solo exceeded the
    /// 600s global in full runs).
    struct HintCapturingVisionPort {
        hints: std::sync::Mutex<Vec<Option<u64>>>,
    }

    impl hkask_types::InferencePort for HintCapturingVisionPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::Connection(
                    "noop — only generate_vision is under test".into(),
                ))
            })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            self.hints
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(parameters.timeout_hint_secs);
            Box::pin(async {
                Err(InferenceError::Connection(
                    "the call outcome is irrelevant — the hint is the assertion".into(),
                ))
            })
        }
    }

    #[tokio::test]
    async fn ocr_vision_calls_carry_a_timeout_hint() {
        crate::helpers::seed_registry_template_root();
        let port = Arc::new(HintCapturingVisionPort {
            hints: std::sync::Mutex::new(Vec::new()),
        });
        let executor = LlmOcrExecutor::new(port.clone());
        let image = crate::helpers::inked_page();
        let _ = executor.execute(0, "RunPod/kask-ocr", &image).await;
        let hints = port
            .hints
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            hints.first(),
            Some(&Some(1200)),
            "the OCR workload must budget 1200s (2x the typical 600s global) for provider queueing"
        );
    }

    /// A page that still requests rotation after the retry fails the page —
    /// the retry is bounded to one, never a loop.
    struct AlwaysRotatingVisionPort {
        calls: AtomicU64,
    }

    impl hkask_types::InferencePort for AlwaysRotatingVisionPort {
        fn generate(
            &self,
            _prompt: &str,
            _parameters: &LLMParameters,
            _tools: Option<&[hkask_types::ChatToolDefinition]>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            Box::pin(async {
                Err(InferenceError::Connection(
                    "noop — only generate_vision is under test".into(),
                ))
            })
        }

        fn generate_vision(
            &self,
            _prompt: &str,
            _images: &[String],
            _parameters: &LLMParameters,
            _model_override: Option<&str>,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<hkask_types::InferenceResult, InferenceError>,
                    > + Send
                    + '_,
            >,
        > {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async {
                Ok(hkask_types::InferenceResult {
                    text: "---\nprimary_language: en\nis_rotation_valid: False\nrotation_correction: 90\nis_table: False\nis_diagram: False\n---\nstill rotated".to_string(),
                    model: "fixture/ocr".to_string(),
                    usage: hkask_types::InferenceUsage::default(),
                    finish_reason: "stop".to_string(),
                    tool_calls: Vec::new(),
                    reasoning: None,
                    cost_usd: None,
                })
            })
        }
    }

    #[tokio::test]
    async fn rotation_retry_is_bounded_to_one() {
        crate::helpers::seed_registry_template_root();
        let port = Arc::new(AlwaysRotatingVisionPort {
            calls: AtomicU64::new(0),
        });
        let executor = LlmOcrExecutor::new(port.clone());
        let image = crate::helpers::inked_page();
        let error = executor
            .execute(0, "RunPod/kask-ocr", &image)
            .await
            .expect_err("a page still requesting rotation after the retry must fail");
        assert!(
            matches!(error, OcrError::RotationRequested { correction: 90 }),
            "the typed rotation request must survive to the caller, got: {error}"
        );
        assert_eq!(
            port.calls.load(Ordering::SeqCst),
            2,
            "exactly one retry, never a rotation loop"
        );
    }

    /// The model's correction is counter-clockwise degrees; the image crate
    /// rotates clockwise, so the application must be the complement. A
    /// clockwise application of the raw value leaves the page 180 off
    /// (wave-ocr-fix6: pages requesting 270 then, after a clockwise 270
    /// application, requesting 180). Pins the direction with an asymmetric
    /// fixture: red left / blue right, correction 90 → blue top / red bottom.
    #[test]
    fn rotation_correction_applies_the_counter_clockwise_complement() {
        use image::GenericImageView;
        let mut img = image::RgbImage::new(2, 1);
        img.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        img.put_pixel(1, 0, image::Rgb([0, 0, 255]));
        let img = DynamicImage::ImageRgb8(img);
        let rotated = apply_rotation_correction(img, 90);
        assert_eq!(rotated.dimensions(), (1, 2));
        assert_eq!(
            rotated.get_pixel(0, 0),
            image::Rgba([0, 0, 255, 255]),
            "the right (blue) pixel must become the top row under a 90-degree counter-clockwise correction"
        );
        assert_eq!(
            rotated.get_pixel(0, 1),
            image::Rgba([255, 0, 0, 255]),
            "the left (red) pixel must become the bottom row"
        );
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
